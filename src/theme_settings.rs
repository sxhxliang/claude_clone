use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    h_flex,
    scroll::ScrollableElement as _,
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    v_flex,
};

use crate::{ClaudeApp, theme};

#[derive(Clone, Copy)]
enum ColorKey {
    Accent,
    Background,
    Sidebar,
    Surface,
    Text,
    TextSecondary,
    TextMuted,
    Border,
    Success,
}

impl ColorKey {
    fn label(self) -> SharedString {
        match self {
            Self::Accent => crate::tr!("settings.theme.accent"),
            Self::Background => crate::tr!("settings.theme.background"),
            Self::Sidebar => crate::tr!("settings.theme.sidebar"),
            Self::Surface => crate::tr!("settings.theme.surface"),
            Self::Text => crate::tr!("settings.theme.text"),
            Self::TextSecondary => crate::tr!("settings.theme.text_secondary"),
            Self::TextMuted => crate::tr!("settings.theme.text_muted"),
            Self::Border => crate::tr!("settings.theme.border"),
            Self::Success => crate::tr!("settings.theme.success"),
        }
    }

    fn get(self, colors: &theme::ThemeColorOverrides) -> Hsla {
        match self {
            Self::Accent => colors.accent.unwrap_or_else(theme::accent),
            Self::Background => colors.background.unwrap_or_else(theme::bg_color),
            Self::Sidebar => colors.sidebar.unwrap_or_else(theme::sidebar_bg),
            Self::Surface => colors.surface.unwrap_or_else(theme::white_color),
            Self::Text => colors.text.unwrap_or_else(theme::text_color),
            Self::TextSecondary => colors.text_secondary.unwrap_or_else(theme::text_2),
            Self::TextMuted => colors.text_muted.unwrap_or_else(theme::text_3),
            Self::Border => colors.border.unwrap_or_else(theme::border_color),
            Self::Success => colors.success.unwrap_or_else(theme::green),
        }
    }

    fn set(self, colors: &mut theme::ThemeColorOverrides, value: Hsla) {
        match self {
            Self::Accent => colors.accent = Some(value),
            Self::Background => colors.background = Some(value),
            Self::Sidebar => colors.sidebar = Some(value),
            Self::Surface => colors.surface = Some(value),
            Self::Text => colors.text = Some(value),
            Self::TextSecondary => colors.text_secondary = Some(value),
            Self::TextMuted => colors.text_muted = Some(value),
            Self::Border => colors.border = Some(value),
            Self::Success => colors.success = Some(value),
        }
    }
}

const COLOR_KEYS: [ColorKey; 9] = [
    ColorKey::Accent,
    ColorKey::Background,
    ColorKey::Sidebar,
    ColorKey::Surface,
    ColorKey::Text,
    ColorKey::TextSecondary,
    ColorKey::TextMuted,
    ColorKey::Border,
    ColorKey::Success,
];

pub(crate) struct ThemeSettingsView {
    app: WeakEntity<ClaudeApp>,
    colors: Vec<(ColorKey, Entity<ColorPickerState>)>,
    opacity: Entity<SliderState>,
    _subscriptions: Vec<Subscription>,
}

impl ThemeSettingsView {
    pub(crate) fn new(
        app: WeakEntity<ClaudeApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let colors = COLOR_KEYS
            .into_iter()
            .map(|key| {
                let value = key.get(&theme::palette_from_component(cx));
                let state = cx.new(|cx| ColorPickerState::new(window, cx).default_value(value));
                (key, state)
            })
            .collect::<Vec<_>>();
        let opacity_value = app
            .upgrade()
            .map(|app| app.read(cx).settings.theme.background.opacity * 100.0)
            .unwrap_or(25.0);
        let opacity = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(opacity_value)
        });

        let mut subscriptions = Vec::new();
        for (key, state) in &colors {
            let key = *key;
            subscriptions.push(cx.subscribe(state, move |this, _, event, cx| {
                if let ColorPickerEvent::Change(Some(value)) = event {
                    this.update_color(key, *value, cx);
                }
            }));
        }
        subscriptions.push(cx.subscribe(&opacity, |this, _, event, cx| {
            if let SliderEvent::Release(SliderValue::Single(value)) = event {
                this.update_background(|settings| settings.background.opacity = value / 100.0, cx);
            }
        }));

        Self {
            app,
            colors,
            opacity,
            _subscriptions: subscriptions,
        }
    }

    fn update_color(&mut self, key: ColorKey, value: Hsla, cx: &mut Context<Self>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        app.update(cx, |app, cx| {
            let mut settings = app.settings.theme.clone();
            if settings.selection == theme::ThemeSelection::Preset {
                settings.custom_base = settings.preset.clone();
            }
            settings.selection = theme::ThemeSelection::Custom;
            ColorKey::set(key, &mut settings.custom, value);
            app.set_theme_settings(settings, cx);
        });
    }

    fn update_background(
        &mut self,
        update: impl FnOnce(&mut theme::ThemeSettings),
        cx: &mut Context<Self>,
    ) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        app.update(cx, |app, cx| {
            let mut settings = app.settings.theme.clone();
            update(&mut settings);
            app.set_theme_settings(settings, cx);
        });
    }

    fn sync_color_states(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let colors = theme::palette_from_component(cx);
        for (key, state) in &self.colors {
            let value = key.get(&colors);
            state.update(cx, |state, cx| state.set_value(value, window, cx));
        }
    }

    fn render_color_row(&self, key: ColorKey, state: Entity<ColorPickerState>) -> impl IntoElement {
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .py_2()
            .border_b_1()
            .border_color(theme::border_color().opacity(0.5))
            .child(div().text_size(px(13.)).child(key.label()))
            .child(ColorPicker::new(&state).small())
    }

    fn render_preset_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let custom_label = crate::tr!("settings.theme.custom").to_string();
        let mut names = vec![custom_label.clone(), theme::CLAUDE_THEME_NAME.to_string()];
        names.extend(
            gpui_component::ThemeRegistry::global(cx)
                .sorted_themes()
                .iter()
                .map(|config| config.name.to_string()),
        );
        names.sort();
        names.dedup();
        let selected = self
            .app
            .upgrade()
            .map(|app| {
                let settings = &app.read(cx).settings.theme;
                if settings.selection == theme::ThemeSelection::Custom {
                    custom_label.clone()
                } else {
                    settings.preset.clone()
                }
            })
            .unwrap_or_else(|| theme::CLAUDE_THEME_NAME.to_string());

        let app = self.app.clone();
        v_flex()
            .gap_1()
            .children(names.into_iter().map(move |name| {
                let selected = selected == name;
                let click_name = name.clone();
                let custom_label = custom_label.clone();
                Button::new(SharedString::from(format!("theme-preset-{name}")))
                    .w_full()
                    .justify_start()
                    .label(name)
                    .when(selected, |button| button.primary())
                    .when(!selected, |button| button.outline())
                    .on_click({
                        let app = app.clone();
                        move |_, window, cx| {
                            if click_name == custom_label {
                                if let Some(app) = app.upgrade() {
                                    _ = app.update(cx, |app, cx| {
                                        let mut settings = app.settings.theme.clone();
                                        settings.selection = theme::ThemeSelection::Custom;
                                        app.set_theme_settings(settings, cx);
                                    });
                                }
                            } else if let Some(app) = app.upgrade() {
                                _ = app.update(cx, |app, cx| {
                                    let mut settings = app.settings.theme.clone();
                                    settings.selection = theme::ThemeSelection::Preset;
                                    settings.preset = click_name.clone();
                                    app.set_theme_settings(settings, cx);
                                });
                            }
                            window.refresh();
                        }
                    })
            }))
    }

    fn render_background(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self
            .app
            .upgrade()
            .map(|app| app.read(cx).settings.theme.background.clone())
            .unwrap_or_default();
        let asset_label = settings
            .asset
            .clone()
            .unwrap_or_else(|| crate::tr!("settings.theme.no_image").to_string());
        let app = self.app.clone();
        v_flex()
            .gap_2()
            .child(div().text_size(px(13.)).child(asset_label))
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("choose-theme-background").label(crate::tr!("settings.theme.choose_image")).on_click({
                        let app = app.clone();
                        cx.listener(move |_, _, window, cx| {
                            let task = cx.prompt_for_paths(PathPromptOptions {
                                files: true,
                                directories: false,
                                multiple: false,
                                prompt: Some(crate::tr!("settings.theme.choose_prompt")),
                            });
                            let app = app.clone();
                            cx.spawn_in(window, async move |view, cx| {
                                let Ok(Ok(Some(paths))) = task.await else { return; };
                                let Some(path) = paths.into_iter().next() else { return; };
                                if let Some(app) = app.upgrade() {
                                    let error = app.update(cx, |app, cx| {
                                        app.import_theme_background(&path, cx).err()
                                    });
                                    if let Some(error) = error {
                                        _ = view.update_in(cx, |_, window, cx| {
                                            window.push_notification(
                                                gpui_component::notification::Notification::error(error),
                                                cx,
                                            );
                                        });
                                    }
                                }
                            }).detach();
                        })
                    }))
                    .child(Button::new("remove-theme-background").label(crate::tr!("settings.theme.remove_image")).on_click({
                        cx.listener(|this, _, _, cx| {
                            if let Some(app) = this.app.upgrade() {
                                app.update(cx, |app, cx| app.remove_theme_background(cx));
                            }
                        })
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(Button::new("background-window").label(crate::tr!("settings.theme.whole_window"))
                        .when(settings.scope == theme::BackgroundScope::Window, |b| b.primary())
                        .when(settings.scope != theme::BackgroundScope::Window, |b| b.outline())
                        .on_click({
                        cx.listener(|this, _, _, cx| this.update_background(
                            |s| s.background.scope = theme::BackgroundScope::Window,
                            cx,
                        ))
                    }))
                    .child(Button::new("background-chat").label(crate::tr!("settings.theme.chat_only"))
                        .when(settings.scope == theme::BackgroundScope::Chat, |b| b.primary())
                        .when(settings.scope != theme::BackgroundScope::Chat, |b| b.outline())
                        .on_click({
                        cx.listener(|this, _, _, cx| this.update_background(
                            |s| s.background.scope = theme::BackgroundScope::Chat,
                            cx,
                        ))
                    }))
            )
            .child(
                h_flex().gap_1().children([
                    ("fit-cover", crate::tr!("settings.theme.fit_cover"), theme::BackgroundFit::Cover),
                    ("fit-contain", crate::tr!("settings.theme.fit_contain"), theme::BackgroundFit::Contain),
                    ("fit-fill", crate::tr!("settings.theme.fit_fill"), theme::BackgroundFit::Fill),
                    ("fit-scale-down", crate::tr!("settings.theme.fit_scale_down"), theme::BackgroundFit::ScaleDown),
                    ("fit-original", crate::tr!("settings.theme.fit_original"), theme::BackgroundFit::Original),
                ].into_iter().map(|(id, label, fit)| {
                    Button::new(id)
                        .small()
                        .label(label)
                        .when(settings.fit == fit, |b| b.primary())
                        .when(settings.fit != fit, |b| b.outline())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.update_background(|s| s.background.fit = fit, cx);
                        }))
                }))
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_size(px(12.)).child(crate::tr!("settings.theme.opacity")))
                    .child(Slider::new(&self.opacity).flex_1())
                    .child(div().text_size(px(12.)).child(format!("{}%", (settings.opacity * 100.).round() as u32))),
            )
    }
}

impl Render for ThemeSettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.colors.clone();
        v_flex()
            .size_full()
            .gap_4()
            .p_6()
            .overflow_y_scrollbar()
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(FontWeight::BOLD)
                    .child(crate::tr!("settings.theme.title")),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::text_3())
                    .child(crate::tr!("settings.theme.description")),
            )
            .child(self.render_preset_buttons(cx))
            .child(
                v_flex().gap_0p5().children(
                    colors
                        .into_iter()
                        .map(|(key, state)| self.render_color_row(key, state)),
                ),
            )
            .child(self.render_background(cx))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.sync_color_states(window, cx);
                }),
            )
    }
}
