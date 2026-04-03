use crate::backend::do_register;
use crate::Route;
use dioxus::prelude::*;

#[component]
pub fn RegisterTip(is_opened: Signal<Option<String>>) -> Element {
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
pub fn Register() -> Element {
    let mut username = use_signal(|| String::new());
    let mut password = use_signal(|| String::new());
    let mut is_opend: Signal<Option<String>> = use_signal(|| None);
    let nav = navigator();

    rsx! {
        main { class: "container", width: "500px", margin_top: "200px",
            div {
                p { "注册" }
                RegisterTip { is_opened: is_opend }
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
                        match do_register(username.clone(), password.clone()).await {
                            Ok(message) => {
                                is_opend.set(Some(message.to_string()));
                                nav.push(Route::Login {});
                            }
                            Err(e) => {
                                is_opend.set(Some(e.to_string()));
                                nav.replace(Route::Register {});
                            }
                        }
                    },
                    "注册"
                    {}
                }
            }
        }

    }
}
