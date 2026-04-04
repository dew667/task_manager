use crate::Route;
use dioxus::prelude::*;

#[component]
pub fn NavBar() -> Element {
    let mut menu_open = use_signal(|| false);
    rsx! {
        nav { class: "container-fluid nav-bar",
            ul { class: "nav-brand",
                li {
                    strong { "✓ 任务管理" }
                }
            }
            button {
                class: "nav-toggle",
                onclick: move |_| menu_open.set(!menu_open()),
                "≡"
            }
            ul { class: if menu_open() { "nav-links open" } else { "nav-links" },
                li {
                    Link {
                        to: Route::Home {},
                        onclick: move |_| menu_open.set(false),
                        "首页"
                    }
                }
                li {
                    Link {
                        to: Route::TaskManager,
                        onclick: move |_| menu_open.set(false),
                        "任务"
                    }
                }
                li {
                    Link {
                        to: Route::PomodoroTimer,
                        onclick: move |_| menu_open.set(false),
                        "番茄钟"
                    }
                }
                li {
                    Link {
                        to: Route::Logout,
                        onclick: move |_| menu_open.set(false),
                        "退出登录"
                    }
                }
            }
        }
        Outlet::<Route> {}
    }
}
