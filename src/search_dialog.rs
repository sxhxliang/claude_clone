use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    Icon, IconName, Sizable as _, WindowExt as _,
    dialog::{DialogHeader, DialogTitle},
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::Scrollbar,
    v_flex,
};

use crate::ClaudeApp;
use crate::models::{ChatRole, Conversation};
use crate::theme::{
    accent, bg_color, border_color, hover_bg, hover_surface, text_2, text_3, text_color,
};

const CONTEXT: &str = "SearchDialog";

#[derive(Action, Clone, PartialEq)]
#[action(namespace = search_dialog, no_json)]
struct SelectPrevResult;

#[derive(Action, Clone, PartialEq)]
#[action(namespace = search_dialog, no_json)]
struct SelectNextResult;

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevResult, Some(CONTEXT)),
        KeyBinding::new("down", SelectNextResult, Some(CONTEXT)),
    ]);
}

struct SearchResult {
    conversation_id: usize,
    title: SharedString,
    subtitle: SharedString,
    snippet: SharedString,
    pinned: bool,
    active: bool,
    score: usize,
}

pub(crate) struct SearchDialog {
    app: WeakEntity<ClaudeApp>,
    input: Entity<InputState>,
    results_scroll_handle: ScrollHandle,
    selected_index: usize,
    _subscriptions: Vec<Subscription>,
}

impl SearchDialog {
    pub(crate) fn new(
        app: WeakEntity<ClaudeApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search conversations..."));
        let subscriptions =
            vec![
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Change => {
                            this.selected_index = 0;
                            this.results_scroll_handle.set_offset(point(px(0.), px(0.)));
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => this.open_selected_result(window, cx),
                        _ => {}
                    }
                }),
            ];

        let input_for_focus = input.clone();
        window.defer(cx, move |window, cx| {
            input_for_focus.update(cx, |input, cx| input.focus(window, cx));
        });

        Self {
            app,
            input,
            results_scroll_handle: ScrollHandle::new(),
            selected_index: 0,
            _subscriptions: subscriptions,
        }
    }

    fn role_label(role: &ChatRole) -> &'static str {
        match role {
            ChatRole::User => "You",
            ChatRole::Ai => "Claude",
        }
    }

    fn title_for(conversation: &Conversation) -> SharedString {
        if conversation.title.is_empty() {
            "Untitled conversation".into()
        } else {
            conversation.title.clone()
        }
    }

    fn preview(text: &str) -> SharedString {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut preview: String = text.chars().take(140).collect();
        if text.chars().count() > 140 {
            preview.push_str("...");
        }
        preview.into()
    }

    fn match_score(
        query: &str,
        title: &str,
        message_match: Option<&crate::models::ChatMessage>,
        pinned: bool,
        active: bool,
    ) -> usize {
        if query.is_empty() {
            return usize::from(active) * 50 + usize::from(pinned) * 25;
        }

        let title = title.to_lowercase();
        let mut score = 0;
        if title == query {
            score += 500;
        } else if title.starts_with(query) {
            score += 350;
        } else if title.contains(query) {
            score += 250;
        }
        if message_match.is_some() {
            score += 100;
        }
        if pinned {
            score += 25;
        }
        if active {
            score += 15;
        }
        score
    }

    fn results(&self, cx: &App) -> Vec<SearchResult> {
        let Some(app) = self.app.upgrade() else {
            return Vec::new();
        };
        let app = app.read(cx);
        let query = self.input.read(cx).value().trim().to_lowercase();

        let mut results = app
            .conversations
            .iter()
            .filter_map(|conversation| {
                let title = Self::title_for(conversation);
                let title_text = title.to_string();
                let title_matches =
                    query.is_empty() || title_text.to_lowercase().contains(query.as_str());

                let message_match = conversation.messages.iter().find(|message| {
                    !query.is_empty() && message.content.to_string().to_lowercase().contains(&query)
                });

                if !title_matches && message_match.is_none() {
                    return None;
                }

                let (subtitle, snippet) = if let Some(message) = message_match {
                    (
                        format!("{} message", Self::role_label(&message.role)).into(),
                        Self::preview(&message.content),
                    )
                } else {
                    (
                        "Conversation title".into(),
                        conversation
                            .messages
                            .first()
                            .map(|message| Self::preview(&message.content))
                            .unwrap_or_else(|| "No messages yet".into()),
                    )
                };

                let active = app.active_conversation_id == Some(conversation.id);
                let score = Self::match_score(
                    &query,
                    &title_text,
                    message_match,
                    conversation.pinned,
                    active,
                );

                Some(SearchResult {
                    conversation_id: conversation.id,
                    title,
                    subtitle,
                    snippet,
                    pinned: conversation.pinned,
                    active,
                    score,
                })
            })
            .collect::<Vec<_>>();

        results.sort_by(|a, b| b.score.cmp(&a.score));
        results
    }

    fn open_result(&self, conversation_id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| {
                app.select_conversation(conversation_id, window, cx);
            });
            window.close_dialog(cx);
        }
    }

    fn open_selected_result(&self, window: &mut Window, cx: &mut Context<Self>) {
        let results = self.results(cx);
        if let Some(result) = results.get(self.selected_index).or_else(|| results.first()) {
            self.open_result(result.conversation_id, window, cx);
        }
    }

    fn select_prev(&mut self, _: &SelectPrevResult, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.results(cx).len();
        if count == 0 {
            return;
        }
        self.selected_index = if self.selected_index == 0 {
            count - 1
        } else {
            self.selected_index - 1
        };
        self.results_scroll_handle
            .scroll_to_item(self.selected_index);
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNextResult, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.results(cx).len();
        if count == 0 {
            return;
        }
        self.selected_index = (self.selected_index + 1) % count;
        self.results_scroll_handle
            .scroll_to_item(self.selected_index);
        cx.notify();
    }

    fn highlighted_text(
        text: SharedString,
        query: &str,
        base_color: Hsla,
        base_size: Pixels,
        bold: bool,
    ) -> AnyElement {
        let raw = text.to_string();
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return div()
                .text_size(base_size)
                .text_color(base_color)
                .when(bold, |this| this.font_weight(FontWeight::MEDIUM))
                .truncate()
                .child(text)
                .into_any_element();
        }

        let lower = raw.to_lowercase();
        let Some(start) = lower.find(&query) else {
            return div()
                .text_size(base_size)
                .text_color(base_color)
                .when(bold, |this| this.font_weight(FontWeight::MEDIUM))
                .truncate()
                .child(text)
                .into_any_element();
        };
        let end = start + query.len();
        if !raw.is_char_boundary(start) || !raw.is_char_boundary(end) {
            return div()
                .text_size(base_size)
                .text_color(base_color)
                .when(bold, |this| this.font_weight(FontWeight::MEDIUM))
                .truncate()
                .child(text)
                .into_any_element();
        }

        h_flex()
            .min_w_0()
            .overflow_hidden()
            .text_size(base_size)
            .text_color(base_color)
            .when(bold, |this| this.font_weight(FontWeight::MEDIUM))
            .child(div().truncate().child(raw[..start].to_string()))
            .child(
                div()
                    .rounded_sm()
                    .px_0p5()
                    .bg(accent().opacity(0.16))
                    .text_color(accent())
                    .child(raw[start..end].to_string()),
            )
            .child(div().truncate().child(raw[end..].to_string()))
            .into_any_element()
    }

    fn render_result(
        &self,
        result: SearchResult,
        selected: bool,
        query: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let conversation_id = result.conversation_id;

        h_flex()
            .id(("search-result", conversation_id))
            .items_start()
            .gap_2p5()
            .px_2p5()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .when(result.active || selected, |this| this.bg(hover_surface()))
            .when(selected, |this| {
                this.border_1().border_color(accent().opacity(0.42))
            })
            .hover(|this| this.bg(hover_bg()))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_result(conversation_id, window, cx);
            }))
            .child(
                div()
                    .mt(px(2.))
                    .size_6()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .border_1()
                    .border_color(border_color())
                    .text_color(text_2())
                    .child(Icon::new(IconName::Inbox).size_3p5()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(div().flex_1().min_w_0().truncate().child(
                                Self::highlighted_text(
                                    result.title,
                                    query,
                                    text_color(),
                                    px(13.5),
                                    true,
                                ),
                            ))
                            .when(result.pinned, |this| {
                                this.child(
                                    Icon::new(IconName::StarFill).size_3().text_color(accent()),
                                )
                            })
                            .when(result.active, |this| {
                                this.child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_full()
                                        .text_size(px(10.))
                                        .bg(accent())
                                        .text_color(bg_color())
                                        .child("Current"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(text_3())
                            .child(result.subtitle),
                    )
                    .child(div().line_height(px(18.)).child(Self::highlighted_text(
                        result.snippet,
                        query,
                        text_2(),
                        px(12.5),
                        false,
                    ))),
            )
            .into_any_element()
    }
}

impl Render for SearchDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.input.read(cx).value().trim().to_string();
        let query_empty = query.is_empty();
        let results = self.results(cx);
        if self.selected_index >= results.len() {
            self.selected_index = 0;
        }

        v_flex()
            .w_full()
            .bg(bg_color())
            .rounded_lg()
            .overflow_hidden()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .child(
                DialogHeader::new()
                    .px_5()
                    .py_4()
                    .border_b_1()
                    .border_color(border_color())
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(DialogTitle::new().child("Search conversations")),
                    ),
            )
            .child(
                v_flex()
                    .p_4()
                    .gap_2p5()
                    .child(
                        h_flex()
                            .h(px(40.))
                            .items_center()
                            .gap_2()
                            .px_3()
                            .rounded_md()
                            .border_1()
                            .border_color(border_color())
                            .bg(hover_surface())
                            .child(Icon::new(IconName::Search).small().text_color(text_3()))
                            .child(
                                div().flex_1().child(
                                    Input::new(&self.input)
                                        .appearance(false)
                                        .bordered(false)
                                        .focus_bordered(false)
                                        .cleanable(true),
                                ),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(11.5))
                            .text_color(text_3())
                            .child(if query_empty {
                                "Recent conversations"
                            } else {
                                "Search results"
                            })
                            .child(SharedString::from(format!("{} found", results.len()))),
                    )
                    .child(
                        div()
                            .relative()
                            .max_h(px(420.))
                            .child(
                                v_flex()
                                    .id("search-results-scroll")
                                    .max_h(px(420.))
                                    .track_scroll(&self.results_scroll_handle)
                                    .overflow_y_scroll()
                                    .gap_0p5()
                                    .when(results.is_empty(), |this| {
                                        this.child(
                                            v_flex()
                                                .items_center()
                                                .justify_center()
                                                .gap_2()
                                                .h(px(180.))
                                                .text_color(text_3())
                                                .child(Icon::new(IconName::Search).size_6())
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .child("No matching conversations"),
                                                ),
                                        )
                                    })
                                    .children(results.into_iter().enumerate().map(
                                        |(ix, result)| {
                                            self.render_result(
                                                result,
                                                ix == self.selected_index,
                                                &query,
                                                cx,
                                            )
                                        },
                                    )),
                            )
                            .child(Scrollbar::vertical(&self.results_scroll_handle)),
                    ),
            )
    }
}
