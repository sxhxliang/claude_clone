//! Row widgets used inside the Settings / Customize dialogs.
use gpui::*;
use gpui_component::{h_flex, switch::Switch, v_flex};

use crate::theme::{bg_color, border_color, text_3};

pub(crate) fn settings_row_switch(
    label: impl Into<SharedString>,
    sub: impl Into<SharedString>,
    switch_id: &'static str,
    checked: bool,
    on_click: impl Fn(bool, &mut App) + 'static,
) -> impl IntoElement {
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
                        .child(label.into()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(text_3())
                        .child(sub.into()),
                ),
        )
        .child(
            Switch::new(switch_id)
                .checked(checked)
                .on_click(move |checked, _, cx| on_click(*checked, cx)),
        )
}

pub(crate) fn static_row(
    label: impl Into<SharedString>,
    sub: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> impl IntoElement {
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
                        .child(label.into()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(text_3())
                        .child(sub.into()),
                ),
        )
        .child(
            div()
                .px_2p5()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(border_color())
                .bg(bg_color())
                .text_size(px(13.))
                .w(px(180.))
                .child(value.into()),
        )
}
