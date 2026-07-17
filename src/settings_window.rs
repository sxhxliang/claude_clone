use futures::StreamExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable as _, Icon, IconName, Sizable as _, TitleBar, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogFooter, DialogHeader, DialogTitle},
    h_flex,
    input::{Input, InputEvent, InputState},
    notification::Notification,
    popover::{Popover, PopoverState},
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::ClaudeApp;
use crate::dialogs::settings_row_switch;
use crate::provider_settings::{ProviderSettings, SettingsSection};
use crate::store;
use crate::theme::{
    bg_color, border_color, green, hover_surface, sidebar_bg, text_2, text_3, text_color,
    white_color,
};
use crate::theme_settings::ThemeSettingsView;
use crate::voice_input;

pub(crate) struct SettingsWindow {
    app: WeakEntity<ClaudeApp>,
    provider_settings: Entity<ProviderSettings>,
    theme_settings: Entity<ThemeSettingsView>,
    mcp_input: Entity<InputState>,
    mcp_status: SharedString,
    mcp_error: Option<SharedString>,
    mcp_dirty: bool,
    audio_devices: Vec<String>,
    audio_devices_error: Option<SharedString>,
    selected_section: SettingsSection,
    voice_url_input: Entity<InputState>,
    voice_download: VoiceDownloadState,
    voice_download_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// Live state of the SenseVoice model download triggered from General settings.
enum VoiceDownloadState {
    Idle,
    Downloading { downloaded: u64, total: Option<u64> },
    Error(SharedString),
}

/// Human-readable download progress ("42% · 120.0 MB / 285.0 MB", or just the
/// downloaded size when the server sends no Content-Length).
fn format_download_progress(downloaded: u64, total: Option<u64>) -> SharedString {
    let mb = |bytes: u64| bytes as f64 / 1_048_576.0;
    match total {
        Some(total) if total > 0 => {
            let pct = (downloaded as f64 / total as f64 * 100.0).round() as u64;
            format!("{pct}% · {:.1} MB / {:.1} MB", mb(downloaded), mb(total)).into()
        }
        _ => format!("{:.1} MB", mb(downloaded)).into(),
    }
}

struct GeneralSettingsSnapshot {
    locale: SharedString,
    memory: bool,
    websearch: bool,
    typing: bool,
    persist_conversations: bool,
    document_parsing_enabled: bool,
    document_ocr_enabled: bool,
    audio_input_device: SharedString,
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
        let theme_settings = cx.new(|cx| ThemeSettingsView::new(app.clone(), window, cx));
        let (audio_devices, audio_devices_error) = match voice_input::input_device_names() {
            Ok(devices) => (devices, None),
            Err(err) => (Vec::new(), Some(err.into())),
        };
        let (mcp_text, mcp_status, mcp_error) = match store::load_mcp_config_text() {
            Ok(text) => (text, crate::tr!("settings.mcp.loaded"), None),
            Err(err) => (
                store::default_mcp_config_text(),
                crate::tr!("settings.mcp.default_template"),
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
        let voice_model_url = app
            .upgrade()
            .map(|app| app.read(cx).settings.voice_model_url.to_string())
            .unwrap_or_default();
        let voice_url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::tr!("settings.general.voice_model_url_placeholder"))
                .default_value(voice_model_url)
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &mcp_input,
                window,
                |this: &mut SettingsWindow, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.mcp_dirty = true;
                        this.mcp_error = None;
                        this.mcp_status = crate::tr!("settings.mcp.dirty");
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &voice_url_input,
                window,
                |this: &mut SettingsWindow, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Blur) {
                        this.persist_voice_url(cx);
                    }
                },
            ),
        ];
        Self {
            app,
            provider_settings,
            theme_settings,
            mcp_input,
            mcp_status,
            mcp_error,
            mcp_dirty: false,
            audio_devices,
            audio_devices_error,
            selected_section: SettingsSection::ModelManagement,
            voice_url_input,
            voice_download: VoiceDownloadState::Idle,
            voice_download_task: None,
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
            .id(SharedString::from(format!("settings-nav-{}", section.id())))
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

    fn render_locale_button(
        &self,
        code: &'static str,
        current_locale: &SharedString,
        app: WeakEntity<ClaudeApp>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = current_locale.as_ref() == crate::i18n::normalize_locale(code);
        Button::new(SharedString::from(format!("settings-locale-{code}")))
            .small()
            .label(crate::i18n::language_name(code))
            .when(selected, |this| this.primary())
            .when(!selected, |this| this.outline())
            .on_click(cx.listener(move |_, _, _, cx| {
                if let Some(app) = app.upgrade() {
                    app.update(cx, |app, cx| {
                        app.set_locale(code, cx);
                    });
                }
                cx.notify();
            }))
    }

    fn refresh_audio_devices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match voice_input::input_device_names() {
            Ok(devices) => {
                self.audio_devices = devices;
                self.audio_devices_error = None;
                window.push_notification(
                    Notification::success(crate::tr!("settings.general.audio_devices_refreshed")),
                    cx,
                );
            }
            Err(err) => {
                self.audio_devices.clear();
                self.audio_devices_error = Some(err.clone().into());
                window.push_notification(Notification::error(err), cx);
            }
        }
        cx.notify();
    }

    fn audio_device_menu_row(
        id: impl Into<ElementId>,
        label: SharedString,
        value: String,
        selected: bool,
        app: WeakEntity<ClaudeApp>,
        cx: &mut Context<PopoverState>,
    ) -> Stateful<Div> {
        h_flex()
            .id(id)
            .px_3p5()
            .py_2p5()
            .gap_2p5()
            .items_center()
            .cursor_pointer()
            .text_size(px(13.))
            .text_color(text_color())
            .hover(|this| this.bg(hover_surface()))
            .when(selected, |this| this.bg(bg_color()))
            .child(div().w_4().when(selected, |this| {
                this.child(Icon::new(IconName::Check).size_3p5().text_color(text_2()))
            }))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .on_click(cx.listener(move |popover, _, window, cx| {
                if let Some(app) = app.upgrade() {
                    app.update(cx, |app, cx| {
                        app.set_audio_input_device(value.clone(), cx);
                    });
                }
                popover.dismiss(window, cx);
            }))
    }

    fn audio_device_menu(
        devices: Vec<String>,
        current: String,
        app: WeakEntity<ClaudeApp>,
        cx: &mut Context<PopoverState>,
    ) -> impl IntoElement {
        let mut menu = v_flex()
            .id("audio-input-device-menu")
            .w(px(320.))
            .max_h(px(340.))
            .overflow_y_scrollbar()
            .py_1()
            .child(Self::audio_device_menu_row(
                "audio-input-system-default",
                crate::tr!("settings.general.audio_input_system_default"),
                String::new(),
                current.is_empty(),
                app.clone(),
                cx,
            ));

        for (ix, device) in devices.into_iter().enumerate() {
            let selected = current == device;
            menu = menu.child(Self::audio_device_menu_row(
                ("audio-input-device", ix),
                device.clone().into(),
                device,
                selected,
                app.clone(),
                cx,
            ));
        }

        menu
    }

    fn render_general_settings(
        &self,
        settings: GeneralSettingsSnapshot,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app = self.app.clone();
        let audio_devices = self.audio_devices.clone();
        let audio_devices_error = self.audio_devices_error.clone();
        let audio_input_device = settings.audio_input_device.to_string();
        let audio_input_label: SharedString = if audio_input_device.trim().is_empty() {
            crate::tr!("settings.general.audio_input_system_default")
        } else {
            audio_input_device.clone().into()
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
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(30.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(text_color())
                            .child(crate::tr!("settings.general.title")),
                    )
                    .child(
                        div()
                            .pb_4()
                            .text_size(px(13.))
                            .text_color(text_3())
                            .child(crate::tr!("settings.general.description")),
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
                                            .child(crate::tr!("settings.language.title")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(text_3())
                                            .child(crate::tr!("settings.language.description")),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(self.render_locale_button(
                                        crate::i18n::EN_LOCALE,
                                        &settings.locale,
                                        app.clone(),
                                        cx,
                                    ))
                                    .child(self.render_locale_button(
                                        crate::i18n::ZH_CN_LOCALE,
                                        &settings.locale,
                                        app.clone(),
                                        cx,
                                    )),
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
                                            .child(crate::tr!(
                                                "settings.general.audio_input_device"
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(text_3())
                                            .child(crate::tr!(
                                                "settings.general.audio_input_device_sub"
                                            )),
                                    )
                                    .when_some(audio_devices_error.clone(), |this, error| {
                                        this.child(
                                            div()
                                                .pt_1()
                                                .text_size(px(12.))
                                                .text_color(cx.theme().danger)
                                                .child(error),
                                        )
                                    }),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        Popover::new("audio-input-device-selector")
                                            .anchor(Anchor::BottomRight)
                                            .p_0()
                                            .trigger(
                                                Button::new("audio-input-device-button")
                                                    .outline()
                                                    .small()
                                                    .max_w(px(280.))
                                                    .child(
                                                        h_flex()
                                                            .min_w_0()
                                                            .gap_1p5()
                                                            .child(
                                                                div()
                                                                    .max_w(px(230.))
                                                                    .truncate()
                                                                    .child(
                                                                        audio_input_label.clone(),
                                                                    ),
                                                            )
                                                            .child(
                                                                Icon::new(IconName::ChevronDown)
                                                                    .size_3()
                                                                    .text_color(text_3()),
                                                            ),
                                                    ),
                                            )
                                            .content({
                                                let app = app.clone();
                                                move |_, _, cx| {
                                                    Self::audio_device_menu(
                                                        audio_devices.clone(),
                                                        audio_input_device.clone(),
                                                        app.clone(),
                                                        cx,
                                                    )
                                                    .into_any_element()
                                                }
                                            }),
                                    )
                                    .child(
                                        Button::new("refresh-audio-input-devices")
                                            .ghost()
                                            .small()
                                            .icon(IconName::Redo2)
                                            .tooltip(crate::tr!(
                                                "settings.general.refresh_audio_devices"
                                            ))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.refresh_audio_devices(window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(settings_row_switch(
                        crate::tr!("settings.general.persist_history"),
                        crate::tr!("settings.general.persist_history_sub"),
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
                        crate::tr!("settings.general.document_parsing"),
                        crate::tr!("settings.general.document_parsing_sub"),
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
                        crate::tr!("settings.general.document_ocr"),
                        crate::tr!("settings.general.document_ocr_sub"),
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
                                            .child(crate::tr!("settings.general.storage_dir")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(text_3())
                                            .child(crate::tr!("settings.general.storage_dir_sub")),
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
                                        Button::new("choose-storage-dir").label(crate::tr!("common.choose")).on_click({
                                            cx.listener(|this, _, window, cx| {
                                                let paths =
                                                    cx.prompt_for_paths(PathPromptOptions {
                                                        files: false,
                                                        directories: true,
                                                        multiple: false,
                                                        prompt: Some(crate::tr!(
                                                            "settings.general.choose_storage_dir"
                                                        )),
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
                                                                Notification::success(crate::tr!(
                                                                    "settings.general.storage_updated"
                                                                ))
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
                                        Button::new("reset-storage-dir").label(crate::tr!("common.reset")).on_click({
                                            let app = app.clone();
                                            move |_, window, cx| {
                                                if let Some(v) = app.upgrade() {
                                                    match v.update(cx, |this, cx| {
                                                        this.reset_storage_dir(cx)
                                                    }) {
                                                        Ok(()) => window.push_notification(
                                                            Notification::success(
                                                                crate::tr!(
                                                                    "settings.general.storage_reset"
                                                                ),
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
                                            .child(crate::tr!("settings.general.config_dir")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(text_3())
                                            .child(crate::tr!("settings.general.config_dir_sub")),
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
                                    .child(Button::new("choose-config-dir").label(crate::tr!("common.choose")).on_click(
                                        {
                                            cx.listener(|this, _, window, cx| {
                                                let paths =
                                                    cx.prompt_for_paths(PathPromptOptions {
                                                        files: false,
                                                        directories: true,
                                                        multiple: false,
                                                        prompt: Some(crate::tr!(
                                                            "settings.general.choose_config_dir"
                                                        )),
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
                                                                Notification::success(crate::tr!(
                                                                    "settings.general.config_updated"
                                                                ))
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
                                    .child(Button::new("reset-config-dir").label(crate::tr!("common.reset")).on_click(
                                        {
                                            let app = app.clone();
                                            move |_, window, cx| {
                                                if let Some(v) = app.upgrade() {
                                                    match v.update(cx, |this, cx| {
                                                        this.reset_config_dir(cx)
                                                    }) {
                                                        Ok(()) => window.push_notification(
                                                            Notification::success(
                                                                crate::tr!(
                                                                    "settings.general.config_reset"
                                                                ),
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
                        crate::tr!("settings.general.memory"),
                        crate::tr!("settings.general.memory_sub"),
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
                        crate::tr!("settings.general.web_search"),
                        crate::tr!("settings.general.web_search_sub"),
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
                        crate::tr!("settings.general.typing"),
                        crate::tr!("settings.general.typing_sub"),
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
                    .child(self.render_voice_model(
                        crate::voice_input::model_installed(),
                        crate::voice_input::models_dir_display().into(),
                        cx,
                    ))
                    .child(
                        div()
                            .pt_5()
                            .pb_1()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().danger)
                            .child(crate::tr!("settings.general.danger")),
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
                                            .child(crate::tr!("settings.general.clear_history")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(text_3())
                                            .child(crate::tr!("settings.general.clear_history_sub")),
                                    ),
                            )
                            .child(
                                Button::new("clear-saved-chat-history")
                                    .label(crate::tr!("common.clear"))
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
                                                                        crate::tr!("settings.general.clear_history_title"),
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
                                                                                crate::tr!("settings.general.clear_history_body"),
                                                                            ),
                                                                    )
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
                                                                        .label(crate::tr!("common.cancel"))
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
                                                                        .label(crate::tr!("common.clear"))
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
                                                                                                    crate::tr!("settings.general.history_cleared"),
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

    fn persist_voice_url(&mut self, cx: &mut Context<Self>) {
        let url = self.voice_url_input.read(cx).value().trim().to_string();
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| app.set_voice_model_url(url, cx));
        }
    }

    fn start_voice_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.voice_download_task.is_some() {
            return;
        }
        let url = self.voice_url_input.read(cx).value().trim().to_string();
        if url.is_empty() {
            window.push_notification(
                Notification::error(crate::tr!("settings.general.voice_url_required")),
                cx,
            );
            return;
        }
        self.persist_voice_url(cx);
        self.voice_download = VoiceDownloadState::Downloading {
            downloaded: 0,
            total: None,
        };
        cx.notify();

        let mut rx = crate::voice_input::download_model(url);
        self.voice_download_task = Some(cx.spawn_in(window, async move |this, cx| {
            while let Some(progress) = rx.next().await {
                let finished = matches!(
                    progress,
                    crate::voice_input::DownloadProgress::Done
                        | crate::voice_input::DownloadProgress::Error(_)
                );
                _ = this.update_in(cx, |this, window, cx| {
                    this.apply_download_progress(progress, window, cx);
                });
                if finished {
                    break;
                }
            }
            _ = this.update_in(cx, |this, _, cx| {
                this.voice_download_task = None;
                cx.notify();
            });
        }));
    }

    fn apply_download_progress(
        &mut self,
        progress: crate::voice_input::DownloadProgress,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::voice_input::DownloadProgress;
        match progress {
            DownloadProgress::Progress { downloaded, total } => {
                self.voice_download = VoiceDownloadState::Downloading { downloaded, total };
            }
            DownloadProgress::Done => {
                self.voice_download = VoiceDownloadState::Idle;
                window.push_notification(
                    Notification::success(crate::tr!("settings.general.voice_model_downloaded")),
                    cx,
                );
            }
            DownloadProgress::Error(err) => {
                self.voice_download = VoiceDownloadState::Error(err.clone().into());
                window.push_notification(Notification::error(err), cx);
            }
        }
        cx.notify();
    }

    fn render_voice_model(
        &self,
        installed: bool,
        models_dir: SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let downloading = matches!(self.voice_download, VoiceDownloadState::Downloading { .. });
        let status_text = if installed {
            crate::tr!("settings.general.voice_model_installed")
        } else {
            crate::tr!("settings.general.voice_model_missing")
        };
        let status_color = if installed { green() } else { text_3() };
        let progress_line = match &self.voice_download {
            VoiceDownloadState::Downloading { downloaded, total } => {
                Some(format_download_progress(*downloaded, *total))
            }
            _ => None,
        };
        let error_line = match &self.voice_download {
            VoiceDownloadState::Error(err) => Some(err.clone()),
            _ => None,
        };

        v_flex()
            .py_3()
            .gap_2()
            .border_b_1()
            .border_color(border_color())
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap_4()
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(crate::tr!("settings.general.voice_model")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(text_3())
                                    .child(crate::tr!("settings.general.voice_model_sub")),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(status_color)
                            .child(status_text),
                    ),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .text_size(px(12.))
                    .text_color(text_2())
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(text_3())
                            .child(crate::tr!("settings.general.voice_model_location")),
                    )
                    .child(div().min_w_0().truncate().child(models_dir)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(40.))
                            .px_3()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(border_color())
                            .bg(white_color())
                            .flex()
                            .items_center()
                            .child(
                                Input::new(&self.voice_url_input)
                                    .appearance(false)
                                    .bordered(false)
                                    .w_full(),
                            ),
                    )
                    .child(
                        Button::new("voice-download-model")
                            .primary()
                            .label(if downloading {
                                crate::tr!("settings.general.voice_downloading")
                            } else {
                                crate::tr!("settings.general.voice_download")
                            })
                            .loading(downloading)
                            .disabled(downloading)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_voice_download(window, cx);
                            })),
                    ),
            )
            .when_some(progress_line, |this, line| {
                this.child(div().text_size(px(12.)).text_color(text_2()).child(line))
            })
            .when_some(error_line, |this, err| {
                this.child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().danger)
                        .child(err),
                )
            })
    }

    fn reload_mcp_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match store::load_mcp_config_text() {
            Ok(text) => {
                self.mcp_input
                    .update(cx, |state, cx| state.set_value(text, window, cx));
                self.mcp_dirty = false;
                self.mcp_error = None;
                self.mcp_status = crate::tr!("settings.mcp.reloaded");
                window.push_notification(
                    Notification::success(crate::tr!("settings.mcp.reloaded")),
                    cx,
                );
            }
            Err(err) => {
                self.mcp_error = Some(err.clone().into());
                self.mcp_status = crate::tr!("settings.mcp.reload_failed");
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
                self.mcp_status =
                    crate::tr!("settings.mcp.saved_to", path = path.display().to_string());
                window
                    .push_notification(Notification::success(crate::tr!("settings.mcp.saved")), cx);
            }
            Err(err) => {
                self.mcp_error = Some(err.clone().into());
                self.mcp_status = crate::tr!("settings.mcp.save_failed");
                window.push_notification(Notification::error(err), cx);
            }
        }
        cx.notify();
    }

    fn render_mcp_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let config_path: SharedString = store::mcp_config_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| crate::tr!("common.unavailable").to_string())
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
                                            .child(crate::tr!("settings.mcp.title")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(text_3())
                                            .child(crate::tr!("settings.mcp.description")),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("reload-mcp-config")
                                            .label(crate::tr!("common.reload"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.reload_mcp_config(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("save-mcp-config")
                                            .primary()
                                            .label(crate::tr!("common.save"))
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
                                    .child(crate::tr!("settings.mcp.file_path")),
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
                                    crate::tr!("common.unsaved")
                                } else {
                                    crate::tr!("common.synced")
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
                                .child(crate::tr!("settings.title")),
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
                    locale: settings.locale.clone(),
                    memory: settings.memory_enabled,
                    websearch: settings.web_search,
                    typing: settings.show_typing,
                    persist_conversations: settings.persist_conversations,
                    document_parsing_enabled: settings.document_parsing_enabled,
                    document_ocr_enabled: settings.document_ocr_enabled,
                    audio_input_device: settings.audio_input_device.clone(),
                    storage_dir: settings.storage_dir.clone(),
                    config_dir: settings.config_dir.clone(),
                    save_error: app.last_save_error(),
                }
            }
            None => GeneralSettingsSnapshot {
                locale: crate::i18n::DEFAULT_LOCALE.into(),
                memory: false,
                websearch: true,
                typing: true,
                persist_conversations: true,
                document_parsing_enabled: true,
                document_ocr_enabled: false,
                audio_input_device: "".into(),
                storage_dir: "".into(),
                config_dir: "".into(),
                save_error: None,
            },
        };

        let content = match self.selected_section {
            SettingsSection::ModelManagement => self.provider_settings.clone().into_any_element(),
            SettingsSection::Mcp => self.render_mcp_settings(cx).into_any_element(),
            SettingsSection::Theme => self.theme_settings.clone().into_any_element(),
            SettingsSection::General => self
                .render_general_settings(general_settings, cx)
                .into_any_element(),
        };

        let nav_items: Vec<_> = SettingsSection::all()
            .into_iter()
            .map(|section| self.render_nav_item(section, cx).into_any_element())
            .collect();

        let background = self
            .app
            .upgrade()
            .map(|app| app.read(cx).settings.theme.background.clone())
            .unwrap_or_default();
        let background_layer = (background.scope == crate::theme::BackgroundScope::Window)
            .then(|| {
                background
                    .asset
                    .as_deref()
                    .and_then(crate::theme::asset_path)
                    .map(|path| {
                        img(path)
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .object_fit(match background.fit {
                                crate::theme::BackgroundFit::Fill => ObjectFit::Fill,
                                crate::theme::BackgroundFit::Contain => ObjectFit::Contain,
                                crate::theme::BackgroundFit::Cover => ObjectFit::Cover,
                                crate::theme::BackgroundFit::ScaleDown => ObjectFit::ScaleDown,
                                crate::theme::BackgroundFit::Original => ObjectFit::None,
                            })
                            .opacity(background.opacity)
                            .into_any_element()
                    })
            })
            .flatten();

        v_flex()
            .size_full()
            .relative()
            .bg(bg_color())
            .text_color(text_color())
            .children(background_layer)
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
                                                    .child(crate::tr!("settings.title")),
                                            )
                                            .child(
                                                div()
                                                    .pt_1()
                                                    .text_size(px(12.))
                                                    .text_color(text_3())
                                                    .child(crate::tr!("settings.subtitle")),
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
                                                    .child(crate::tr!("settings.window_note")),
                                            ),
                                    ),
                            ),
                    )
                    .child(div().flex_1().h_full().child(content)),
            )
    }
}
