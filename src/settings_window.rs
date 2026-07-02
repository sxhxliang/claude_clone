use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, Sizable as _, TitleBar, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogFooter, DialogHeader, DialogTitle},
    h_flex,
    input::{Input, InputEvent, InputState},
    notification::Notification,
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::ClaudeApp;
use crate::dialogs::settings_row_switch;
use crate::provider_settings::{ProviderSettings, SettingsSection};
use crate::store;
use crate::theme::{bg_color, border_color, hover_surface, sidebar_bg, text_2, text_3, text_color};

pub(crate) struct SettingsWindow {
    app: WeakEntity<ClaudeApp>,
    provider_settings: Entity<ProviderSettings>,
    mcp_input: Entity<InputState>,
    mcp_status: SharedString,
    mcp_error: Option<SharedString>,
    mcp_dirty: bool,
    selected_section: SettingsSection,
    _subscriptions: Vec<Subscription>,
}

struct GeneralSettingsSnapshot {
    memory: bool,
    websearch: bool,
    typing: bool,
    persist_conversations: bool,
    document_parsing_enabled: bool,
    document_ocr_enabled: bool,
    storage_dir: SharedString,
    config_dir: SharedString,
    save_error: Option<SharedString>,
}

impl SettingsWindow {
    pub(crate) fn new(
        app: WeakEntity<ClaudeApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let provider_settings = cx.new(|cx| ProviderSettings::new(app.clone(), window, cx));
        let (mcp_text, mcp_status, mcp_error) = match store::load_mcp_config_text() {
            Ok(text) => (text, "mcp.json 已加载".into(), None),
            Err(err) => (
                store::default_mcp_config_text(),
                "使用默认 MCP 配置模板".into(),
                Some(err.into()),
            ),
        };
        let mcp_input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .line_number(true)
                .soft_wrap(true)
                .default_value(mcp_text)
        });
        let subscriptions = vec![cx.subscribe_in(
            &mcp_input,
            window,
            |this: &mut SettingsWindow, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.mcp_dirty = true;
                    this.mcp_error = None;
                    this.mcp_status = "MCP 配置有未保存更改".into();
                    cx.notify();
                }
            },
        )];
        Self {
            app,
            provider_settings,
            mcp_input,
            mcp_status,
            mcp_error,
            mcp_dirty: false,
            selected_section: SettingsSection::ModelManagement,
            _subscriptions: subscriptions,
        }
    }

    fn render_nav_item(
        &self,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.selected_section == section;
        h_flex()
            .id(SharedString::from(format!(
                "settings-nav-{}",
                section.label()
            )))
            .w_full()
            .px_3()
            .py_3()
            .rounded(px(14.))
            .items_center()
            .gap_3()
            .cursor_pointer()
            .bg(if selected { bg_color() } else { sidebar_bg() })
            .hover(|this| this.bg(hover_surface()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_section = section;
                cx.notify();
            }))
            .child(
                div()
                    .size(px(34.))
                    .rounded_full()
                    .bg(if selected {
                        hsla(145.0 / 360.0, 0.58, 0.92, 1.0)
                    } else {
                        hsla(40.0 / 360.0, 0.12, 0.90, 1.0)
                    })
                    .text_color(if selected {
                        hsla(145.0 / 360.0, 0.48, 0.34, 1.0)
                    } else {
                        text_2()
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(section.icon()).small()),
            )
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_color())
                            .child(section.label()),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(text_3())
                            .child(section.sublabel()),
                    ),
            )
    }

    fn render_general_settings(
        &self,
        settings: GeneralSettingsSnapshot,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app = self.app.clone();
        div()
            .size_full()
            .overflow_y_scrollbar()
            .bg(hsla(0.0, 0.0, 1.0, 1.0))
            .child(
                v_flex()
                    .min_h_full()
                    .px_8()
                    .py_7()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(30.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(text_color())
                            .child("通用"),
                    )
                    .child(
                        div()
                            .pb_4()
                            .text_size(px(13.))
                            .text_color(text_3())
                            .child("配置对话体验、联网检索与记忆行为。"),
                    )
                    .child(settings_row_switch(
                        "保存聊天历史",
                        "重启后恢复 Chat 模式对话、标题与窗口布局",
                        "persist-chat-history-tog",
                        settings.persist_conversations,
                        {
                            let app = app.clone();
                            move |checked, cx| {
                                if let Some(v) = app.upgrade() {
                                    v.update(cx, |this, cx| {
                                        this.set_persist_conversations(checked, cx);
                                    });
                                }
                            }
                        },
                    ))
                    .child(settings_row_switch(
                        "文档解析",
                        "添加附件时解析文档内容并随 Chat 请求发送",
                        "document-parsing-tog",
                        settings.document_parsing_enabled,
                        {
                            let app = app.clone();
                            move |checked, cx| {
                                if let Some(v) = app.upgrade() {
                                    v.update(cx, |this, cx| {
                                        this.set_document_parsing_enabled(checked, cx);
                                    });
                                }
                            }
                        },
                    ))
                    .child(settings_row_switch(
                        "启用 OCR",
                        "解析扫描件和图片文字，可能增加附件处理时间",
                        "document-ocr-tog",
                        settings.document_ocr_enabled,
                        {
                            let app = app.clone();
                            move |checked, cx| {
                                if let Some(v) = app.upgrade() {
                                    v.update(cx, |this, cx| {
                                        this.set_document_ocr_enabled(checked, cx);
                                    });
                                }
                            }
                        },
                    ))
                    .child(
                        h_flex()
                            .py_3()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(border_color())
                            .gap_4()
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_size(px(13.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child("存储目录"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(text_3())
                                            .child("保存对话历史与窗口布局数据"),
                                    )
                                    .child(
                                        div()
                                            .pt_1()
                                            .text_size(px(12.))
                                            .text_color(text_2())
                                            .truncate()
                                            .child(settings.storage_dir.clone()),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("choose-storage-dir").label("选择").on_click({
                                            cx.listener(|this, _, window, cx| {
                                                let paths =
                                                    cx.prompt_for_paths(PathPromptOptions {
                                                        files: false,
                                                        directories: true,
                                                        multiple: false,
                                                        prompt: Some("选择存储目录".into()),
                                                    });
                                                let app = this.app.clone();
                                                cx.spawn_in(window, async move |settings, cx| {
                                                    let Ok(Ok(Some(paths))) = paths.await else {
                                                        return;
                                                    };
                                                    let Some(path) = paths.into_iter().next()
                                                    else {
                                                        return;
                                                    };
                                                    if let Some(app) = app.upgrade() {
                                                        let notification = match app
                                                            .update(cx, |app, cx| {
                                                                app.set_storage_dir(path, cx)
                                                            }) {
                                                            Ok(()) => {
                                                                Notification::success("存储目录已更新")
                                                            }
                                                            Err(err) => Notification::error(err),
                                                        };
                                                        _ = settings.update_in(cx, |_, window, cx| {
                                                            window.push_notification(
                                                                notification,
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                    _ = settings.update(cx, |_, cx| cx.notify());
                                                })
                                                .detach();
                                            })
                                        }),
                                    )
                                    .child(
                                        Button::new("reset-storage-dir").label("重置").on_click({
                                            let app = app.clone();
                                            move |_, window, cx| {
                                                if let Some(v) = app.upgrade() {
                                                    match v.update(cx, |this, cx| {
                                                        this.reset_storage_dir(cx)
                                                    }) {
                                                        Ok(()) => window.push_notification(
                                                            Notification::success(
                                                                "存储目录已重置",
                                                            ),
                                                            cx,
                                                        ),
                                                        Err(err) => window.push_notification(
                                                            Notification::error(err),
                                                            cx,
                                                        ),
                                                    }
                                                }
                                            }
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .py_3()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(border_color())
                            .gap_4()
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_size(px(13.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child("配置文件目录"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(text_3())
                                            .child("保存供应商、模型与通用配置"),
                                    )
                                    .child(
                                        div()
                                            .pt_1()
                                            .text_size(px(12.))
                                            .text_color(text_2())
                                            .truncate()
                                            .child(settings.config_dir.clone()),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(Button::new("choose-config-dir").label("选择").on_click(
                                        {
                                            cx.listener(|this, _, window, cx| {
                                                let paths =
                                                    cx.prompt_for_paths(PathPromptOptions {
                                                        files: false,
                                                        directories: true,
                                                        multiple: false,
                                                        prompt: Some("选择配置文件目录".into()),
                                                    });
                                                let app = this.app.clone();
                                                cx.spawn_in(window, async move |settings, cx| {
                                                    let Ok(Ok(Some(paths))) = paths.await else {
                                                        return;
                                                    };
                                                    let Some(path) = paths.into_iter().next()
                                                    else {
                                                        return;
                                                    };
                                                    if let Some(app) = app.upgrade() {
                                                        let notification = match app
                                                            .update(cx, |app, cx| {
                                                                app.set_config_dir(path, cx)
                                                            }) {
                                                            Ok(()) => {
                                                                Notification::success("配置目录已更新")
                                                            }
                                                            Err(err) => Notification::error(err),
                                                        };
                                                        _ = settings.update_in(cx, |_, window, cx| {
                                                            window.push_notification(
                                                                notification,
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                    _ = settings.update(cx, |_, cx| cx.notify());
                                                })
                                                .detach();
                                            })
                                        },
                                    ))
                                    .child(Button::new("reset-config-dir").label("重置").on_click(
                                        {
                                            let app = app.clone();
                                            move |_, window, cx| {
                                                if let Some(v) = app.upgrade() {
                                                    match v.update(cx, |this, cx| {
                                                        this.reset_config_dir(cx)
                                                    }) {
                                                        Ok(()) => window.push_notification(
                                                            Notification::success(
                                                                "配置目录已重置",
                                                            ),
                                                            cx,
                                                        ),
                                                        Err(err) => window.push_notification(
                                                            Notification::error(err),
                                                            cx,
                                                        ),
                                                    }
                                                }
                                            }
                                        },
                                    )),
                            ),
                    )
                    .when_some(settings.save_error.clone(), |this, error| {
                        this.child(
                            div()
                                .py_2()
                                .px_3()
                                .rounded(px(8.))
                                .border_1()
                                .border_color(cx.theme().danger)
                                .text_size(px(12.5))
                                .text_color(cx.theme().danger)
                                .child(error),
                        )
                    })
                    .child(settings_row_switch(
                        "记忆",
                        "允许 Claude 记住偏好和上下文",
                        "memory-tog",
                        settings.memory,
                        {
                            let app = app.clone();
                            move |checked, cx| {
                                if let Some(v) = app.upgrade() {
                                    v.update(cx, |this, cx| {
                                        this.settings.memory_enabled = checked;
                                        cx.notify();
                                    });
                                }
                            }
                        },
                    ))
                    .child(settings_row_switch(
                        "联网搜索",
                        "默认允许对话使用联网检索",
                        "ws-tog",
                        settings.websearch,
                        {
                            let app = app.clone();
                            move |checked, cx| {
                                if let Some(v) = app.upgrade() {
                                    v.update(cx, |this, cx| {
                                        this.settings.web_search = checked;
                                        cx.notify();
                                    });
                                }
                            }
                        },
                    ))
                    .child(settings_row_switch(
                        "显示输入指示器",
                        "Claude 思考时显示动态状态",
                        "typ-tog",
                        settings.typing,
                        {
                            let app = app.clone();
                            move |checked, cx| {
                                if let Some(v) = app.upgrade() {
                                    v.update(cx, |this, cx| {
                                        this.settings.show_typing = checked;
                                        cx.notify();
                                    });
                                }
                            }
                        },
                    ))
                    .child(
                        div()
                            .pt_5()
                            .pb_1()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().danger)
                            .child("危险操作"),
                    )
                    .child(
                        h_flex()
                            .py_3()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(border_color())
                            .child(
                                v_flex()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_size(px(13.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child("清空已保存历史"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(text_3())
                                            .child("只清理磁盘中的历史记录，不关闭当前窗口"),
                                    ),
                            )
                            .child(
                                Button::new("clear-saved-chat-history")
                                    .label("清空")
                                    .on_click({
                                        let app = app.clone();
                                        move |_, window, cx| {
                                            let app = app.clone();
                                            window.open_dialog(cx, move |dialog, _, _| {
                                                let app = app.clone();
                                                dialog.w(px(440.)).p_0().content(
                                                    move |content, _, cx| {
                                                        content
                                                            .child(
                                                                DialogHeader::new()
                                                                    .px_5()
                                                                    .py_4()
                                                                    .border_b_1()
                                                                    .border_color(cx.theme().border)
                                                                    .child(DialogTitle::new().child(
                                                                        "清空已保存历史？",
                                                                    )),
                                                            )
                                                            .child(
                                                                v_flex()
                                                                    .px_5()
                                                                    .py_4()
                                                                    .gap_2()
                                                                    .child(
                                                                        div()
                                                                            .text_size(px(13.))
                                                                            .text_color(text_color())
                                                                            .child(
                                                                                "这只会清理磁盘中的聊天历史，不关闭当前窗口。",
                                                                            ),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .text_size(px(12.))
                                                                            .text_color(text_3())
                                                                            .child(
                                                                                "下次启动时这些历史不会恢复。",
                                                                            ),
                                                                    ),
                                                            )
                                                            .child(
                                                                DialogFooter::new()
                                                                    .px_5()
                                                                    .py_3p5()
                                                                    .border_t_1()
                                                                    .border_color(cx.theme().border)
                                                                    .child(
                                                                        Button::new(
                                                                            "cancel-clear-history",
                                                                        )
                                                                        .label("取消")
                                                                        .on_click(
                                                                            |_, window, cx| {
                                                                                window
                                                                                    .close_dialog(cx);
                                                                            },
                                                                        ),
                                                                    )
                                                                    .child(
                                                                        Button::new(
                                                                            "confirm-clear-history",
                                                                        )
                                                                        .primary()
                                                                        .label("清空")
                                                                        .on_click({
                                                                            let app = app.clone();
                                                                            move |_, window, cx| {
                                                                                window
                                                                                    .close_dialog(cx);
                                                                                if let Some(v) =
                                                                                    app.upgrade()
                                                                                {
                                                                                    match v.update(
                                                                                        cx,
                                                                                        |this, _| {
                                                                                            this.clear_saved_conversations()
                                                                                        },
                                                                                    ) {
                                                                                        Ok(()) => {
                                                                                            window.push_notification(
                                                                                                Notification::info(
                                                                                                    "已清空保存的对话历史",
                                                                                                ),
                                                                                                cx,
                                                                                            );
                                                                                        }
                                                                                        Err(err) => {
                                                                                            window.push_notification(
                                                                                                Notification::error(err),
                                                                                                cx,
                                                                                            );
                                                                                        }
                                                                                    }
                                                                                }
                                                                            }
                                                                        }),
                                                                    ),
                                                            )
                                                    },
                                                )
                                            });
                                        }
                                    }),
                            ),
                    ),
            )
    }

    fn reload_mcp_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match store::load_mcp_config_text() {
            Ok(text) => {
                self.mcp_input
                    .update(cx, |state, cx| state.set_value(text, window, cx));
                self.mcp_dirty = false;
                self.mcp_error = None;
                self.mcp_status = "mcp.json 已重新加载".into();
                window.push_notification(Notification::success("MCP 配置已重新加载"), cx);
            }
            Err(err) => {
                self.mcp_error = Some(err.clone().into());
                self.mcp_status = "重新加载失败".into();
                window.push_notification(Notification::error(err), cx);
            }
        }
        cx.notify();
    }

    fn save_mcp_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.mcp_input.read(cx).value().to_string();
        match store::save_mcp_config_text(&text) {
            Ok((path, formatted)) => {
                self.mcp_input
                    .update(cx, |state, cx| state.set_value(formatted, window, cx));
                self.mcp_dirty = false;
                self.mcp_error = None;
                self.mcp_status = format!("已保存到 {}", path.display()).into();
                window.push_notification(Notification::success("MCP 配置已保存"), cx);
            }
            Err(err) => {
                self.mcp_error = Some(err.clone().into());
                self.mcp_status = "保存失败".into();
                window.push_notification(Notification::error(err), cx);
            }
        }
        cx.notify();
    }

    fn render_mcp_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let config_path: SharedString = store::mcp_config_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "配置目录不可用".to_string())
            .into();
        let status = self
            .mcp_error
            .clone()
            .unwrap_or_else(|| self.mcp_status.clone());
        let status_color = if self.mcp_error.is_some() {
            cx.theme().danger
        } else if self.mcp_dirty {
            text_2()
        } else {
            text_3()
        };

        div()
            .size_full()
            .overflow_y_scrollbar()
            .bg(hsla(0.0, 0.0, 1.0, 1.0))
            .child(
                v_flex()
                    .min_h_full()
                    .px_8()
                    .py_7()
                    .gap_4()
                    .child(
                        h_flex()
                            .items_start()
                            .justify_between()
                            .gap_4()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(30.))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(text_color())
                                            .child("MCP 配置"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(text_3())
                                            .child("直接编辑并保存 Chat 模式使用的 mcp.json。"),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("reload-mcp-config")
                                            .label("重新加载")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.reload_mcp_config(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("save-mcp-config")
                                            .primary()
                                            .label("保存")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save_mcp_config(window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(text_3())
                                    .child("文件路径"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(text_2())
                                    .truncate()
                                    .child(config_path),
                            ),
                    )
                    .child(
                        div()
                            .h(px(460.))
                            .w_full()
                            .overflow_hidden()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(border_color())
                            .child(
                                Input::new(&self.mcp_input)
                                    .h_full()
                                    .appearance(false)
                                    .bordered(false),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(px(12.5))
                                    .text_color(status_color)
                                    .truncate()
                                    .child(status),
                            )
                            .child(div().text_size(px(12.)).text_color(text_3()).child(
                                if self.mcp_dirty {
                                    "未保存"
                                } else {
                                    "已同步"
                                },
                            )),
                    ),
            )
    }

    fn render_title_bar(&self) -> impl IntoElement {
        TitleBar::new().child(
            h_flex()
                .w_full()
                .pr_2()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .min_w_0()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .size(px(24.))
                                .rounded_full()
                                .bg(hsla(145.0 / 360.0, 0.58, 0.92, 1.0))
                                .text_color(hsla(145.0 / 360.0, 0.48, 0.34, 1.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new(self.selected_section.icon()).small()),
                        )
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(text_color())
                                .child("设置"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(text_3())
                                .truncate()
                                .child(self.selected_section.sublabel()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(text_3())
                        .child(self.selected_section.label()),
                ),
        )
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let general_settings = match self.app.upgrade() {
            Some(app) => {
                let app = app.read(cx);
                let settings = &app.settings;
                GeneralSettingsSnapshot {
                    memory: settings.memory_enabled,
                    websearch: settings.web_search,
                    typing: settings.show_typing,
                    persist_conversations: settings.persist_conversations,
                    document_parsing_enabled: settings.document_parsing_enabled,
                    document_ocr_enabled: settings.document_ocr_enabled,
                    storage_dir: settings.storage_dir.clone(),
                    config_dir: settings.config_dir.clone(),
                    save_error: app.last_save_error(),
                }
            }
            None => GeneralSettingsSnapshot {
                memory: false,
                websearch: true,
                typing: true,
                persist_conversations: true,
                document_parsing_enabled: true,
                document_ocr_enabled: false,
                storage_dir: "".into(),
                config_dir: "".into(),
                save_error: None,
            },
        };

        let content = match self.selected_section {
            SettingsSection::ModelManagement => self.provider_settings.clone().into_any_element(),
            SettingsSection::Mcp => self.render_mcp_settings(cx).into_any_element(),
            SettingsSection::Theme => self
                .provider_settings
                .read(cx)
                .render_theme_stub()
                .into_any_element(),
            SettingsSection::General => self
                .render_general_settings(general_settings, cx)
                .into_any_element(),
        };

        let nav_items: Vec<_> = SettingsSection::all()
            .into_iter()
            .map(|section| self.render_nav_item(section, cx).into_any_element())
            .collect();

        v_flex()
            .size_full()
            .bg(bg_color())
            .text_color(text_color())
            .child(self.render_title_bar())
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .h_full()
                    .child(
                        div()
                            .w(px(260.))
                            .h_full()
                            .px_4()
                            .py_5()
                            .border_r_1()
                            .border_color(border_color())
                            .bg(sidebar_bg())
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .pb_4()
                                            .child(
                                                div()
                                                    .text_size(px(18.))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(text_color())
                                                    .child("设置"),
                                            )
                                            .child(
                                                div()
                                                    .pt_1()
                                                    .text_size(px(12.))
                                                    .text_color(text_3())
                                                    .child("三列结构：分类、供应商、配置"),
                                            ),
                                    )
                                    .children(nav_items)
                                    .child(
                                        div()
                                            .mt_4()
                                            .rounded(px(14.))
                                            .border_1()
                                            .border_color(border_color())
                                            .bg(bg_color())
                                            .px_3()
                                            .py_3()
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(text_2())
                                                    .child("当前窗口专用于设置管理。"),
                                            ),
                                    ),
                            ),
                    )
                    .child(div().flex_1().h_full().child(content)),
            )
    }
}
