use crate::components::{Login, NavBar, TaskManager, Register};
use dioxus::logger::tracing::{debug, Level};
use dioxus::prelude::*;

#[cfg(feature = "server")]
use tracing_subscriber;

mod backend;
mod components;

// localStorage helpers — web only
#[cfg(feature = "web")]
fn local_storage_get(key: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
        .and_then(|s| s.get_item(key).ok())
        .flatten()
}

#[cfg(feature = "web")]
fn local_storage_set(key: &str, value: &str) {
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
    {
        let _ = storage.set_item(key, value);
    }
}

#[cfg(feature = "web")]
fn local_storage_remove(key: &str) {
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
    {
        let _ = storage.remove_item(key);
    }
}

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[route("/login")]
    Login,
    #[route("/register")]
    Register,
    #[route("/logout")]
    Logout,
    #[layout(AuthLayout)]
    #[layout(NavBar)]
        #[route("/task")]
        TaskManager,
        #[route("/")]
        Home { },
}

fn main() {
    // 服务端：初始化 tracing subscriber 使 #[server] 函数的 debug! 可见
    #[cfg(feature = "server")]
    {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
        
    }
    // 客户端：初始化 dioxus logger
    #[cfg(not(feature = "server"))]
    {
        dioxus::logger::init(Level::DEBUG).expect("Failed to initialize logger");
    }
    debug!("Logger initialized at DEBUG level");
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut user = use_context_provider::<Signal<Option<String>>>(|| Signal::new(None));

    // hydration 完成后从 localStorage 恢复登录状态
    use_effect(move || {
        #[cfg(feature = "web")]
        if let Some(email) = local_storage_get("user") {
            *user.write() = Some(email);
        }
    });

    rsx! {
        document::Stylesheet { href: asset!("/assets/pico.min.css") }
        document::Stylesheet { href: asset!("/assets/main.css") }
        Router::<Route> {}
    }
}

#[component]
fn Home() -> Element {
    let user = use_context::<Signal<Option<String>>>();
    let uname = match user() {
        Some(uname) => uname,
        None => "游客".to_string(),
    };
    rsx! {
        main { class: "container",
            div { style: "text-align:center; padding: 5rem 1rem;",
                h1 { "任务管理" }
                p { "欢迎 {uname}" }
                p { "保持专注，高效完成每一件事。" }
                Link { to: Route::TaskManager, role: "button", "查看任务 →" }
            }
        }
    }
}

#[component]
fn AuthLayout() -> Element {
    let user = use_context::<Signal<Option<String>>>();
    let nav = navigator();

    // use_effect 在 hydration 后才运行，初次渲染时 user 可能还是 None
    // 用一个 initialized 标记避免在 localStorage 读取前就跳转
    let mut initialized = use_signal(|| false);
    use_effect(move || {
        initialized.set(true);
    });

    if initialized() && user().is_none() {
        nav.replace(Route::Login {});
        return rsx! {};
    }

    rsx! {
        Outlet::<Route> {}

    }
}


#[component]
fn Logout() -> Element {
    let mut user = use_context::<Signal<Option<String>>>();
    let nav = navigator();
    
    #[cfg(feature = "web")]
    local_storage_remove("user");
    
    *user.write() = None;

    nav.replace(Route::Login {});

    rsx!{}
}