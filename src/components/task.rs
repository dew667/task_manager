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
    fn badge_class(&self) -> &'static str {
        match self {
            Status::Idle => "badge badge-idle",
            Status::Doing => "badge badge-doing",
            Status::Completed => "badge badge-completed",
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

#[component]
pub fn TaskManager() -> Element {
    let mut page: Signal<u32> = use_signal(|| 1);
    let page_size: u32 = 6;
    let tasks = use_resource(move || list_tasks(page(), page_size));
    use_context_provider(|| tasks);
    use_context_provider(|| page);
    rsx! {
        TaskToolBar {}
        TaskList { page_size }
    }
}

#[component]
pub fn TaskToolBar() -> Element {
    let mut is_open = use_signal(|| false);
    rsx! {
        div { class: "container",
            div { style: "display:flex; justify-content:flex-end; padding: 1rem 0 0.5rem;",
                button { onclick: move |_| is_open.set(!is_open()), "+ 新建任务" }
            }
        }
        TaskAddDialog { is_open }
    }
}

#[component]
pub fn TaskAddDialog(mut is_open: Signal<bool>) -> Element {
    let mut name = use_signal(|| String::new());
    let mut content = use_signal(|| String::new());
    let mut start_time = use_signal(|| String::new());
    let mut end_time = use_signal(|| String::new());
    let user_id: Signal<Option<i64>> = use_signal(|| None);
    let mut message = use_signal(|| String::new());
    let mut tasks = use_context::<Resource<Result<(Vec<Task>, u32), ServerFnError>>>();
    rsx! {
        dialog { open: is_open(),
            article {
                header {
                    button {
                        aria_label: "close",
                        onclick: move |_| is_open.set(false),
                    }
                    h3 { "新建任务" }
                }
                p { "{message}" }
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
                div { style: "display:grid; grid-template-columns:1fr 1fr; gap:1rem;",
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
                        onclick: move |_| is_open.set(false),
                        "取消"
                    }
                    button {
                        onclick: move |_| async move {
                            let Some(uid) = user_id() else {
                                message.set("请选择任务执行人".to_string());
                                return;
                            };
                            let user_belong = use_context::<Signal<Option<String>>>();
                            let uname_belong = match user_belong() {
                                Some(uname) => uname,
                                None => "".to_string(),
                            };
                            match save_task(name(), content(), start_time(), end_time(), uid, uname_belong)
                                .await
                            {
                                Ok(msg) => {
                                    message.set(msg);
                                    tasks.restart();
                                    is_open.set(false);
                                }
                                Err(_) => {
                                    message.set("保存任务出错！".to_string());
                                }
                            }
                        },
                        "添加任务"
                    }
                }
            }
        }
    }
}

/// None = 关闭，Some(id) = 打开并显示对应任务
#[component]
pub fn TaskDetailDialog(mut is_detail_open: Signal<Option<u32>>, list: Vec<Task>) -> Element {
    let content = if let Some(id) = is_detail_open() {
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
                {content}
                footer {
                    button { onclick: move |_| is_detail_open.set(None), "关闭" }
                }
            }
        }
    }
}

#[component]
pub fn TaskList(page_size: u32) -> Element {
    let tasks = use_context::<Resource<Result<(Vec<Task>, u32), ServerFnError>>>();
    let mut page = use_context::<Signal<u32>>();
    let mut is_detail_open: Signal<Option<u32>> = use_signal(|| None);

    match tasks() {
        None => rsx! {
            main { class: "container",
                p { aria_busy: "true", "正在加载…" }
            }
        },
        Some(Err(e)) => rsx! {
            main { class: "container",
                p { style: "color:var(--pico-del-color)", "加载失败：{e}" }
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
                    div { style: "display:flex; align-items:center; justify-content:space-between; margin-bottom:0.75rem;",
                        h2 { style: "margin:0", "任务列表" }
                        span {
                            class: "badge badge-idle",
                            style: "cursor:default",
                            "共 {total} 项"
                        }
                    }
                    figure { style: "margin:0; overflow:hidden; border-radius:var(--pico-border-radius); border:1px solid var(--pico-table-border-color);",
                        div { class: "task-grid task-grid-head",
                            span { "#" }
                            span { "名称" }
                            span { "描述" }
                            span { "执行人" }
                            span { "状态" }
                            span { style: "text-align:right", "操作" }
                        }
                        if list.is_empty() {
                            p { style: "text-align:center; padding:3rem; color:var(--pico-muted-color); margin:0;",
                                "暂无任务"
                            }
                        }
                        for (index , task) in list.iter().enumerate() {
                            {
                                let task_id = task.id.unwrap_or(index as u32);
                                let assignee = task.assignee_name.clone();
                                rsx! {
                                    div { class: "task-grid task-grid-row",
                                        span { class: "task-num", "{(cur - 1) * page_size + index as u32 + 1}" }
                                        span { class: "task-name", "{task.name}" }
                                        span { class: "task-desc", "{task.content}" }
                                        span { class: "task-execute", "{assignee}" }
                                        span { class: "{task.status.badge_class()}", "{task.status}" }
                                        div { class: "task-actions",
                                            button {
                                                class: "outline secondary",
                                                onclick: move |_| is_detail_open.set(Some(task_id)),
                                                "详情"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // 分页按钮
                    div { style: "display:flex; justify-content:center; align-items:center; gap:0.5rem; margin-top:1rem;",
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
                                    button { class: if is_cur { "" } else { "outline secondary" }, onclick: move |_| page.set(p), "{p}" }
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
