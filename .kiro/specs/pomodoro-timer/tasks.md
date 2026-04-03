# 任务列表：番茄钟功能

## 任务

- [-] 1. 数据类型与共享模型
  - [-] 1.1 在 `src/pomodoro_backend.rs` 中定义 `Phase`、`TimerState`、`PomodoroConfig`、`PomodoroSession` 结构体，添加 `serde` 派生和 `Default` 实现
  - [ ] 1.2 在 `src/components/pomodoro.rs` 中引入上述类型（客户端侧）
  - [ ] 1.3 在 `Cargo.toml` 的 `[dev-dependencies]` 中添加 `proptest = "1"`

- [ ] 2. RESP 协议客户端（`src/resp.rs`）
  - [ ] 2.1 定义 `RespValue` 枚举（`SimpleString`、`Error`、`Integer`、`BulkString`、`Array`）和 `RespError` 枚举，均加 `#[cfg(feature = "server")]`
  - [ ] 2.2 实现 `write_command`：将 `&[&str]` 编码为 RESP Array 格式字节流写入 `TcpStream`
  - [ ] 2.3 实现 `read_response`：从 `TcpStream` 按首字节（`+`、`-`、`:`、`$`、`*`）解析为 `RespValue`
  - [ ] 2.4 实现 `RespClient` 结构体，含 `tokio::sync::Mutex<Option<TcpStream>>` 和 `addr: String`
  - [ ] 2.5 实现 `ensure_connected`：懒初始化 TCP 连接，使用 `tokio::time::timeout(5s)` 包裹
  - [ ] 2.6 实现 `send_command`：调用 `ensure_connected`，写命令，读响应；连接断开时重置为 `None` 并返回 `RespError::ConnectionError`
  - [ ] 2.7 在 `main.rs` 中声明 `mod resp;`，在 `pomodoro_backend.rs` 中引入

- [ ] 3. Redis 状态管理（`RedisStore`）
  - [ ] 3.1 定义全局 `static REDIS_CLIENT: OnceLock<RespClient>`，在服务端启动时通过 `REDIS_URL` 环境变量初始化
  - [ ] 3.2 实现 `redis_get_timer_state(email: &str) -> Result<TimerState, ServerFnError>`：GET key，不存在时写入默认值并返回
  - [ ] 3.3 实现 `redis_set_timer_state(email: &str, state: &TimerState) -> Result<(), ServerFnError>`：SET key JSON
  - [ ] 3.4 实现 `redis_get_config(email: &str) -> Result<PomodoroConfig, ServerFnError>`：GET config key，不存在时返回 `Default::default()`
  - [ ] 3.5 实现 `redis_set_config(email: &str, config: &PomodoroConfig) -> Result<(), ServerFnError>`

- [ ] 4. SQLite 会话存储（`SessionStore`）
  - [ ] 4.1 在 `backend.rs` 的 `DB` 初始化 SQL 中追加 `pomodoro_sessions` 表和索引的 `CREATE TABLE IF NOT EXISTS` 语句
  - [ ] 4.2 实现 `save_session(session: &PomodoroSession) -> Result<(), ServerFnError>`：INSERT 到 `pomodoro_sessions`
  - [ ] 4.3 实现 `query_sessions(email: &str) -> Result<Vec<PomodoroSession>, ServerFnError>`：SELECT 最近 20 条，按 `end_time DESC`，LEFT JOIN tasks 获取 `task_name`

- [ ] 5. Server Functions（`src/pomodoro_backend.rs`）
  - [ ] 5.1 实现 `get_timer_state(email: String) -> Result<TimerState, ServerFnError>`：读取 Redis，若 `is_running=true` 则计算 `remaining = stored - (now - started_at)`；若 `remaining <= 0` 则推进阶段、持久化 session、更新 Redis
  - [ ] 5.2 实现 `start_timer(email: String) -> Result<(), ServerFnError>`：设 `is_running=true`，`started_at=now_unix`
  - [ ] 5.3 实现 `pause_timer(email: String) -> Result<(), ServerFnError>`：计算并写回 `remaining_secs`，设 `is_running=false`，`started_at=0`
  - [ ] 5.4 实现 `reset_timer(email: String) -> Result<(), ServerFnError>`：从 config 读取当前阶段时长，重置 `remaining_secs`，设 `is_running=false`
  - [ ] 5.5 实现 `update_config(email: String, config: PomodoroConfig) -> Result<(), ServerFnError>`：验证配置值在 [60, 3600] 范围内，仅 `is_running=false` 时更新 Redis 中的 config 和当前阶段 `remaining_secs`
  - [ ] 5.6 实现 `list_sessions(email: String) -> Result<Vec<PomodoroSession>, ServerFnError>`：调用 `query_sessions`
  - [ ] 5.7 在 `main.rs` 中声明 `mod pomodoro_backend;`

- [ ] 6. 阶段推进辅助函数
  - [ ] 6.1 实现 `next_phase(current: &Phase, focus_count: u32) -> (Phase, u32)`：返回下一阶段和新的 `focus_count`
  - [ ] 6.2 实现 `phase_duration(phase: &Phase, config: &PomodoroConfig) -> u32`：返回阶段对应秒数
  - [ ] 6.3 实现 `validate_config_secs(secs: u32) -> Result<(), String>`：检查 [60, 3600] 范围

- [ ] 7. 前端组件（`src/components/pomodoro.rs`）
  - [ ] 7.1 实现 `PomodoroPage` 根组件：从 Context 获取 `user` email，提供 `tick` signal（`use_effect` 每秒 +1），通过 `use_resource` 驱动 `get_timer_state` 轮询
  - [ ] 7.2 实现 `TimerDisplay`：接收 `TimerState` prop，显示 MM:SS 格式剩余时间、阶段名称（"专注中"/"短休息"/"长休息"）、已完成番茄数
  - [ ] 7.3 实现 `TimerControls`：开始/暂停/重置按钮，根据 `is_running` 切换显示，点击调用对应 server function 后触发 resource 刷新
  - [ ] 7.4 实现 `TaskSelector`：下拉框列出当前用户任务（复用 `list_tasks`），选择后更新本地 `task_id` signal
  - [ ] 7.5 实现 `HistoryPanel`：通过 `use_resource` 调用 `list_sessions`，展示日期、任务名、阶段、时长；空列表时显示"暂无记录"
  - [ ] 7.6 实现 `ConfigPanel`：三个数字输入框（专注/短休息/长休息分钟数），从 `localStorage["pomodoro_config"]` 初始化，保存时验证并调用 `update_config`
  - [ ] 7.7 在 `use_effect` 中实现浏览器通知：请求权限，检测前后两次 `phase` 变化时发送 `Notification`（使用 `web-sys` 的 `Notification`）
  - [ ] 7.8 在 `use_effect` 中实现 `document.title` 更新（计时运行时显示剩余时间和阶段名）

- [ ] 8. 路由与导航集成
  - [ ] 8.1 在 `src/main.rs` 的 `Route` 枚举中添加 `#[route("/pomodoro")] PomodoroPage {}` 路由，纳入 `AuthLayout + NavBar` 布局
  - [ ] 8.2 在 `src/components/nav.rs` 的 `NavBar` 中添加指向 `Route::PomodoroPage` 的链接，文字为"番茄钟"
  - [ ] 8.3 在 `src/components/mod.rs` 中添加 `pub mod pomodoro; pub use pomodoro::*;`

- [ ] 9. web-sys 扩展
  - [ ] 9.1 在 `Cargo.toml` 的 `web-sys` features 中添加 `"Notification"`，用于浏览器通知 API

- [ ] 10. 属性测试（`src/resp.rs` 和 `src/pomodoro_backend.rs` 的 `#[cfg(test)]` 模块）
  - [ ] 10.1 编写属性测试：`prop_resp_roundtrip` — 任意命令参数编码后解析得到等价值（验证属性 4）
  - [ ] 10.2 编写属性测试：`prop_timer_state_serde_roundtrip` — 任意 `TimerState` 序列化后反序列化等价（验证属性 5）
  - [ ] 10.3 编写属性测试：`prop_phase_progression` — 任意 `focus_count` 的推进序列正确（验证属性 6）
  - [ ] 10.4 编写属性测试：`prop_config_validation` — 越界值被拒绝，合法值被接受（验证属性 8）
  - [ ] 10.5 编写属性测试：`prop_sessions_ordered_desc` — 任意会话集合查询结果降序且 ≤ 20 条（验证属性 9）
  - [ ] 10.6 编写属性测试：`prop_format_mm_ss` — 任意秒数格式化结果符合 MM:SS 模式（验证属性 3）
  - [ ] 10.7 编写单元测试：`test_start_pause_roundtrip` — 开始后立即暂停，remaining_secs 误差 ≤ 1 秒（验证属性 1）
  - [ ] 10.8 编写单元测试：`test_reset_restores_initial` — 重置后 remaining_secs 等于配置时长（验证属性 2）
  - [ ] 10.9 编写单元测试：`test_focus_completion_saves_session` — 专注归零时 session 被持久化（验证属性 10）
  - [ ] 10.10 编写单元测试：`test_user_isolation` — 操作用户 A 不影响用户 B 的状态（验证属性 7）
  - [ ] 10.11 编写单元测试：`test_config_localStorage_roundtrip` — 配置序列化写入再读取等价（验证属性 11）
