# 需求文档：番茄钟功能

## 简介

为现有的 Dioxus 0.7 全栈任务管理应用添加番茄钟（Pomodoro Timer）功能。番茄钟是一种时间管理方法：专注工作 25 分钟（一个"番茄"），然后休息 5 分钟，每完成 4 个番茄后进行一次长休息（15 分钟）。

本功能使用 **Redis 服务端计时**方案：计时器状态（剩余秒数、当前阶段、运行状态、开始时间戳）存储在 Redis 中，客户端通过每秒轮询服务端 server function `get_timer_state` 获取最新状态，服务端根据存储的 `started_at` 时间戳动态计算剩余时间，无需后台 tick 进程。服务端仅在 `is_running=true` 时通过 `now - started_at` 实时推算剩余秒数，并在检测到 `remaining_secs <= 0` 时自动推进阶段。

此方案天然支持多设备同步：用户在任意设备打开番茄钟页面，客户端轮询均可获取同一份服务端状态。服务端同时负责将完成的专注会话持久化到 SQLite。

**技术栈补充：**
- 自实现 RESP 协议客户端：基于 `tokio::net::TcpStream`，不依赖任何第三方 Redis crate
- 服务端连接持有：`tokio::sync::Mutex<Option<TcpStream>>`，支持懒初始化
- 客户端轮询：`use_signal` tick 计数每秒递增，驱动 `use_resource` 重新触发

---

## 词汇表

- **PomodoroTimer**：番茄钟组件，负责渲染计时器 UI 并通过轮询展示服务端状态
- **PomodoroSession**：一次完整的番茄钟会话记录，包含开始时间、结束时间、关联任务、阶段类型
- **Phase**：番茄钟当前阶段，枚举值为 `Focus`（专注）、`ShortBreak`（短休息）、`LongBreak`（长休息）
- **Cycle**：一个完整的番茄循环，由 4 个 `Focus` 阶段和对应的休息阶段组成
- **TimerState**：Redis 中存储的计时器状态结构，包含字段：`phase`、`remaining_secs`、`is_running`、`started_at`（Unix 时间戳，`is_running=false` 时为 0）
- **RespClient**：自实现的 RESP 协议 TCP 客户端，基于 `tokio::net::TcpStream`，负责编码 RESP 命令并解析 Redis 响应
- **RespValue**：RESP 协议响应的 Rust 枚举表示，包含 `SimpleString`、`Error`、`Integer`、`BulkString`、`Array` 五种类型
- **RedisStore**：服务端 Redis 数据层，通过 **RespClient** 与 Redis 通信，负责读写 `TimerState`，key 格式为 `pomodoro:{user_email}`
- **SessionStore**：服务端 SQLite 中存储 `PomodoroSession` 的数据层
- **User**：已登录用户，通过 Context API 中的 `Signal<Option<String>>` 标识

---

## 需求

### 需求 1：服务端计时与客户端轮询

**用户故事：** 作为用户，我希望在浏览器中看到实时倒计时，以便掌握当前专注或休息的剩余时间。

#### 验收标准

1. THE **PomodoroTimer** SHALL 在页面加载后调用 server function `get_timer_state`，若 Redis 中不存在该用户的 `TimerState`，THE **RedisStore** SHALL 以 25 分钟（1500 秒）、`Focus` 阶段、`is_running=false` 初始化并写入 Redis。
2. WHEN 用户点击"开始"按钮，THE **PomodoroTimer** SHALL 调用 server function `start_timer`，THE **RedisStore** SHALL 将 `is_running` 设为 `true` 并记录当前 Unix 时间戳到 `started_at`。
3. WHEN 用户点击"暂停"按钮，THE **PomodoroTimer** SHALL 调用 server function `pause_timer`，THE **RedisStore** SHALL 计算当前剩余秒数并写回 `remaining_secs`，将 `is_running` 设为 `false`，`started_at` 设为 0。
4. WHEN 用户点击"重置"按钮，THE **PomodoroTimer** SHALL 调用 server function `reset_timer`，THE **RedisStore** SHALL 将当前阶段的 `remaining_secs` 恢复为该阶段初始时长，`is_running` 设为 `false`。
5. WHILE 页面处于活跃状态，THE **PomodoroTimer** SHALL 每秒通过 `use_signal` tick 计数驱动 `use_resource` 重新调用 `get_timer_state`，将返回的 `TimerState` 渲染到 UI。
6. THE **PomodoroTimer** SHALL 以 `MM:SS` 格式显示 `TimerState.remaining_secs`。
7. WHILE 计时器运行中，THE **PomodoroTimer** SHALL 将页面标题（`document.title`）更新为当前剩余时间和阶段名称。

---

### 需求 2：自实现 RESP 协议客户端

**用户故事：** 作为开发者，我希望使用自行实现的 RESP 协议工具与 Redis 通信，以便不依赖第三方 Redis 客户端库，深入理解协议细节。

#### 验收标准

1. THE **RespClient** SHALL 使用 `tokio::net::TcpStream` 建立与 Redis 服务器的 TCP 连接，连接地址通过环境变量 `REDIS_URL`（默认 `127.0.0.1:6379`）配置。
2. THE **RespClient** SHALL 实现 RESP 内联命令编码：将命令及参数序列化为 `*{argc}\r\n${len}\r\n{arg}\r\n...` 格式的字节流并写入 TCP 流。
3. THE **RespClient** SHALL 实现 RESP 响应解析：从 TCP 流读取字节，根据首字节（`+`、`-`、`:`、`$`、`*`）解析为对应的 `RespValue` 枚举变体。
4. THE **RespClient** SHALL 提供异步方法 `send_command(args: &[&str]) -> Result<RespValue, RespError>`，封装编码、发送、接收、解析的完整流程。
5. THE **RespClient** SHALL 支持以下 Redis 命令的发送与响应解析：`GET`、`SET`、`DEL`、`EXISTS`，足以支撑 `TimerState` 的读写操作。
6. IF TCP 连接断开或读写超时（超时阈值 5 秒），THEN THE **RespClient** SHALL 返回 `RespError::ConnectionError`，上层 `RedisStore` 捕获后返回 `ServerFnError`。
7. THE **RespClient** SHALL 在服务端以 `tokio::sync::Mutex<Option<TcpStream>>` 持有连接，支持懒初始化（首次调用时建立连接）。

---

### 需求 3：Redis 状态管理

**用户故事：** 作为用户，我希望计时器状态由服务端维护，以便在页面刷新或切换设备后状态不丢失。

#### 验收标准

1. THE **RedisStore** SHALL 以 `pomodoro:{user_email}` 为 key，将 `TimerState` 序列化为 JSON 存储在 Redis 中。
2. WHEN `get_timer_state` 被调用且 `TimerState.is_running=true`，THE **RedisStore** SHALL 通过 `now_unix - started_at` 计算已流逝秒数，返回 `remaining_secs - elapsed` 作为当前剩余时间，不修改 Redis 中的存储值。
3. WHEN `get_timer_state` 被调用且计算所得剩余时间 `<= 0`，THE **RedisStore** SHALL 自动推进阶段（按 Focus → ShortBreak → Focus 循环，每 4 个 Focus 后切换为 LongBreak），更新 Redis 中的 `TimerState`，并将新阶段的 `remaining_secs` 设为该阶段初始时长，`is_running` 设为 `false`。
4. IF Redis 连接失败，THEN THE **RedisStore** SHALL 返回 `ServerFnError`，THE **PomodoroTimer** SHALL 在页面上显示连接错误提示，不中断已有 UI 渲染。
5. THE **RedisStore** SHALL 在服务端启动时通过 **RespClient** 懒初始化 TCP 连接，并通过 Dioxus 服务端 Context 注入到所有 server function 中。

---

### 需求 4：多设备同步

**用户故事：** 作为用户，我希望在多台设备上打开番茄钟时看到相同的计时状态，以便在不同设备间无缝切换。

#### 验收标准

1. WHEN 用户在另一台设备打开番茄钟页面，THE **PomodoroTimer** SHALL 通过轮询 `get_timer_state` 自动获取 Redis 中的最新 `TimerState`，无需手动刷新。
2. WHEN 用户在一台设备执行开始、暂停或重置操作，THE **RedisStore** SHALL 立即更新 Redis 状态，其他设备的下一次轮询将反映该变更，延迟不超过 1 秒。
3. THE **RedisStore** SHALL 以用户 email 为维度隔离状态，不同用户的 `TimerState` 互不影响。

---

### 需求 5：阶段自动切换

**用户故事：** 作为用户，我希望番茄钟在一个阶段结束后自动切换到下一阶段，以便我无需手动操作就能遵循番茄工作法节奏。

#### 验收标准

1. WHEN `get_timer_state` 检测到专注阶段剩余时间归零，THE **RedisStore** SHALL 自动切换到短休息阶段（5 分钟），`is_running` 设为 `false`。
2. WHEN `get_timer_state` 检测到短休息阶段剩余时间归零，THE **RedisStore** SHALL 自动切换到下一个专注阶段，`is_running` 设为 `false`。
3. WHEN 连续完成 4 个专注阶段，THE **RedisStore** SHALL 在第 4 个专注阶段结束后切换到长休息阶段（15 分钟），`is_running` 设为 `false`。
4. WHEN 长休息阶段剩余时间归零，THE **RedisStore** SHALL 将已完成番茄计数重置为 0 并切换到新的专注阶段，`is_running` 设为 `false`。
5. WHEN 任意阶段切换发生，THE **PomodoroTimer** SHALL 在页面上显示当前阶段名称（"专注中"、"短休息"、"长休息"）。

---

### 需求 6：浏览器通知

**用户故事：** 作为用户，我希望在阶段切换时收到浏览器通知，以便我在切换到其他标签页时也能得到提醒。

#### 验收标准

1. WHEN 用户首次使用番茄钟，THE **PomodoroTimer** SHALL 请求浏览器通知权限（`Notification.requestPermission`）。
2. IF 用户已授予通知权限，THEN THE **PomodoroTimer** SHALL 在客户端检测到阶段切换（前后两次轮询返回的 `phase` 不同）时发送一条包含新阶段名称的浏览器通知。
3. IF 用户拒绝通知权限，THEN THE **PomodoroTimer** SHALL 仅在页面内以视觉方式提示阶段切换，不再重复请求权限。

---

### 需求 7：关联任务

**用户故事：** 作为用户，我希望将番茄钟与某个任务关联，以便记录我在哪个任务上花费了多少专注时间。

#### 验收标准

1. THE **PomodoroTimer** SHALL 提供一个下拉选择框，列出当前用户的所有任务。
2. WHEN 用户选择一个任务，THE **PomodoroTimer** SHALL 将该任务 ID 与当前番茄钟会话关联。
3. WHERE 未选择任务，THE **PomodoroTimer** SHALL 允许在无关联任务的情况下正常计时。

---

### 需求 7：会话持久化

**用户故事：** 作为用户，我希望每次完成一个专注阶段后自动保存记录，以便查看历史专注数据。

#### 验收标准

1. WHEN `get_timer_state` 检测到专注阶段（`Focus` Phase）剩余时间归零并推进阶段，THE **SessionStore** SHALL 在 SQLite 的 `pomodoro_sessions` 表中插入一条记录，包含：用户 ID、关联任务 ID（可为空）、开始时间、结束时间、阶段类型。
2. IF 服务端写入失败，THEN THE **PomodoroTimer** SHALL 在页面上显示错误提示，并不中断计时器继续运行。
3. WHERE 用户未登录，THE **PomodoroTimer** SHALL 禁用会话保存功能，仅提供本地计时。

---

### 需求 8：历史记录查看

**用户故事：** 作为用户，我希望查看我的番茄钟历史记录，以便了解自己的专注时间分布。

#### 验收标准

1. THE **PomodoroTimer** SHALL 提供一个历史记录面板，展示当前用户最近 20 条 `PomodoroSession` 记录。
2. THE **SessionStore** SHALL 按 `end_time` 降序返回会话列表。
3. WHEN 历史记录面板加载，THE **PomodoroTimer** SHALL 显示每条记录的：日期、关联任务名称（若有）、阶段类型、持续时长（分钟）。
4. IF 当前用户没有任何历史记录，THEN THE **PomodoroTimer** SHALL 显示"暂无记录"提示。

---

### 需求 9：时长配置

**用户故事：** 作为用户，我希望自定义各阶段时长，以便根据个人习惯调整番茄钟节奏。

#### 验收标准

1. THE **PomodoroTimer** SHALL 提供专注时长、短休息时长、长休息时长三个配置项，默认值分别为 25 分钟、5 分钟、15 分钟。
2. WHEN 用户修改配置并保存，THE **PomodoroTimer** SHALL 将配置持久化到 `localStorage`，键名为 `pomodoro_config`，并调用 server function 更新 Redis 中当前阶段的 `remaining_secs`（仅在 `is_running=false` 时生效）。
3. WHEN 页面重新加载，THE **PomodoroTimer** SHALL 从 `localStorage` 读取并应用已保存的配置。
4. IF 配置值不在 1 到 60 分钟范围内，THEN THE **PomodoroTimer** SHALL 拒绝保存并显示验证错误提示。

---

### 需求 10：路由集成

**用户故事：** 作为用户，我希望通过导航栏访问番茄钟页面，以便与现有任务管理功能无缝切换。

#### 验收标准

1. THE **Router** SHALL 在 `/pomodoro` 路径下注册番茄钟页面，并将其纳入 `AuthLayout` 和 `NavBar` 布局。
2. THE **NavBar** SHALL 在导航栏中添加指向 `/pomodoro` 的链接，链接文字为"番茄钟"。
3. WHERE 用户未登录，THE **AuthLayout** SHALL 将访问 `/pomodoro` 的请求重定向到 `/login`。
