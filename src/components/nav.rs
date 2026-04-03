use crate::Route;
use dioxus::prelude::*;

#[component]
pub fn NavBar() -> Element {
    rsx! {
        nav { class: "container-fluid",
            ul {
                li {
                    strong { "✓ 任务管理" }
                }
            }
            ul {
                li {
                    Link { to: Route::Home {}, "首页" }
                }
                li {
                    Link { to: Route::TaskManager, "任务" }
                }
                li {
                    Link { to: Route::Logout, "退出登录" }
                }
            }
        }
        Outlet::<Route> {}
    }
}
