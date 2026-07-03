//! The window title bar as its own view: sidebar toggle, logo, the editable
//! conversation-title chip, and the plan pill. It reads title/edit state from
//! `ClaudeApp` and dispatches actions back through it.
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    IconName, InteractiveElementExt as _, Sizable as _, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    popover::Popover,
};

use crate::ClaudeApp;
use crate::menus::chat_title_menu_content;
use crate::theme::{accent, border_color, text_2, text_3, text_color, white_color};

pub(crate) struct TopBar {
    app: WeakEntity<ClaudeApp>,
}

impl TopBar {
    pub(crate) fn new(app: WeakEntity<ClaudeApp>) -> Self {
        Self { app }
    }

    fn render_title_chip(
        &self,
        editing: bool,
        title: SharedString,
        pinned: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(app) = self.app.upgrade() else {
            return div().into_any_element();
        };

        if editing {
            let title_input = app.read(cx).title_input.clone();
            return h_flex()
                .px_2()
                .py_1()
                .gap_1p5()
                .items_center()
                .rounded_md()
                .border_1()
                .border_color(border_color())
                .bg(white_color())
                .w(px(320.))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(13.5))
                        .child(Input::new(&title_input).appearance(false).bordered(false)),
                )
                .into_any_element();
        }

        let weak = self.app.clone();
        h_flex()
            .id("chat-title-group")
            .gap_1()
            .items_center()
            .child(
                div()
                    .id("chat-title-label")
                    .max_w(px(320.))
                    .truncate()
                    .text_size(px(13.5))
                    .text_color(text_color())
                    .cursor_pointer()
                    .child(title)
                    .on_double_click(cx.listener(|this, _, window, cx| {
                        if let Some(app) = this.app.upgrade() {
                            app.update(cx, |app, cx| app.begin_rename(window, cx));
                        }
                    })),
            )
            .child(
                Popover::new("chat-title-menu")
                    .anchor(Anchor::BottomLeft)
                    .p_0()
                    .trigger(
                        Button::new("chat-title-chevron")
                            .ghost()
                            .small()
                            .icon(IconName::ChevronDown),
                    )
                    .content(move |_, _, cx| {
                        chat_title_menu_content(cx, weak.clone(), pinned).into_any_element()
                    }),
            )
            .into_any_element()
    }
}

impl Render for TopBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (in_chat, editing, title, pinned) = match self.app.upgrade() {
            Some(app) => {
                let app = app.read(cx);
                let title = if app.chat_title.is_empty() {
                    crate::tr!("conversation.untitled_short")
                } else {
                    app.chat_title.clone()
                };
                (
                    app.active_conversation_id.is_some(),
                    app.editing_title,
                    title,
                    app.chat_pinned,
                )
            }
            None => (
                false,
                false,
                crate::tr!("conversation.untitled_short"),
                false,
            ),
        };

        TitleBar::new().child(
            h_flex()
                .w_full()
                .pr_2()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(
                                    Button::new("toggle-sb-top")
                                        .ghost()
                                        .small()
                                        .icon(IconName::PanelLeft)
                                        .tooltip(crate::tr!("nav.toggle_sidebar"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(app) = this.app.upgrade() {
                                                app.update(cx, |app, cx| {
                                                    app.sidebar_collapsed = !app.sidebar_collapsed;
                                                    cx.notify();
                                                });
                                            }
                                        })),
                                ),
                        )
                        .child(div().text_size(px(17.)).text_color(accent()).child("✳"))
                        .child(div().text_size(px(13.5)).child("Claude"))
                        .when(in_chat, |this| {
                            this.child(
                                div()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .child(self.render_title_chip(editing, title, pinned, cx)),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(
                            h_flex()
                                .id("plan-pill")
                                .items_center()
                                .gap_1p5()
                                .px_3p5()
                                .py_1()
                                .rounded_full()
                                .border_1()
                                .border_color(border_color())
                                .bg(white_color())
                                .text_size(px(12.5))
                                .text_color(text_2())
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(crate::tr!("nav.free_plan"))
                                .child(div().text_color(text_3()).child("·"))
                                .child(
                                    div()
                                        .id("upgrade-link")
                                        .text_color(text_color())
                                        .cursor_pointer()
                                        .child(crate::tr!("nav.upgrade"))
                                        .on_click(|_, window, cx| {
                                            ClaudeApp::toast(
                                                window,
                                                cx,
                                                crate::tr!("nav.upgrade_to_pro"),
                                            );
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(text_3())
                                .child("Clone"),
                        ),
                ),
        )
    }
}
