use crate::components::{PomodoroRecord, PomodoroState, PomodoroStats, Task};
use dioxus::prelude::*;

#[cfg(feature = "server")]
use crate::components::{PomodoroPhase, Status};
#[cfg(feature = "server")]
use dioxus::logger::tracing::debug;

#[cfg(feature = "server")]
use bcrypt::{hash, verify, DEFAULT_COST};

#[cfg(feature = "server")]
thread_local! {
    pub static DB: rusqlite::Connection = {
        let conn = rusqlite::Connection::open("taskmanager.db").expect("Failed to open database");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY,
                email TEXT NOT NULL,
                password TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tasks (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                user_id INTEGER NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                content TEXT NOT NULL,
                status INTEGER NOT NULL,
                belongin_to INTEGER NOT NULL
            );
            ",
        ).unwrap();

        conn
    }
}

#[server]
pub async fn list_tasks(page: u32, page_size: u32) -> Result<(Vec<Task>, u32), ServerFnError> {
    let offset = (page.saturating_sub(1)) * page_size;

    let total = DB.with(|conn| {
        conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, u32>(0))
            .unwrap_or(0)
    });

    let tasks = DB.with(|conn| {
        conn.prepare(
            "SELECT t.id, t.name, t.user_id, t.start_time, t.end_time, t.content, t.status,
                    COALESCE(u.email, '未知') as assignee_name
             FROM tasks t
             LEFT JOIN users u ON t.user_id = u.id
             LIMIT ?1 OFFSET ?2",
        )
        .unwrap()
        .query_map(rusqlite::params![page_size, offset], |row| {
            let status_int: i64 = row.get(6)?;
            Ok(Task {
                id: Some(row.get::<_, i64>(0)? as u32),
                name: row.get(1)?,
                user_id: row.get(2)?,
                start_time: row.get(3)?,
                end_time: row.get(4)?,
                content: row.get(5)?,
                status: match status_int {
                    1 => Status::Doing,
                    2 => Status::Completed,
                    _ => Status::Idle,
                },
                assignee_name: row.get(7)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>()
    });

    Ok((tasks, total))
}

#[server]
pub async fn do_login(username: String, password: String) -> Result<String, ServerFnError> {
    let stored_hash = DB
        .with(|conn| {
            conn.query_row(
                "SELECT password FROM users WHERE email=?1",
                rusqlite::params![username],
                |row| row.get::<_, String>(0),
            )
            .ok()
        });

    match stored_hash {
        None => Err(ServerFnError::Response("账号或密码错误！".to_string())),
        Some(hash) => {
            let valid = verify(&password, &hash).map_err(|e| ServerFnError::new(e))?;
            if valid {
                Ok(username)
            } else {
                Err(ServerFnError::Response("邮箱或密码不正确".to_string()))
            }
        }
    }
}

#[server]
pub async fn do_register(username: String, password: String) -> Result<String, ServerFnError> {
    // 检查邮箱是否已注册
    let exists = DB.with(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM users WHERE email = ?1",
            rusqlite::params![username],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
    });

    if exists > 0 {
        return Err(ServerFnError::Response("该邮箱已被注册".to_string()));
    }

    // bcrypt 哈希密码（自动加盐）
    let hashed_pwd = hash(&password, DEFAULT_COST)
        .map_err(|e| ServerFnError::Response(e.to_string()))?;

    debug!("do_register: {}", username);

    DB.with(|conn| {
        conn.execute(
            "INSERT INTO users (email, password) VALUES (?1, ?2)",
            rusqlite::params![username, hashed_pwd],
        )
    })
    .map_err(|e| ServerFnError::Response(e.to_string()))?;

    Ok("注册成功，请登录".to_string())
}

#[server]
pub async fn list_users(keyword: String) -> Result<Vec<(i64, String)>, ServerFnError> {
    // LIKE 模糊匹配：把 % 拼进参数值，而不是 SQL 语句里
    let pattern = format!("%{}%", keyword);
    let users = DB.with(|conn| {
        conn.prepare("SELECT id, email FROM users WHERE email LIKE ?1")
            .unwrap()
            .query_map(rusqlite::params![pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>()
    });
    Ok(users)
}

#[server]
pub async fn save_task(name: String, content: String, start_time: String, end_time: String, user_id: i64, uname_belong: String) -> Result<String, ServerFnError> {
    let stored_id = DB
        .with(|conn| {
            conn.query_row(
                "SELECT id FROM users WHERE email=?1",
                rusqlite::params![uname_belong],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
        });

    let status: i8 = 0;
    DB.with(|conn| {
        conn.execute(
            "INSERT INTO tasks (name, user_id, start_time, end_time, content, status, belonging_to) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![name, user_id, start_time, end_time, content, status, stored_id],
        )
    }).map_err(|e| ServerFnError::Response(e.to_string()))?;

    Ok("任务保存成功！".to_string())
}

#[server]
pub async fn find_uname(user_id: i64) -> Result<String, ServerFnError> {
    let stored_name = DB
        .with(|conn| {
            conn.query_row(
                "SELECT name FROM users WHERE id=?1",
                rusqlite::params![user_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or("".to_string())
        });
    Ok(stored_name)
}

// ── Task CRUD Server Functions ─────────────────────────────────────

#[server]
pub async fn update_task(
    id: u32,
    name: String,
    content: String,
    start_time: String,
    end_time: String,
    user_id: i64,
) -> Result<String, ServerFnError> {
    DB.with(|conn| {
        conn.execute(
            "UPDATE tasks SET name=?1, content=?2, start_time=?3, end_time=?4, user_id=?5 WHERE id=?6",
            rusqlite::params![name, content, start_time, end_time, user_id, id],
        )
    })
    .map_err(|e| ServerFnError::Response(e.to_string()))?;
    Ok("任务更新成功！".to_string())
}

#[server]
pub async fn delete_task(id: u32) -> Result<String, ServerFnError> {
    DB.with(|conn| {
        conn.execute("DELETE FROM tasks WHERE id=?1", rusqlite::params![id])
    })
    .map_err(|e| ServerFnError::Response(e.to_string()))?;
    Ok("任务已删除".to_string())
}

#[server]
pub async fn update_task_status(id: u32, status: i32) -> Result<String, ServerFnError> {
    DB.with(|conn| {
        conn.execute(
            "UPDATE tasks SET status=?1 WHERE id=?2",
            rusqlite::params![status, id],
        )
    })
    .map_err(|e| ServerFnError::Response(e.to_string()))?;
    Ok("状态已更新".to_string())
}

// ── Pomodoro Server Functions ──────────────────────────────────────

#[server]
pub async fn save_pomodoro_state(
    user_email: String,
    state: PomodoroState,
) -> Result<(), ServerFnError> {
    let mut redis = crate::resp::get_redis().await?;
    let key = format!("pomodoro:state:{}", user_email);
    let json = serde_json::to_string(&state)
        .map_err(|e| ServerFnError::new(format!("Serialize error: {e}")))?;
    redis.setex(&key, 86400, &json).await?;
    debug!("Saved pomodoro state for {user_email}");
    Ok(())
}

#[server]
pub async fn load_pomodoro_state(
    user_email: String,
) -> Result<Option<PomodoroState>, ServerFnError> {
    let mut redis = crate::resp::get_redis().await?;
    let key = format!("pomodoro:state:{}", user_email);
    let val = redis.get(&key).await?;

    match val {
        None => Ok(None),
        Some(json) => {
            let mut state: PomodoroState = serde_json::from_str(&json)
                .map_err(|e| ServerFnError::new(format!("Deserialize error: {e}")))?;

            // If the timer was running, compensate for elapsed time since last sync
            if state.is_running && state.last_sync_epoch > 0 {
                let now = chrono::Utc::now().timestamp();
                let elapsed = (now - state.last_sync_epoch).max(0) as u32;
                if elapsed >= state.remaining_seconds {
                    state.remaining_seconds = 0;
                    state.is_running = false;
                } else {
                    state.remaining_seconds -= elapsed;
                }
            }
            Ok(Some(state))
        }
    }
}

#[server]
pub async fn save_pomodoro_record(
    user_email: String,
    record: PomodoroRecord,
) -> Result<(), ServerFnError> {
    let mut redis = crate::resp::get_redis().await?;

    let history_key = format!("pomodoro:history:{}", user_email);
    let json = serde_json::to_string(&record)
        .map_err(|e| ServerFnError::new(format!("Serialize error: {e}")))?;
    redis.lpush(&history_key, &json).await?;

    // Update stats only for completed work phases
    if record.phase == PomodoroPhase::Working {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

        let total_key = format!("pomodoro:stats:{}:total", user_email);
        redis.incr(&total_key).await?;

        let today_key = format!("pomodoro:stats:{}:today:{}", user_email, today);
        redis.incr(&today_key).await?;
        redis.expire(&today_key, 172800).await?; // 48h TTL

        let minutes_key = format!("pomodoro:stats:{}:minutes:{}", user_email, today);
        redis
            .incrby(&minutes_key, record.duration_minutes as i64)
            .await?;
        redis.expire(&minutes_key, 172800).await?;
    }

    debug!("Saved pomodoro record for {user_email}");
    Ok(())
}

#[server]
pub async fn load_pomodoro_history(
    user_email: String,
) -> Result<Vec<PomodoroRecord>, ServerFnError> {
    let mut redis = crate::resp::get_redis().await?;
    let key = format!("pomodoro:history:{}", user_email);
    let items = redis.lrange(&key, 0, 19).await?;

    let records: Vec<PomodoroRecord> = items
        .iter()
        .filter_map(|json| serde_json::from_str(json).ok())
        .collect();
    Ok(records)
}

#[server]
pub async fn load_pomodoro_stats(user_email: String) -> Result<PomodoroStats, ServerFnError> {
    let mut redis = crate::resp::get_redis().await?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let total_key = format!("pomodoro:stats:{}:total", user_email);
    let today_key = format!("pomodoro:stats:{}:today:{}", user_email, today);
    let minutes_key = format!("pomodoro:stats:{}:minutes:{}", user_email, today);

    let total_count = redis
        .get(&total_key)
        .await?
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    let today_count = redis
        .get(&today_key)
        .await?
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    let today_focus_minutes = redis
        .get(&minutes_key)
        .await?
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    Ok(PomodoroStats {
        today_count,
        total_count,
        today_focus_minutes,
    })
}