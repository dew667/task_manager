use crate::backend::*;
use crate::Route;
use dioxus::prelude::*;

#[component]
pub fn LoginTip(is_opened: Signal<Option<String>>) -> Element {
    if is_opened().is_some() {
        let message = is_opened.unwrap();
        rsx! {
            p { color: "red", "{message}" }
        }
    } else {
        rsx! {}
    }
}

#[component]
pub fn Login() -> Element {
    let mut username = use_signal(|| String::new());
    let mut password = use_signal(|| String::new());
    let mut is_opend: Signal<Option<String>> = use_signal(|| None);
    let nav = navigator();
    let mut user = use_context::<Signal<Option<String>>>();
    rsx! {
        main { class: "container", width: "500px", margin_top: "200px",
            div {
                p { "登录" }
                LoginTip { is_opened: is_opend }
                input {
                    name: "email",
                    r#type: "email",
                    placeholder: "邮箱",
                    autocomplete: "email",
                    oninput: move |e| username.set(e.value()),
                    {}
                }
                input {
                    name: "password",
                    r#type: "password",
                    placeholder: "密码",
                    autocomplete: "current-password",
                    oninput: move |e| password.set(e.value()),
                    {}
                }
                button {
                    width: "500px",
                    margin_bottom: "20px",
                    onclick: move |_| async move {
                        let username = username();
                        let password = password();
                        match do_login(username.clone(), password.clone()).await {
                            Ok(message) => {
                                is_opend.set(Some(message.to_string()));
                                *user.write() = Some(username.clone());
                                #[cfg(feature = "web")]
                                crate::local_storage_set("user", &username);
                                nav.push(Route::Home {});
                            }
                            Err(e) => {
                                is_opend.set(Some(e.to_string()));
                                *user.write() = None;
                                nav.replace(Route::Login {});
                            }
                        }
                    },
                    "登录"
                    {}
                }
                button {
                    width: "500px",
                    onclick: move |_| async move {
                        nav.push(Route::Register);
                    },
                    "注册账号"
                    {}
                }
            
            }
        }

    }
}
