use crate::backend::*;
use dioxus::prelude::*;

#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum Status {
    Idle,
    Doing,
    Completed,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Status::Idle => f.write_str("待处理"),
            Status::Doing => f.write_str("进行中"),
            Status::Completed => f.write_str("已完成"),
        }
    }
}

impl Status {
    pub fn to_i32(&self) -> i32 {
        match self {
            Status::Idle => 0,
            Status::Doing => 1,
            Status::Completed => 2,
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Task {
    pub id: Option<u32>,
    pub name: String,
    pub content: String,
    pub status: Status,
    pub start_time: String,
    pub end_time: String,
    pub user_id: i64,
    pub assignee_name: String,
}

// ── TaskManager (root component) ──────────────────────────────────

#[component]
pub fn TaskManager() -> Element {
    let page: Signal<u32> = use_signal(|| 1);
    let page_size: u32 = 9;
    let tasks = use_resource(move || list_tasks(page(), page_size));
    use_context_provider(|| tasks);
    use_context_provider(|| page);

    // Shared dialog signals
    let editing_task: Signal<Option<Task>> = use_signal(|| None);
    let is_form_open: Signal<bool> = use_signal(|| false);
    let delete_target: Signal<Option<u32>> = use_signal(|| None);
    use_context_provider(|| editing_task);
    use_context_provider(|| is_form_open);
    use_context_provider(|| delete_target);

    rsx! {
        TaskFormDialog { is_open: is_form_open, editing_task }
        DeleteConfirmDialog { delete_target }
        TaskToolBar {}
        TaskList { page_size }
    }
}

// ── TaskToolBar ───────────────────────────────────────────────────

#[component]
pub fn TaskToolBar() -> Element {
    let mut is_form_open = use_context::<Signal<bool>>();
    let mut editing_task = use_context::<Signal<Option<Task>>>();
    rsx! {
        div { class: "container",
            div { class: "task-toolbar",
                button {
                    onclick: move |_| {
                        editing_task.set(None);
                        is_form_open.set(true);
                    },
                    "+ 新建任务"
                }
            }
        }
    }
}

// ── TaskFormDialog (unified create / edit) ─────────────────────────

#[component]
pub fn TaskFormDialog(
    mut is_open: Signal<bool>,
    mut editing_task: Signal<Option<Task>>,
) -> Element {
    let mut name = use_signal(|| String::new());
    let mut content = use_signal(|| String::new());
    let mut start_time = use_signal(|| String::new());
    let mut end_time = use_signal(|| String::new());
    let mut user_id: Signal<Option<i64>> = use_signal(|| None);
    let mut message = use_signal(|| String::new());
    let mut tasks = use_context::<Resource<Result<(Vec<Task>, u32), ServerFnError>>>();

    // When editing_task changes, populate or clear form fields
    use_effect(move || {
        if let Some(task) = editing_task() {
            name.set(task.name.clone());
            content.set(task.content.clone());
            start_time.set(task.start_time.clone());
            end_time.set(task.end_time.clone());
            user_id.set(Some(task.user_id));
        } else {
            name.set(String::new());
            content.set(String::new());
            start_time.set(String::new());
            end_time.set(String::new());
            user_id.set(None);
        }
        message.set(String::new());
    });

    let is_edit = editing_task().is_some();
    let title = if is_edit { "编辑任务" } else { "新建任务" };
    let submit_label = if is_edit { "保存修改" } else { "添加任务" };

    rsx! {
        dialog { open: is_open(),
            article {
                header {
                    button {
                        aria_label: "close",
                        onclick: move |_| {
                            is_open.set(false);
                            editing_task.set(None);
                        },
                    }
                    h3 { "{title}" }
                }
                if !message().is_empty() {
                    p { class: "auth-error", "{message}" }
                }
                label {
                    "任务名称"
                    input {
                        r#type: "text",
                        placeholder: "输入任务名称",
                        value: "{name}",
                        oninput: move |e| name.set(e.value()),
                    }
                }
                label {
                    "任务描述"
                    textarea {
                        placeholder: "描述一下需要做什么…",
                        value: "{content}",
                        oninput: move |e| content.set(e.value()),
                    }
                }
                div { class: "dialog-date-grid",
                    label {
                        "开始时间"
                        input {
                            r#type: "datetime-local",
                            value: "{start_time}",
                            oninput: move |e| start_time.set(e.value()),
                        }
                    }
                    label {
                        "结束时间"
                        input {
                            r#type: "datetime-local",
                            value: "{end_time}",
                            oninput: move |e| end_time.set(e.value()),
                        }
                    }
                }
                label {
                    "选择任务执行人"
                    UserSelector { selected_id: user_id }
                }
                footer {
                    button {
                        class: "secondary",
                        onclick: move |_| {
                            is_open.set(false);
                            editing_task.set(None);
                        },
                        "取消"
                    }
                    button {
                        onclick: move |_| async move {
                            let Some(uid) = user_id() else {
                                message.set("请选择任务执行人".to_string());
                                return;
                            };
                            if let Some(task) = editing_task() {
                                // Edit mode
                                let task_id = task.id.unwrap_or(0);
                                match update_task(task_id, name(), content(), start_time(), end_time(), uid).await {
                                    Ok(msg) => {
                                        message.set(msg);
                                        tasks.restart();
                                        is_open.set(false);
                                        editing_task.set(None);
                                    }
                                    Err(e) => {
                                        message.set(format!("更新失败：{e}"));
                                    }
                                }
                            } else {
                                // Create mode
                                let user_belong = use_context::<Signal<Option<String>>>();
                                let uname_belong = match user_belong() {
                                    Some(uname) => uname,
                                    None => "".to_string(),
                                };
                                match save_task(name(), content(), start_time(), end_time(), uid, uname_belong).await {
                                    Ok(msg) => {
                                        message.set(msg);
                                        tasks.restart();
                                        is_open.set(false);
                                        editing_task.set(None);
                                    }
                                    Err(_) => {
                                        message.set("保存任务出错！".to_string());
                                    }
                                }
                            }
                        },
                        "{submit_label}"
                    }
                }
            }
        }
    }
}

// ── DeleteConfirmDialog ───────────────────────────────────────────

#[component]
pub fn DeleteConfirmDialog(mut delete_target: Signal<Option<u32>>) -> Element {
    let mut tasks = use_context::<Resource<Result<(Vec<Task>, u32), ServerFnError>>>();
    let mut message = use_signal(|| String::new());

    rsx! {
        dialog { open: delete_target().is_some(),
            article {
                header {
                    button {
                        aria_label: "close",
                        onclick: move |_| {
                            delete_target.set(None);
                            message.set(String::new());
                        },
                    }
                    h3 { "确认删除" }
                }
                p { "确定要删除此任务吗？此操作不可撤销。" }
                if !message().is_empty() {
                    p { class: "auth-error", "{message}" }
                }
                footer {
                    button {
                        class: "secondary",
                        onclick: move |_| {
                            delete_target.set(None);
                            message.set(String::new());
                        },
                        "取消"
                    }
                    button {
                        class: "btn-danger",
                        onclick: move |_| async move {
                            if let Some(id) = delete_target() {
                                match delete_task(id).await {
                                    Ok(_) => {
                                        tasks.restart();
                                        delete_target.set(None);
                                        message.set(String::new());
                                    }
                                    Err(e) => {
                                        message.set(format!("删除失败：{e}"));
                                    }
                                }
                            }
                        },
                        "删除"
                    }
                }
            }
        }
    }
}

// ── TaskDetailDialog ──────────────────────────────────────────────

/// None = closed, Some(id) = open and display the task
#[component]
pub fn TaskDetailDialog(mut is_detail_open: Signal<Option<u32>>, list: Vec<Task>) -> Element {
    let detail_content = if let Some(id) = is_detail_open() {
        let cur_task = list.iter().find(|t| t.id == Some(id));
        match cur_task {
            Some(cur_task) => rsx! {
                p { "任务 ID：{id}" }
                p { "任务名称: {cur_task.name}" }
                p { "任务内容: {cur_task.content}" }
                p { "任务执行人: {cur_task.assignee_name}" }
                p { "任务状态: {cur_task.status}" }
                p { "开始时间: {cur_task.start_time}" }
                p { "结束时间: {cur_task.end_time}" }
            },
            None => rsx! {
                p { "未找到任务" }
            },
        }
    } else {
        rsx! {}
    };

    rsx! {
        dialog { open: is_detail_open().is_some(),
            article {
                header {
                    button {
                        aria_label: "close",
                        onclick: move |_| is_detail_open.set(None),
                    }
                    h3 { "任务详情" }
                }
                {detail_content}
                footer {
                    button { onclick: move |_| is_detail_open.set(None), "关闭" }
                }
            }
        }
    }
}

// ── TaskList (card-based layout) ──────────────────────────────────

#[component]
pub fn TaskList(page_size: u32) -> Element {
    let mut tasks = use_context::<Resource<Result<(Vec<Task>, u32), ServerFnError>>>();
    let mut page = use_context::<Signal<u32>>();
    let mut is_detail_open: Signal<Option<u32>> = use_signal(|| None);
    let mut is_form_open = use_context::<Signal<bool>>();
    let mut editing_task = use_context::<Signal<Option<Task>>>();
    let mut delete_target = use_context::<Signal<Option<u32>>>();

    match tasks() {
        None => rsx! {
            main { class: "container",
                p { aria_busy: "true", "正在加载…" }
            }
        },
        Some(Err(e)) => rsx! {
            main { class: "container",
                p { class: "auth-error", "加载失败：{e}" }
            }
        },
        Some(Ok((list, total))) => {
            let total_pages = total.div_ceil(page_size).max(1);
            let cur = page();
            rsx! {
                TaskDetailDialog {
                    is_detail_open: is_detail_open.clone(),
                    list: list.clone(),
                }
                main { class: "container",
                    div { class: "task-header",
                        h2 { "任务列表" }
                        span {
                            class: "badge badge-idle",
                            style: "cursor:default",
                            "共 {total} 项"
                        }
                    }
                    if list.is_empty() {
                        p { class: "task-empty", "暂无任务" }
                    } else {
                        div { class: "task-card-grid",
                            for task in list.iter() {
                                {
                                    let task_id = task.id.unwrap_or(0);
                                    let task_clone = task.clone();
                                    let assignee = task.assignee_name.clone();
                                    let cur_status_i32 = task.status.to_i32();
                                    rsx! {
                                        div { class: "task-card",
                                            div { class: "task-card-header",
                                                span { class: "task-card-name", "{task.name}" }
                                                select {
                                                    class: "status-select",
                                                    value: "{cur_status_i32}",
                                                    onchange: move |evt| async move {
                                                        if let Ok(new_val) = evt.value().parse::<i32>() {
                                                            if new_val != cur_status_i32 {
                                                                if let Ok(_) = update_task_status(task_id, new_val).await {
                                                                    tasks.restart();
                                                                }
                                                            }
                                                        }
                                                    },
                                                    option { value: "0", selected: cur_status_i32 == 0, "待处理" }
                                                    option { value: "1", selected: cur_status_i32 == 1, "进行中" }
                                                    option { value: "2", selected: cur_status_i32 == 2, "已完成" }
                                                }
                                            }
                                            if !task.content.is_empty() {
                                                div { class: "task-card-content", "{task.content}" }
                                            }
                                            div { class: "task-card-meta",
                                                span { "执行人: {assignee}" }
                                                if !task.start_time.is_empty() {
                                                    span { "开始: {task.start_time}" }
                                                }
                                                if !task.end_time.is_empty() {
                                                    span { "结束: {task.end_time}" }
                                                }
                                            }
                                            div { class: "task-card-footer",
                                                button {
                                                    class: "outline secondary",
                                                    onclick: move |_| {
                                                        is_detail_open.set(Some(task_id));
                                                    },
                                                    "详情"
                                                }
                                                button {
                                                    class: "outline secondary",
                                                    onclick: move |_| {
                                                        editing_task.set(Some(task_clone.clone()));
                                                        is_form_open.set(true);
                                                    },
                                                    "编辑"
                                                }
                                                button {
                                                    class: "outline btn-danger",
                                                    onclick: move |_| {
                                                        delete_target.set(Some(task_id));
                                                    },
                                                    "删除"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Pagination
                    div { class: "task-pagination",
                        button {
                            class: "outline secondary",
                            disabled: cur <= 1,
                            onclick: move |_| page.set(cur - 1),
                            "‹ 上一页"
                        }
                        for p in 1..=total_pages {
                            {
                                let is_cur = p == cur;
                                rsx! {
                                    button {
                                        class: if is_cur { "" } else { "outline secondary" },
                                        onclick: move |_| page.set(p),
                                        "{p}"
                                    }
                                }
                            }
                        }
                        button {
                            class: "outline secondary",
                            disabled: cur >= total_pages,
                            onclick: move |_| page.set(cur + 1),
                            "下一页 ›"
                        }
                    }
                }
            }
        }
    }
}

// ── UserSelector ──────────────────────────────────────────────────

#[component]
pub fn UserSelector(selected_id: Signal<Option<i64>>) -> Element {
    let mut input_name = use_signal(|| String::new());
    let mut user_list = use_resource(move || list_users(input_name()));

    rsx! {
        div {
            input {
                r#type: "text",
                placeholder: "搜索用户…",
                value: "{input_name}",
                oninput: move |e| {
                    input_name.set(e.value());
                    user_list.restart();
                },
            }
            select {
                name: "task_assignee",
                aria_label: "选择任务执行人...",
                required: true,
                onchange: move |e| {
                    selected_id.set(e.value().parse::<i64>().ok());
                },
                match user_list() {
                    None => rsx! {
                        option { disabled: true, "正在加载…" }
                    },
                    Some(Err(e)) => rsx! {
                        option { disabled: true, "加载失败：{e}" }
                    },
                    Some(Ok(users)) => rsx! {
                        for (id , username) in users {
                            option { value: "{id}", "{username}" }
                        }
                    },
                }
            }
        }
    }
}
