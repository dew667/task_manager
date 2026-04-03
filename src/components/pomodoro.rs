use crate::backend::*;
use dioxus::prelude::*;

// ── Constants ──────────────────────────────────────────────────────

const WORK_DURATION: u32 = 25 * 60;
const SHORT_BREAK_DURATION: u32 = 5 * 60;
const LONG_BREAK_DURATION: u32 = 15 * 60;
const POMODOROS_BEFORE_LONG_BREAK: u32 = 4;

/// Cross-platform 1-second sleep
async fn sleep_one_second() {
    #[cfg(feature = "web")]
    gloo_timers::future::sleep(std::time::Duration::from_millis(1000)).await;
    #[cfg(not(feature = "web"))]
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
}

// ── Shared Data Models ─────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PomodoroPhase {
    Idle,
    Working,
    ShortBreak,
    LongBreak,
}

impl PomodoroPhase {
    fn label(&self) -> &'static str {
        match self {
            PomodoroPhase::Idle => "就绪",
            PomodoroPhase::Working => "专注中",
            PomodoroPhase::ShortBreak => "短休息",
            PomodoroPhase::LongBreak => "长休息",
        }
    }

    fn card_class(&self) -> &'static str {
        match self {
            PomodoroPhase::Idle => "timer-card",
            PomodoroPhase::Working => "timer-card pomodoro-working",
            PomodoroPhase::ShortBreak => "timer-card pomodoro-short-break",
            PomodoroPhase::LongBreak => "timer-card pomodoro-long-break",
        }
    }

    fn history_class(&self) -> &'static str {
        match self {
            PomodoroPhase::Working => "history-state working",
            PomodoroPhase::ShortBreak => "history-state shortbreak",
            PomodoroPhase::LongBreak => "history-state longbreak",
            PomodoroPhase::Idle => "history-state",
        }
    }

    fn history_label(&self) -> &'static str {
        match self {
            PomodoroPhase::Working => "专注",
            PomodoroPhase::ShortBreak => "短休",
            PomodoroPhase::LongBreak => "长休",
            PomodoroPhase::Idle => "",
        }
    }
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PomodoroState {
    pub phase: PomodoroPhase,
    pub remaining_seconds: u32,
    pub total_seconds: u32,
    pub completed_count: u32,
    pub is_running: bool,
    pub associated_task: Option<String>,
    pub last_sync_epoch: i64,
}

impl Default for PomodoroState {
    fn default() -> Self {
        Self {
            phase: PomodoroPhase::Idle,
            remaining_seconds: 0,
            total_seconds: WORK_DURATION,
            completed_count: 0,
            is_running: false,
            associated_task: None,
            last_sync_epoch: 0,
        }
    }
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PomodoroRecord {
    pub timestamp: String,
    pub phase: PomodoroPhase,
    pub duration_minutes: u32,
    pub task_name: String,
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
pub struct PomodoroStats {
    pub today_count: u32,
    pub total_count: u32,
    pub today_focus_minutes: u32,
}

// ── Helper ─────────────────────────────────────────────────────────

fn format_time(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string()
}

// ── Root Component ─────────────────────────────────────────────────

#[component]
pub fn PomodoroTimer() -> Element {
    let user = use_context::<Signal<Option<String>>>();
    let email = user().unwrap_or_default();

    let mut state = use_signal(PomodoroState::default);
    let mut stats = use_signal(PomodoroStats::default);
    let mut history: Signal<Vec<PomodoroRecord>> = use_signal(Vec::new);
    let mut show_history = use_signal(|| false);
    let selected_task = use_signal(|| String::new());
    let mut initialized = use_signal(|| false);
    let mut tick_generation = use_signal(|| 0u32);

    // Load initial state from Redis after hydration
    let email_init = email.clone();
    use_effect(move || {
        let email = email_init.clone();
        if email.is_empty() {
            return;
        }
        spawn(async move {
            if let Ok(Some(s)) = load_pomodoro_state(email.clone()).await {
                state.set(s);
            }
            if let Ok(s) = load_pomodoro_stats(email.clone()).await {
                stats.set(s);
            }
            if let Ok(h) = load_pomodoro_history(email.clone()).await {
                history.set(h);
            }
            initialized.set(true);
            // If timer was running, kick off the tick loop
            if state().is_running {
                tick_generation.set(tick_generation() + 1);
            }
        });
    });

    // Timer tick engine — spawns a new loop whenever tick_generation changes
    let email_tick = email.clone();
    use_effect(move || {
        let gen = tick_generation();
        let email = email_tick.clone();
        if email.is_empty() {
            return;
        }

        spawn(async move {
            let mut sync_counter: u32 = 0;
            loop {
                sleep_one_second().await;

                // If a newer loop was spawned, exit this stale one
                if tick_generation() != gen {
                    break;
                }

                let mut s = state();
                if !s.is_running {
                    break;
                }

                if s.remaining_seconds == 0 {
                    // Phase completed
                    let completed_phase = s.phase;
                    let task_name = s
                        .associated_task
                        .clone()
                        .unwrap_or_else(|| "无任务".to_string());
                    let duration_minutes = s.total_seconds / 60;

                    let record = PomodoroRecord {
                        timestamp: now_iso(),
                        phase: completed_phase,
                        duration_minutes,
                        task_name,
                    };

                    // Save record to Redis
                    let _ = save_pomodoro_record(email.clone(), record.clone()).await;

                    // Increment completed_count if work phase
                    if completed_phase == PomodoroPhase::Working {
                        s.completed_count += 1;
                    }

                    // Transition to next phase
                    match completed_phase {
                        PomodoroPhase::Working => {
                            if s.completed_count % POMODOROS_BEFORE_LONG_BREAK == 0 {
                                s.phase = PomodoroPhase::LongBreak;
                                s.remaining_seconds = LONG_BREAK_DURATION;
                                s.total_seconds = LONG_BREAK_DURATION;
                            } else {
                                s.phase = PomodoroPhase::ShortBreak;
                                s.remaining_seconds = SHORT_BREAK_DURATION;
                                s.total_seconds = SHORT_BREAK_DURATION;
                            }
                        }
                        PomodoroPhase::ShortBreak | PomodoroPhase::LongBreak => {
                            s.phase = PomodoroPhase::Working;
                            s.remaining_seconds = WORK_DURATION;
                            s.total_seconds = WORK_DURATION;
                        }
                        PomodoroPhase::Idle => {}
                    }

                    s.is_running = true;
                    s.last_sync_epoch = now_epoch();
                    state.set(s.clone());

                    // Sync immediately
                    let _ = save_pomodoro_state(email.clone(), s).await;

                    // Refresh stats and history
                    if let Ok(new_stats) = load_pomodoro_stats(email.clone()).await {
                        stats.set(new_stats);
                    }
                    if let Ok(new_history) = load_pomodoro_history(email.clone()).await {
                        history.set(new_history);
                    }

                    sync_counter = 0;
                    continue;
                }

                s.remaining_seconds -= 1;
                s.last_sync_epoch = now_epoch();
                state.set(s.clone());

                // Sync to Redis every 30 seconds
                sync_counter += 1;
                if sync_counter >= 30 {
                    let _ = save_pomodoro_state(email.clone(), s).await;
                    sync_counter = 0;
                }
            }
        });
    });

    // ── Action handlers ────────────────────────────────────────────

    let email_start = email.clone();
    let on_start = move |_| {
        let email = email_start.clone();
        let task = selected_task();
        let associated = if task.is_empty() { None } else { Some(task) };
        let s = PomodoroState {
            phase: PomodoroPhase::Working,
            remaining_seconds: WORK_DURATION,
            total_seconds: WORK_DURATION,
            completed_count: state().completed_count,
            is_running: true,
            associated_task: associated,
            last_sync_epoch: now_epoch(),
        };
        state.set(s.clone());
        tick_generation.set(tick_generation() + 1);
        spawn(async move {
            let _ = save_pomodoro_state(email, s).await;
        });
    };

    let email_pause = email.clone();
    let on_pause = move |_| {
        let email = email_pause.clone();
        let mut s = state();
        s.is_running = false;
        s.last_sync_epoch = now_epoch();
        state.set(s.clone());
        spawn(async move {
            let _ = save_pomodoro_state(email, s).await;
        });
    };

    let email_resume = email.clone();
    let on_resume = move |_| {
        let email = email_resume.clone();
        let mut s = state();
        s.is_running = true;
        s.last_sync_epoch = now_epoch();
        state.set(s.clone());
        tick_generation.set(tick_generation() + 1);
        spawn(async move {
            let _ = save_pomodoro_state(email, s).await;
        });
    };

    let email_reset = email.clone();
    let on_reset = move |_| {
        let email = email_reset.clone();
        let s = PomodoroState {
            completed_count: state().completed_count,
            ..PomodoroState::default()
        };
        state.set(s.clone());
        spawn(async move {
            let _ = save_pomodoro_state(email, s).await;
        });
    };

    // ── Render ─────────────────────────────────────────────────────

    let s = state();
    let st = stats();
    let is_idle = s.phase == PomodoroPhase::Idle;
    let is_running = s.is_running;

    let progress_pct = if s.total_seconds > 0 {
        ((s.total_seconds - s.remaining_seconds) as f64 / s.total_seconds as f64) * 100.0
    } else {
        0.0
    };

    rsx! {
        main { class: "container",
            div { class: "pomodoro-container",
                header {
                    h2 { "番茄钟" }
                    p { class: "subtitle", "保持专注，高效工作" }
                }

                // Stats bar
                div { class: "stats-bar",
                    div { class: "stat-item",
                        span { class: "stat-value", "{st.today_count}" }
                        span { class: "stat-label", "今日番茄" }
                    }
                    div { class: "stat-item",
                        span { class: "stat-value", "{st.total_count}" }
                        span { class: "stat-label", "累计番茄" }
                    }
                    div { class: "stat-item",
                        span { class: "stat-value", "{st.today_focus_minutes} 分钟" }
                        span { class: "stat-label", "今日专注" }
                    }
                }

                // Timer card
                div { class: "{s.phase.card_class()}",
                    div { class: "state-label", "{s.phase.label()}" }
                    div { class: "timer-display",
                        if is_idle {
                            "{format_time(WORK_DURATION)}"
                        } else {
                            "{format_time(s.remaining_seconds)}"
                        }
                    }
                    if !is_idle {
                        div { class: "progress-bar",
                            div {
                                class: "progress-fill",
                                style: "width: {progress_pct:.1}%",
                            }
                        }
                    }
                }

                // Task selector
                div { class: "task-input",
                    TaskOptionLoader { selected_task, disabled: !is_idle }
                }

                // Controls
                div { class: "controls",
                    if is_idle {
                        button {
                            onclick: on_start,
                            "开始专注"
                        }
                    } else if is_running {
                        button {
                            class: "secondary",
                            onclick: on_pause,
                            "暂停"
                        }
                        button {
                            class: "outline secondary",
                            onclick: on_reset,
                            "重置"
                        }
                    } else {
                        button {
                            onclick: on_resume,
                            "继续"
                        }
                        button {
                            class: "outline secondary",
                            onclick: on_reset,
                            "重置"
                        }
                    }
                }

                // History toggle
                div { class: "history-toggle",
                    button {
                        class: "outline",
                        onclick: move |_| show_history.set(!show_history()),
                        if show_history() { "隐藏历史记录" } else { "查看历史记录" }
                    }
                }

                // History section
                if show_history() {
                    HistorySection { history: history() }
                }
            }
        }
    }
}

// ── Task option loader (fills select with task names) ──────────────

#[component]
fn TaskOptionLoader(mut selected_task: Signal<String>, disabled: bool) -> Element {
    let tasks = use_resource(|| list_tasks(1, 100));

    match tasks() {
        Some(Ok((task_list, _))) => {
            rsx! {
                select {
                    disabled,
                    onchange: move |e| selected_task.set(e.value()),
                    option { value: "", "独立番茄（不关联任务）" }
                    for task in task_list {
                        option {
                            value: "{task.name}",
                            "{task.name}"
                        }
                    }
                }
            }
        }
        _ => rsx! {},
    }
}

// ── History Section ────────────────────────────────────────────────

#[component]
fn HistorySection(history: Vec<PomodoroRecord>) -> Element {
    rsx! {
        div { class: "history-section",
            h3 { "历史记录" }
            if history.is_empty() {
                p { class: "empty-message", "暂无记录" }
            } else {
                div { class: "history-list",
                    for record in history {
                        div { class: "history-item",
                            div { class: "history-info",
                                span { class: "history-task", "{record.task_name}" }
                                span { class: "history-date", "{record.timestamp}" }
                            }
                            div { class: "history-meta",
                                span {
                                    class: "{record.phase.history_class()}",
                                    "{record.phase.history_label()}"
                                }
                                span { class: "history-duration", "{record.duration_minutes} 分钟" }
                            }
                        }
                    }
                }
            }
        }
    }
}
