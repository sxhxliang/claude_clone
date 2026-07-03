//! Popover menu content builders (chat-title menu, user menu, add menu, model
//! menu) and the shared `menu_item` row. These are free functions rendered into
//! `PopoverState` content and call back into `ClaudeApp` via a `WeakEntity`.
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    Icon, IconName, WindowExt as _, h_flex, notification::Notification, popover::PopoverState,
    scroll::ScrollableElement as _, switch::Switch, v_flex,
};
use std::path::PathBuf;

use crate::ClaudeApp;
use crate::conversation_panel::ConversationPanel;
use crate::mcp_backend::McpServerInfo;
use crate::theme::{border_color, hover_surface, text_2, text_3, text_color};

pub(crate) fn chat_title_menu_content(
    cx: &mut Context<PopoverState>,
    weak: WeakEntity<ClaudeApp>,
    pinned: bool,
) -> Stateful<Div> {
    let weak_pin = weak.clone();
    let weak_rename = weak.clone();
    let weak_project = weak.clone();
    let weak_delete = weak.clone();
    v_flex()
        .id("chat-title-menu-content")
        .w(px(200.))
        .py_1()
        .child(menu_item(
            "ct-pin",
            if pinned {
                IconName::StarFill
            } else {
                IconName::Star
            },
            if pinned {
                crate::tr!("menu.unpin")
            } else {
                crate::tr!("menu.pin")
            },
            move |window, cx| {
                if let Some(v) = weak_pin.upgrade() {
                    v.update(cx, |this, cx| {
                        this.chat_pinned = !this.chat_pinned;
                        this.sync_active_conversation();
                        this.save_state(cx);
                        window.push_notification(
                            Notification::info(if this.chat_pinned {
                                crate::tr!("menu.pinned")
                            } else {
                                crate::tr!("menu.unpinned")
                            }),
                            cx,
                        );
                        cx.notify();
                    });
                }
            },
            cx,
        ))
        .child(menu_item(
            "ct-rename",
            IconName::Replace,
            crate::tr!("menu.rename"),
            move |window, cx| {
                if let Some(v) = weak_rename.upgrade() {
                    v.update(cx, |this, cx| this.begin_rename(window, cx));
                }
            },
            cx,
        ))
        .child(menu_item(
            "ct-project",
            IconName::Folder,
            crate::tr!("project.add_to_project"),
            move |window, cx| {
                if let Some(v) = weak_project.upgrade() {
                    v.update(cx, |this, cx| this.open_project_picker(window, cx));
                }
            },
            cx,
        ))
        .child(div().h(px(1.)).bg(border_color()).my_1())
        .child(menu_item(
            "ct-delete",
            IconName::Delete,
            crate::tr!("common.delete"),
            move |window, cx| {
                if let Some(v) = weak_delete.upgrade() {
                    v.update(cx, |this, cx| this.delete_active_conversation(window, cx));
                }
            },
            cx,
        ))
        .text_color(text_color())
}

pub(crate) fn user_menu_content(
    cx: &mut Context<PopoverState>,
    weak: WeakEntity<ClaudeApp>,
) -> Stateful<Div> {
    let weak_settings = weak.clone();
    let weak_update = weak.clone();
    let weak_language = weak.clone();
    let update_label = weak
        .upgrade()
        .map(|app| app.read(cx).update_menu_label(cx))
        .unwrap_or_else(|| crate::tr!("updates.check"));
    let current_locale = weak
        .upgrade()
        .map(|app| app.read(cx).settings.locale.to_string())
        .unwrap_or_else(|| crate::i18n::current_locale().to_string());
    let next_locale = if crate::i18n::normalize_locale(&current_locale) == crate::i18n::ZH_CN_LOCALE
    {
        crate::i18n::EN_LOCALE
    } else {
        crate::i18n::ZH_CN_LOCALE
    };
    let language_label = crate::tr!(
        "menu.switch_to_language",
        language = crate::i18n::language_name(next_locale).to_string()
    );
    v_flex()
        .id("user-menu-content")
        .w(px(240.))
        .py_1()
        .child(
            div()
                .px_3p5()
                .py_2p5()
                .text_size(px(12.5))
                .text_color(text_3())
                .border_b_1()
                .border_color(border_color())
                .child("jackhenry@example.com"),
        )
        .child(menu_item(
            "um-settings",
            IconName::Settings,
            crate::tr!("menu.settings"),
            move |window, cx| {
                if let Some(v) = weak_settings.upgrade() {
                    v.update(cx, |this, cx| this.open_settings(window, cx));
                }
            },
            cx,
        ))
        .child(menu_item(
            "um-update",
            IconName::Redo2,
            update_label,
            move |window, cx| {
                if let Some(v) = weak_update.upgrade() {
                    v.update(cx, |this, cx| this.handle_update_action(window, cx));
                }
            },
            cx,
        ))
        .child(menu_item(
            "um-lang",
            IconName::Globe,
            language_label,
            move |window, cx| {
                if let Some(v) = weak_language.upgrade() {
                    v.update(cx, |this, cx| {
                        this.set_locale(next_locale, cx);
                    });
                    window.push_notification(
                        Notification::info(crate::tr!(
                            "menu.language_changed",
                            language = crate::i18n::language_name(next_locale).to_string()
                        )),
                        cx,
                    );
                }
            },
            cx,
        ))
        .child(menu_item(
            "um-help",
            IconName::Info,
            crate::tr!("menu.get_help"),
            |window, cx| {
                window.push_notification(Notification::info(crate::tr!("menu.help_opened")), cx);
            },
            cx,
        ))
        .child(div().h(px(1.)).bg(border_color()).my_1())
        .child(menu_item(
            "um-upgrade",
            IconName::Star,
            crate::tr!("menu.upgrade_plan"),
            |window, cx| {
                window.push_notification(
                    Notification::info(crate::tr!("menu.upgrade_plan_hint")),
                    cx,
                );
            },
            cx,
        ))
        .child(menu_item(
            "um-apps",
            IconName::ArrowDown,
            crate::tr!("menu.apps"),
            |window, cx| {
                window.push_notification(
                    Notification::info(crate::tr!("menu.opening_app_store")),
                    cx,
                );
            },
            cx,
        ))
        .child(div().h(px(1.)).bg(border_color()).my_1())
        .child(menu_item(
            "um-logout",
            IconName::Delete,
            crate::tr!("menu.log_out"),
            |window, cx| {
                window.push_notification(Notification::info(crate::tr!("menu.logged_out")), cx);
            },
            cx,
        ))
        .text_color(text_color())
}

pub(crate) fn add_menu_content(
    cx: &mut Context<PopoverState>,
    _ws_on: bool,
    panel: WeakEntity<ConversationPanel>,
) -> Stateful<Div> {
    let panel_image = panel.clone();
    let panel_generate = panel.clone();
    v_flex()
        .id("add-menu-content")
        .w(px(260.))
        .py_1()
        .child(menu_item(
            "am-files",
            IconName::GalleryVerticalEnd,
            crate::tr!("menu.add_photo"),
            move |window, cx| {
                if let Some(panel) = panel_image.upgrade() {
                    panel.update(cx, |this, cx| this.select_local_images(window, cx));
                }
            },
            cx,
        ))
        .child(menu_item(
            "am-generate-image",
            IconName::Palette,
            crate::tr!("menu.generate_image"),
            move |window, cx| {
                if let Some(panel) = panel_generate.upgrade() {
                    panel.update(cx, |this, cx| this.send_image_generation_sample(window, cx));
                }
            },
            cx,
        ))
        .child(menu_item(
            "am-screen",
            IconName::Frame,
            crate::tr!("menu.screenshot"),
            |window, cx| {
                window.push_notification(
                    Notification::info(crate::tr!("menu.screenshot_captured")),
                    cx,
                );
            },
            cx,
        ))
        .child(menu_item(
            "am-proj",
            IconName::Folder,
            crate::tr!("project.add_to_project"),
            |window, cx| {
                window.push_notification(Notification::info(crate::tr!("menu.pick_project")), cx);
            },
            cx,
        ))
        .child(div().h(px(1.)).bg(border_color()).my_1())
        .child(menu_item(
            "am-skills",
            IconName::SquareTerminal,
            crate::tr!("menu.skills"),
            |window, cx| {
                window.push_notification(Notification::info(crate::tr!("menu.skills_opened")), cx);
            },
            cx,
        ))
        .child(menu_item(
            "am-conn",
            IconName::Network,
            crate::tr!("menu.connectors"),
            |window, cx| {
                window.push_notification(
                    Notification::info(crate::tr!("menu.connectors_opened")),
                    cx,
                );
            },
            cx,
        ))
        .child(div().h(px(1.)).bg(border_color()).my_1())
        .child(menu_item(
            "am-web",
            IconName::Globe,
            crate::tr!("menu.web_search"),
            |window, cx| {
                window.push_notification(
                    Notification::info(crate::tr!("menu.web_search_toggled")),
                    cx,
                );
            },
            cx,
        ))
        .text_color(text_color())
}

pub(crate) fn model_menu_content(
    cx: &mut Context<PopoverState>,
    adaptive: bool,
    weak: WeakEntity<ClaudeApp>,
) -> Stateful<Div> {
    const MODEL_MENU_ROW_HEIGHT: f32 = 30.;
    const MODEL_MENU_MAX_HEIGHT: f32 = 360.;

    // (provider_id, model_id, is_current) for every selected model.
    let mut rows: Vec<(usize, String, bool)> = Vec::new();
    if let Some(app) = weak.upgrade() {
        let app = app.read(cx);
        let current = app.current_model_ref.clone();
        for provider in &app.providers {
            for model in &provider.models {
                if model.selected {
                    let is_current = current
                        .as_ref()
                        .is_some_and(|c| c.provider_id == provider.id && c.model_id == model.id);
                    rows.push((provider.id, model.id.clone(), is_current));
                }
            }
        }
    }
    rows.sort_by(|a, b| {
        model_group_key(&a.1)
            .cmp(&model_group_key(&b.1))
            .then_with(|| a.1.to_ascii_lowercase().cmp(&b.1.to_ascii_lowercase()))
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut menu = v_flex().id("model-menu-content").w(px(320.)).py_1();
    if rows.is_empty() {
        menu = menu.child(
            div()
                .px_3p5()
                .py_2p5()
                .text_size(px(12.5))
                .text_color(text_3())
                .child(crate::tr!("menu.no_models")),
        );
    } else {
        let model_list_height =
            ((rows.len() as f32) * MODEL_MENU_ROW_HEIGHT).min(MODEL_MENU_MAX_HEIGHT);
        let mut model_list = v_flex()
            .id("model-menu-list")
            .h(px(model_list_height))
            .overflow_y_scrollbar();
        for (provider_id, model_id, is_current) in rows {
            model_list = model_list.child(provider_model_row(
                provider_id,
                model_id,
                is_current,
                weak.clone(),
                cx,
            ));
        }
        menu = menu.child(model_list);
    }

    menu.child(div().h(px(1.)).bg(border_color()).my_1())
        .child(
            h_flex()
                .px_3p5()
                .py_2p5()
                .gap_2p5()
                .items_center()
                .child(
                    v_flex()
                        .flex_1()
                        .child(
                            div()
                                .text_size(px(13.5))
                                .child(crate::tr!("menu.adaptive_thinking")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(text_3())
                                .child(crate::tr!("menu.adaptive_thinking_hint")),
                        ),
                )
                .child(Switch::new("adapt-tog").checked(adaptive).on_click(
                    move |checked, _, cx| {
                        if let Some(v) = weak.upgrade() {
                            v.update(cx, |this, cx| {
                                this.settings.adaptive_thinking = *checked;
                                cx.notify();
                            });
                        }
                    },
                )),
        )
        .text_color(text_color())
}

pub(crate) fn mcp_menu_content(
    _cx: &mut Context<PopoverState>,
    config_path: Option<PathBuf>,
    servers: Result<Vec<McpServerInfo>, String>,
    weak: WeakEntity<ClaudeApp>,
) -> Stateful<Div> {
    let configured = config_path.as_ref().is_some_and(|path| path.is_file());
    let config_label: SharedString = config_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| crate::tr!("menu.mcp_config_unavailable").to_string())
        .into();

    let mut menu = v_flex()
        .id("mcp-menu-content")
        .w(px(320.))
        .py_1()
        .text_color(text_color())
        .child(
            h_flex()
                .px_3p5()
                .py_2p5()
                .gap_2p5()
                .items_center()
                .child(
                    Icon::new(IconName::SquareTerminal)
                        .size_4()
                        .text_color(text_2()),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .child("MCP"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(text_3())
                                .truncate()
                                .child(config_label),
                        ),
                ),
        )
        .child(div().h(px(1.)).bg(border_color()).my_1());

    match servers {
        Ok(servers) if servers.is_empty() => {
            let status: SharedString = if configured {
                crate::tr!("menu.mcp_no_servers")
            } else {
                crate::tr!("menu.mcp_not_found")
            };
            menu = menu.child(
                div()
                    .px_3p5()
                    .py_2p5()
                    .text_size(px(12.5))
                    .text_color(text_3())
                    .child(status),
            );
        }
        Ok(servers) => {
            let enabled_count = servers.iter().filter(|server| server.enabled).count();
            menu = menu.child(
                h_flex()
                    .px_3p5()
                    .pb_1p5()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(text_3())
                            .child(crate::tr!("menu.mcp_servers")),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(text_3())
                            .child(crate::tr!(
                                "menu.mcp_enabled_count",
                                enabled = enabled_count,
                                total = servers.len()
                            )),
                    ),
            );

            for (ix, server) in servers.into_iter().enumerate() {
                menu = menu.child(mcp_server_row(ix, server, weak.clone()));
            }
        }
        Err(err) => {
            menu = menu.child(
                v_flex()
                    .px_3p5()
                    .py_2p5()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .child(crate::tr!("menu.mcp_read_failed")),
                    )
                    .child(div().text_size(px(12.)).text_color(text_3()).child(err)),
            );
        }
    }

    menu
}

fn mcp_server_row(ix: usize, server: McpServerInfo, weak: WeakEntity<ClaudeApp>) -> Stateful<Div> {
    let name = server.name.clone();
    let label: SharedString = server.name.clone().into();
    let detail: SharedString = if server.config_enabled {
        server.command.clone().into()
    } else {
        crate::tr!(
            "menu.mcp_disabled_in_config",
            command = server.command.clone()
        )
    };

    h_flex()
        .id(SharedString::from(format!("mcp-server-row-{ix}")))
        .px_3p5()
        .py_2p5()
        .gap_2p5()
        .items_center()
        .child(Icon::new(IconName::Network).size_3p5().text_color(text_2()))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(text_3())
                        .truncate()
                        .child(detail),
                ),
        )
        .child(
            Switch::new(format!("mcp-server-tog-{ix}"))
                .checked(server.enabled)
                .on_click(move |checked, _, cx| {
                    if let Some(app) = weak.upgrade() {
                        let name = name.clone();
                        app.update(cx, |this, cx| {
                            this.set_mcp_server_enabled(name, *checked, cx);
                        });
                    }
                }),
        )
}

fn model_group_key(model_id: &str) -> String {
    model_id
        .split_once('-')
        .map_or(model_id, |(group, _)| group)
        .to_ascii_lowercase()
}

fn provider_model_row(
    provider_id: usize,
    model_id: String,
    is_current: bool,
    weak: WeakEntity<ClaudeApp>,
    cx: &mut Context<PopoverState>,
) -> Stateful<Div> {
    let row_id = SharedString::from(format!("model-{provider_id}-{model_id}"));
    let label = SharedString::from(model_id.clone());
    v_flex()
        .id(row_id)
        .px_3p5()
        .py_1p5()
        .cursor_pointer()
        .hover(|this| this.bg(hover_surface()))
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(label),
                )
                .when(is_current, |this| {
                    this.child(
                        Icon::new(IconName::Check)
                            .size_3p5()
                            .text_color(text_color()),
                    )
                }),
        )
        .on_click(cx.listener(move |_, _, window, cx| {
            let model_id = model_id.clone();
            if let Some(v) = weak.upgrade() {
                v.update(cx, |this, cx| {
                    this.select_model(provider_id, model_id.clone(), cx);
                });
                window.push_notification(
                    Notification::info(crate::tr!("menu.switched_model", model = model_id)),
                    cx,
                );
            }
        }))
}

pub(crate) fn menu_item<T: 'static>(
    id: &'static str,
    icon: IconName,
    label: impl IntoElement + 'static,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
    cx: &mut Context<T>,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .px_3p5()
        .py_2p5()
        .gap_2p5()
        .items_center()
        .cursor_pointer()
        .text_size(px(13.5))
        .text_color(text_color())
        .hover(|this| this.bg(hover_surface()))
        .child(Icon::new(icon).size_3p5().text_color(text_2()))
        .child(div().flex_1().child(label))
        .on_click(cx.listener(move |_, _, window, cx| on_click(window, cx)))
}
