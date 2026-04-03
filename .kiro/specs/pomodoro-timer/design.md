# 设计文档：番茄钟功能

## 概述

在现有 Dioxus 0.7 全栈任务管理应用中集成番茄钟功能。采用**服务端计时 + 客户端轮询**架构：计时器状态存储于 Redis，服务端通过时间戳动态推算剩余时间，客户端每秒轮询获取最新状态。

核心设计决策：
- 不依赖第三方 Redis crate，自实现 RESP 协议客户端（`src/resp.rs`）
- 服务端以 `tokio::sync::Mutex<Option<TcpStream>>` 持有 Redis 连接，懒初始化
- 客户端用 `use_signal` tick 计数驱动 `use_resource` 每秒重新拉取状态
- 阶段推进逻辑完全在服务端 `get_timer_state` 中执行，无需后台进程

---

## 架构

```mermaid
graph TD
    subgraph 客户端 (WASM)
        A[PomodoroPage] --> B[TimerDisplay]
        A --> C[TimerControls]
        A --> D[TaskSelector]
        A --> E[HistoryPanel]
        A --> F[ConfigPanel]
        G[use_signal tick] -->|每秒+1| H[use_resource]
        H -->|调用| SF1[get_timer_state]
    end

    subgraph 服务端 (Axum)
        SF1 --> RS[RedisStore]
        SF2[start_timer] --> RS
        SF3[pause_timer] --> RS
        SF4[reset_timer] --> RS
        SF5[update_config] --> RS
        SF6[list_sessions] --> SS[SessionStore]
        RS --> RC[RespClient]
        RS --> SS
        RC -->|RESP over TCP| Redis[(Redis)]
        SS -->|rusqlite| SQLite[(SQLite)]
    end

    C -->|onclick| SF2
    C -->|onclick| SF3
    C -->|onclick| SF4
    F -->|onsubmit| SF5
    E -->|onload| SF6
```

**数据流（轮询路径）：**
1. 客户端 tick signal 每秒递增 → `use_resource` 重新执行
2. 调用 `get_timer_state(email)` server function
3. 服务端 `RedisStore` 读取 Redis 中的 `TimerState` JSON
4. 若 `is_running=true`，计算 `remaining = stored_remaining - (now - started_at)`
5. 若 `remaining <= 0`，推进阶段、持久化 session、更新 Redis
6. 返回最新 `TimerState` 给客户端渲染

---

## 组件与接口

### 新增文件

| 文件 | 职责 |
|------|------|
| `src/resp.rs` | RESP 协议编解码、RespClient、全局连接管理 |
| `src/pomodoro_backend.rs` | 番茄钟 server functions、RedisStore、SessionStore |
| `src/components/pomodoro.rs` | 番茄钟前端组件树 |

### 组件树

```
PomodoroPage
├── TimerDisplay        // 显示 MM:SS、阶段名称、番茄计数
├── TimerControls       // 开始/暂停/重置按钮
├── TaskSelector        // 关联任务下拉框（复用 list_tasks）
├── HistoryPanel        // 最近 20 条会话记录
└── ConfigPanel         // 时长配置表单（localStorage 持久化）
```

### Server Functions 接口

```rust
// 获取当前计时器状态（含阶段推进逻辑）
#[server]
async fn get_timer_state(email: String) -> Result<TimerState, ServerFnError>

// 开始计时：写入 started_at = now，is_running = true
#[server]
async fn start_timer(email: String) -> Result<(), ServerFnError>

// 暂停计时：计算并写回 remaining_secs，is_running = false
#[server]
async fn pause_timer(email: String) -> Result<(), ServerFnError>

// 重置当前阶段：remaining_secs = 阶段初始时长，is_running = false
#[server]
async fn reset_timer(email: String) -> Result<(), ServerFnError>

// 更新时长配置（仅 is_running=false 时生效）
#[server]
async fn update_config(email: String, config: PomodoroConfig) -> Result<(), ServerFnError>

// 查询历史会话（最近 20 条，按 end_time 降序）
#[server]
async fn list_sessions(email: String) -> Result<Vec<PomodoroSession>, ServerFnError>
```

### RespClient 接口

```rust
// src/resp.rs（仅编译到 server feature）
#[cfg(feature = "server")]
pub struct RespClient {
    stream: tokio::sync::Mutex<Option<tokio::net::TcpStream>>,
    addr: String,
}

impl RespClient {
    pub fn new(addr: String) -> Self
    pub async fn send_command(&self, args: &[&str]) -> Result<RespValue, RespError>
    async fn ensure_connected(&self) -> Result<(), RespError>  // 懒初始化
    async fn write_command(stream: &mut TcpStream, args: &[&str]) -> Result<(), RespError>
    async fn read_response(stream: &mut TcpStream) -> Result<RespValue, RespError>
}
```

---

## 数据模型

### Rust 类型定义

```rust
// 共享（client + server）
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TimerState {
    pub phase: Phase,
    pub remaining_secs: u32,
    pub is_running: bool,
    pub started_at: u64,      // Unix 时间戳，is_running=false 时为 0
    pub focus_count: u32,     // 当前循环已完成的专注次数（0-3）
    pub task_id: Option<i64>, // 关联任务 ID
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PomodoroConfig {
    pub focus_secs: u32,       // 默认 1500
    pub short_break_secs: u32, // 默认 300
    pub long_break_secs: u32,  // 默认 900
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        Self { focus_secs: 1500, short_break_secs: 300, long_break_secs: 900 }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PomodoroSession {
    pub id: Option<i64>,
    pub user_email: String,
    pub task_id: Option<i64>,
    pub task_name: Option<String>, // JOIN 查询填充，不存储
    pub phase: Phase,
    pub start_time: String,  // ISO 8601
    pub end_time: String,
    pub duration_secs: u32,
}
```

### RESP 协议类型（仅 server）

```rust
#[cfg(feature = "server")]
#[derive(Debug, Clone, PartialEq)]
pub enum RespValue {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Option<String>), // None = Null Bulk String ($-1)
    Array(Vec<RespValue>),
}

#[cfg(feature = "server")]
#[derive(Debug, thiserror::Error)]
pub enum RespError {
    #[error("连接错误: {0}")]
    ConnectionError(#[from] std::io::Error),
    #[error("协议解析错误: {0}")]
    ParseError(String),
    #[error("Redis 错误响应: {0}")]
    RedisError(String),
    #[error("超时")]
    Timeout,
}
```

### SQLite Schema

```sql
-- 在现有 DB 初始化中追加
CREATE TABLE IF NOT EXISTS pomodoro_sessions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_email  TEXT    NOT NULL,
    task_id     INTEGER,                    -- 可为 NULL
    phase       TEXT    NOT NULL,           -- 'Focus' | 'ShortBreak' | 'LongBreak'
    start_time  TEXT    NOT NULL,           -- ISO 8601
    end_time    TEXT    NOT NULL,
    duration_secs INTEGER NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_user_end
    ON pomodoro_sessions(user_email, end_time DESC);
```

### Redis Key 结构

```
pomodoro:{user_email}        → TimerState JSON（SET/GET）
pomodoro:config:{user_email} → PomodoroConfig JSON（SET/GET）
```

---

## 正确性属性

*属性（Property）是在系统所有合法执行路径上都应成立的特征或行为——本质上是对系统应做什么的形式化陈述。属性是人类可读规范与机器可验证正确性保证之间的桥梁。*

### 属性 1：开始-暂停往返保持时间守恒

*对任意* `TimerState`（`is_running=false`），执行 `start_timer` 后立即（0 秒内）执行 `pause_timer`，写回的 `remaining_secs` 应与开始前相同（误差 ≤ 1 秒），且 `is_running` 为 `false`。

此属性同时验证了 start_timer 将 `is_running` 置为 `true`（中间状态）以及 pause_timer 正确计算并写回剩余时间。

**验证需求：1.2, 1.3**

---

### 属性 2：重置后状态恢复初始值

*对任意* `TimerState` 和 `PomodoroConfig`，调用 `reset_timer` 后，`remaining_secs` 必须等于当前阶段对应的配置时长，`is_running` 必须为 `false`，`started_at` 必须为 0。

**验证需求：1.4**

---

### 属性 3：MM:SS 格式化正确性

*对任意* `remaining_secs`（0 到 3600 之间），格式化函数输出必须匹配 `^\d{2}:\d{2}$` 模式，且分钟部分等于 `remaining_secs / 60`，秒部分等于 `remaining_secs % 60`。

**验证需求：1.6**

---

### 属性 4：RESP 编解码往返

*对任意* 合法的 Redis 命令参数列表（非空字符串数组），将其编码为 RESP 字节流后再解析，应得到语义等价的 `RespValue`（`BulkString` 或 `Array` 变体，内容与原参数一致）。

**验证需求：2.2, 2.3**

---

### 属性 5：TimerState JSON 序列化往返

*对任意* `TimerState` 值，序列化为 JSON 字符串后再反序列化，应得到与原值字段完全相等的结构体。

**验证需求：3.1**

---

### 属性 6：阶段推进序列正确性

*对任意* 初始 `focus_count`（0-3），连续触发阶段归零推进，阶段序列必须满足：
- Focus（focus_count < 3）→ ShortBreak → Focus
- Focus（focus_count = 3）→ LongBreak → Focus（focus_count 重置为 0）
- 每次推进后 `is_running` 必须为 `false`

**验证需求：5.1, 5.2, 5.3, 5.4**

---

### 属性 7：用户状态隔离

*对任意* 两个不同 email 的用户 A 和 B，对用户 A 执行任意计时器操作（start/pause/reset），用户 B 的 `TimerState` 在操作前后必须保持不变。

**验证需求：4.3**

---

### 属性 8：配置验证拒绝越界值

*对任意* 不在 [60, 3600] 秒范围内的配置值，`validate_config_secs` 函数必须返回错误；对任意在 [60, 3600] 范围内的值，必须返回 `Ok`。

**验证需求：9.4**

---

### 属性 9：历史记录降序且数量上限

*对任意* 用户的会话集合（数量任意），`list_sessions` 返回的结果必须：（1）按 `end_time` 严格降序排列；（2）数量不超过 20 条。

**验证需求：8.1, 8.2**

---

### 属性 10：专注阶段完成触发会话持久化

*对任意* 正在运行的专注阶段（`Phase::Focus`），当服务端检测到剩余时间归零并推进阶段时，`pomodoro_sessions` 表中必须新增恰好一条记录，且该记录的 `duration_secs` 等于专注阶段的配置时长，`phase` 字段为 `"Focus"`。

**验证需求：7.1**

---

### 属性 11：配置 localStorage 往返

*对任意* 合法的 `PomodoroConfig`（所有字段在 [60, 3600] 范围内），将其序列化后写入 `localStorage["pomodoro_config"]`，再读取并反序列化，应得到与原值相等的配置。

**验证需求：9.2, 9.3**

---

## 错误处理

| 错误场景 | 处理方式 |
|----------|----------|
| Redis 连接失败（初始化） | `RespError::ConnectionError`，server function 返回 `ServerFnError`，客户端显示"Redis 连接失败"提示 |
| Redis 读写超时（5 秒） | `RespError::Timeout`，重置连接（将 `Mutex<Option<TcpStream>>` 置为 `None`），下次调用重新连接 |
| Redis key 不存在（首次访问） | `RedisStore` 自动写入默认 `TimerState`，返回初始状态 |
| JSON 反序列化失败 | 视为 key 损坏，删除并重新初始化 |
| SQLite 写入失败（会话持久化） | 记录错误日志，不中断计时器，客户端显示非阻塞警告 |
| 配置值越界 | server function 返回 `ServerFnError::Response`，客户端显示验证错误 |
| 用户未登录访问 `/pomodoro` | `AuthLayout` 重定向到 `/login` |
| 浏览器通知权限被拒绝 | 仅页面内视觉提示，不重复请求权限 |

### 连接重试策略

`RespClient` 在 `ensure_connected` 中实现懒初始化：
- 首次调用时建立 TCP 连接
- 连接断开后，下次 `send_command` 调用时自动重连（最多重试 1 次）
- 使用 `tokio::time::timeout(Duration::from_secs(5), ...)` 包裹所有 IO 操作

---

## 测试策略

### 双轨测试方法

**单元测试**（具体示例和边界条件）：
- `RespClient` 编码输出格式验证（固定输入 → 固定字节序列）
- `TimerState` 默认值验证
- 阶段推进边界：`focus_count=3` 时触发长休息
- 配置验证：边界值 60 秒（合法）、59 秒（非法）
- 历史记录为空时返回空列表

**属性测试**（使用 `proptest` crate）：
- 每个属性测试最少运行 100 次迭代
- 每个测试用注释标注对应设计属性

```toml
# 仅在 dev-dependencies 中添加
[dev-dependencies]
proptest = "1"
```

### 属性测试配置示例

```rust
// Feature: pomodoro-timer, Property 4: RESP 编解码往返
proptest! {
    #[test]
    fn prop_resp_roundtrip(args in arb_command_args()) {
        // 编码 → 解析 → 验证语义等价
    }
}

// Feature: pomodoro-timer, Property 5: TimerState JSON 序列化往返
proptest! {
    #[test]
    fn prop_timer_state_serde_roundtrip(state in arb_timer_state()) {
        let json = serde_json::to_string(&state).unwrap();
        let restored: TimerState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, restored);
    }
}

// Feature: pomodoro-timer, Property 6: 阶段推进序列正确性
proptest! {
    #[test]
    fn prop_phase_progression(focus_count in 0u32..4u32) {
        // 验证从任意 focus_count 开始的推进序列
    }
}

// Feature: pomodoro-timer, Property 8: 配置验证拒绝越界值
proptest! {
    #[test]
    fn prop_config_validation_rejects_out_of_range(
        secs in prop::num::u32::ANY.prop_filter("out of range", |&s| s < 60 || s > 3600)
    ) {
        assert!(validate_config_secs(secs).is_err());
    }
}

// Feature: pomodoro-timer, Property 9: 历史记录按时间降序
proptest! {
    #[test]
    fn prop_sessions_ordered_desc(sessions in arb_sessions(1..=20)) {
        // 插入乱序会话，查询后验证降序
    }
}
```

### 集成测试

- 使用内存 SQLite（`rusqlite::Connection::open_in_memory()`）测试 `SessionStore`
- 使用 mock `RespClient`（trait object）测试 `RedisStore` 逻辑
- 端到端：启动本地 Redis，验证完整的开始→暂停→重置流程

### Cargo.toml 变更

```toml
# 新增 dev-dependencies
[dev-dependencies]
proptest = "1"

# server feature 中 tokio 已通过 dioxus/server 引入，无需额外添加
# 无需新增 Redis 依赖
```
