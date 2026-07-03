//! Model-management UI for the Settings window: a two-pane layout (provider
//! list + provider editor) plus the settings-section enum used by the window's
//! left nav. Provider data lives on `ClaudeApp`; this view owns the add/edit
//! inputs and transient fetch state, dispatching mutations back through the app.
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    Disableable as _, Icon, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonRounded, ButtonVariants as _},
    collapsible::Collapsible,
    h_flex,
    input::{Input, InputEvent, InputState},
    notification::Notification,
    scroll::ScrollableElement,
    switch::Switch,
    v_flex,
};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

use crate::ClaudeApp;
use crate::genai_backend;
use crate::models::{Provider, ProviderKind, ProviderModel};
use crate::theme::{accent, bg_color, sidebar_bg, text_2, text_3, text_color, white_color};

fn panel_bg() -> Hsla {
    hsla(42.0 / 360.0, 0.30, 0.975, 1.0)
}

fn paper_bg() -> Hsla {
    hsla(42.0 / 360.0, 0.26, 0.955, 1.0)
}

fn elevated_bg() -> Hsla {
    hsla(0.0, 0.0, 1.0, 0.94)
}

fn clay() -> Hsla {
    hsla(16.0 / 360.0, 0.46, 0.44, 1.0)
}

fn clay_soft_bg() -> Hsla {
    hsla(20.0 / 360.0, 0.42, 0.91, 1.0)
}

fn clay_hover_bg() -> Hsla {
    hsla(22.0 / 360.0, 0.38, 0.88, 1.0)
}

fn enabled_badge_bg() -> Hsla {
    hsla(150.0 / 360.0, 0.28, 0.91, 1.0)
}

fn enabled_badge_text() -> Hsla {
    hsla(150.0 / 360.0, 0.34, 0.32, 1.0)
}

fn disabled_badge_bg() -> Hsla {
    hsla(36.0 / 360.0, 0.10, 0.90, 1.0)
}

fn disabled_badge_text() -> Hsla {
    hsla(35.0 / 360.0, 0.08, 0.42, 1.0)
}

fn warning_bg() -> Hsla {
    hsla(42.0 / 360.0, 0.55, 0.90, 1.0)
}

fn warning_text() -> Hsla {
    hsla(32.0 / 360.0, 0.55, 0.38, 1.0)
}

fn search_bg() -> Hsla {
    hsla(42.0 / 360.0, 0.22, 0.965, 1.0)
}

fn search_border() -> Hsla {
    hsla(37.0 / 360.0, 0.12, 0.84, 1.0)
}

fn divider() -> Hsla {
    hsla(38.0 / 360.0, 0.12, 0.86, 1.0)
}

fn muted_surface() -> Hsla {
    hsla(40.0 / 360.0, 0.18, 0.935, 1.0)
}

fn field_bg() -> Hsla {
    hsla(40.0 / 360.0, 0.22, 0.972, 1.0)
}

fn selected_row_bg() -> Hsla {
    hsla(30.0 / 360.0, 0.35, 0.90, 1.0)
}

fn row_bg() -> Hsla {
    hsla(42.0 / 360.0, 0.18, 0.945, 1.0)
}

fn row_hover_bg() -> Hsla {
    hsla(36.0 / 360.0, 0.22, 0.925, 1.0)
}

fn selected_row_border() -> Hsla {
    hsla(18.0 / 360.0, 0.32, 0.70, 1.0)
}

fn avatar_color(kind: ProviderKind) -> Hsla {
    match kind {
        ProviderKind::OpenAI | ProviderKind::OpenAIResp | ProviderKind::GithubCopilot => {
            hsla(220.0 / 360.0, 0.78, 0.62, 1.0)
        }
        ProviderKind::Anthropic | ProviderKind::MiniMax => hsla(18.0 / 360.0, 0.80, 0.62, 1.0),
        ProviderKind::Gemini | ProviderKind::Vertex => hsla(162.0 / 360.0, 0.60, 0.52, 1.0),
        ProviderKind::Fireworks | ProviderKind::Together | ProviderKind::Groq => {
            hsla(278.0 / 360.0, 0.52, 0.56, 1.0)
        }
        ProviderKind::Aihubmix | ProviderKind::Mimo | ProviderKind::Moonshot => {
            hsla(204.0 / 360.0, 0.58, 0.52, 1.0)
        }
        ProviderKind::Nebius | ProviderKind::Xai | ProviderKind::DeepSeek => {
            hsla(188.0 / 360.0, 0.48, 0.44, 1.0)
        }
        ProviderKind::Zai | ProviderKind::BigModel | ProviderKind::Aliyun | ProviderKind::Baidu => {
            hsla(34.0 / 360.0, 0.72, 0.54, 1.0)
        }
        ProviderKind::Cohere | ProviderKind::OpenRouter | ProviderKind::OpenCodeGo => {
            hsla(136.0 / 360.0, 0.40, 0.44, 1.0)
        }
        ProviderKind::Ollama | ProviderKind::OllamaCloud | ProviderKind::BedrockApi => {
            hsla(250.0 / 360.0, 0.40, 0.50, 1.0)
        }
    }
}

/// The endpoint path each provider kind appends to its base URL, used only for
/// the preview hint under the API-address field.
fn preview_path(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAI
        | ProviderKind::Fireworks
        | ProviderKind::Together
        | ProviderKind::Groq
        | ProviderKind::Aihubmix
        | ProviderKind::Mimo
        | ProviderKind::Moonshot
        | ProviderKind::Nebius
        | ProviderKind::Xai
        | ProviderKind::DeepSeek
        | ProviderKind::Zai
        | ProviderKind::BigModel
        | ProviderKind::Aliyun
        | ProviderKind::Baidu
        | ProviderKind::GithubCopilot
        | ProviderKind::OpenRouter => "chat/completions",
        ProviderKind::OpenAIResp => "responses",
        ProviderKind::Anthropic | ProviderKind::MiniMax => "messages",
        ProviderKind::Gemini => "models/{model}:generateContent",
        ProviderKind::Cohere => "chat",
        ProviderKind::Ollama | ProviderKind::OllamaCloud => "api/chat",
        ProviderKind::Vertex => "publishers/google/models/{model}:generateContent",
        ProviderKind::OpenCodeGo => "chat/completions | messages",
        ProviderKind::BedrockApi => "model/{model}/converse",
    }
}

fn kind_caption(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAI => "Chat Completions",
        ProviderKind::OpenAIResp => "Responses API",
        ProviderKind::Gemini => "Generative Language",
        ProviderKind::Anthropic => "Messages API",
        ProviderKind::Fireworks
        | ProviderKind::Together
        | ProviderKind::Groq
        | ProviderKind::Aihubmix
        | ProviderKind::Mimo
        | ProviderKind::Moonshot
        | ProviderKind::Nebius
        | ProviderKind::Xai
        | ProviderKind::DeepSeek
        | ProviderKind::Zai
        | ProviderKind::BigModel
        | ProviderKind::Aliyun
        | ProviderKind::OpenRouter => "OpenAI-compatible",
        ProviderKind::Baidu => "Qianfan API",
        ProviderKind::Cohere => "Cohere Chat",
        ProviderKind::Ollama => "Ollama Native",
        ProviderKind::OllamaCloud => "Ollama Cloud",
        ProviderKind::Vertex => "Vertex AI",
        ProviderKind::GithubCopilot => "GitHub Models",
        ProviderKind::OpenCodeGo => "OpenCode Gateway",
        ProviderKind::BedrockApi => "Bedrock Converse",
        ProviderKind::MiniMax => "Anthropic-compatible",
    }
}

/// Compact URL for the provider-row subtitle: drop the scheme and trailing slash.
fn short_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed)
        .to_string()
}

fn compact_error_message(error: &str) -> SharedString {
    const MAX_CHARS: usize = 240;

    let compact = error.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let shortened: String = chars.by_ref().take(MAX_CHARS).collect();

    if chars.next().is_some() {
        format!("{shortened}...").into()
    } else {
        compact.into()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    ModelManagement,
    Mcp,
    Theme,
    General,
}

impl SettingsSection {
    pub(crate) fn id(self) -> &'static str {
        match self {
            SettingsSection::ModelManagement => "models",
            SettingsSection::Mcp => "mcp",
            SettingsSection::Theme => "theme",
            SettingsSection::General => "general",
        }
    }

    pub(crate) fn label(self) -> SharedString {
        match self {
            SettingsSection::ModelManagement => crate::tr!("settings.sections.models"),
            SettingsSection::Mcp => crate::tr!("settings.sections.mcp"),
            SettingsSection::Theme => crate::tr!("settings.sections.theme"),
            SettingsSection::General => crate::tr!("settings.sections.general"),
        }
    }

    pub(crate) fn sublabel(self) -> SharedString {
        match self {
            SettingsSection::ModelManagement => crate::tr!("settings.sections.models_sub"),
            SettingsSection::Mcp => crate::tr!("settings.sections.mcp_sub"),
            SettingsSection::Theme => crate::tr!("settings.sections.theme_sub"),
            SettingsSection::General => crate::tr!("settings.sections.general_sub"),
        }
    }

    pub(crate) fn icon(self) -> IconName {
        match self {
            SettingsSection::ModelManagement => IconName::Settings,
            SettingsSection::Mcp => IconName::Network,
            SettingsSection::Theme => IconName::Palette,
            SettingsSection::General => IconName::Info,
        }
    }

    pub(crate) fn all() -> [SettingsSection; 4] {
        [
            SettingsSection::ModelManagement,
            SettingsSection::Mcp,
            SettingsSection::Theme,
            SettingsSection::General,
        ]
    }
}

struct ModelGroup<'a> {
    key: String,
    models: Vec<&'a ProviderModel>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum ModelSortPart {
    Number(u64),
    Text(String),
}

fn is_version_segment(segment: &str) -> bool {
    let mut has_digit = false;
    let valid = segment.chars().all(|ch| {
        if ch.is_ascii_digit() {
            has_digit = true;
            true
        } else {
            ch.is_ascii_alphabetic() || ch == '.'
        }
    });

    valid && has_digit
}

fn model_group_key(model_id: &str) -> String {
    let parts: Vec<_> = model_id
        .split('-')
        .filter(|part| !part.is_empty())
        .collect();

    if parts.len() <= 1 {
        return model_id.to_string();
    }

    let mut version_end = None;
    for (ix, part) in parts.iter().enumerate().skip(1) {
        if is_version_segment(part) {
            version_end = Some(ix);
        } else {
            break;
        }
    }

    if let Some(ix) = version_end
        && ix + 1 < parts.len()
    {
        return parts[..=ix].join("-");
    }

    parts[..parts.len() - 1].join("-")
}

fn push_model_sort_part(parts: &mut Vec<ModelSortPart>, value: &str, is_number: bool) {
    if is_number {
        parts.push(ModelSortPart::Number(value.parse().unwrap_or(u64::MAX)));
    } else {
        parts.push(ModelSortPart::Text(value.to_string()));
    }
}

fn model_sort_key(value: &str) -> Vec<ModelSortPart> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut digit_run = None;

    for ch in value.chars() {
        let is_digit = ch.is_ascii_digit();
        if let Some(was_digit) = digit_run
            && was_digit != is_digit
            && !buf.is_empty()
        {
            push_model_sort_part(&mut parts, &buf, was_digit);
            buf.clear();
        }

        digit_run = Some(is_digit);
        if is_digit {
            buf.push(ch);
        } else {
            buf.extend(ch.to_lowercase());
        }
    }

    if let Some(is_digit) = digit_run
        && !buf.is_empty()
    {
        push_model_sort_part(&mut parts, &buf, is_digit);
    }

    parts
}

fn compare_model_ids(a: &str, b: &str) -> Ordering {
    model_sort_key(a)
        .cmp(&model_sort_key(b))
        .then_with(|| a.cmp(b))
}

fn build_model_groups(models: &[ProviderModel]) -> Vec<ModelGroup<'_>> {
    let mut grouped: HashMap<String, Vec<&ProviderModel>> = HashMap::new();
    for model in models {
        grouped
            .entry(model_group_key(&model.id))
            .or_default()
            .push(model);
    }

    let mut groups: Vec<_> = grouped
        .into_iter()
        .map(|(key, mut models)| {
            models.sort_by(|a, b| compare_model_ids(&a.id, &b.id));
            ModelGroup { key, models }
        })
        .collect();
    groups.sort_by(|a, b| compare_model_ids(&a.key, &b.key));
    groups
}

pub(crate) struct ProviderSettings {
    app: WeakEntity<ClaudeApp>,
    provider_search_input: Entity<InputState>,
    key_input: Entity<InputState>,
    url_input: Entity<InputState>,
    new_kind: ProviderKind,
    selected_provider_id: Option<usize>,
    /// Which provider the key/url inputs currently mirror. Editing must not be
    /// clobbered, so we only re-sync the inputs when this differs from the
    /// selected provider (i.e. the selection actually changed).
    synced_provider: Option<usize>,
    key_revealed: bool,
    fetching: HashSet<usize>,
    collapsed_model_groups: HashSet<(usize, String)>,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl ProviderSettings {
    pub(crate) fn new(
        app: WeakEntity<ClaudeApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let provider_search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::tr!("provider.search_placeholder"))
        });
        let key_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::tr!("provider.api_key_placeholder"))
                .masked(true)
        });
        let url_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::tr!("provider.api_url_placeholder"))
        });

        // Auto-save: persist the edited credentials back to the selected
        // provider when either field loses focus (no explicit save button).
        let subscriptions = vec![
            cx.subscribe_in(&key_input, window, |this, _, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Blur) {
                    this.commit_editor(cx);
                }
            }),
            cx.subscribe_in(&url_input, window, |this, _, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Blur) {
                    this.commit_editor(cx);
                }
            }),
        ];

        Self {
            app,
            provider_search_input,
            key_input,
            url_input,
            new_kind: ProviderKind::OpenAI,
            selected_provider_id: None,
            synced_provider: None,
            key_revealed: false,
            fetching: HashSet::new(),
            collapsed_model_groups: HashSet::new(),
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn sync_selected_provider(&mut self, providers: &[Provider]) {
        let should_reset = self
            .selected_provider_id
            .is_none_or(|id| !providers.iter().any(|provider| provider.id == id));
        if should_reset {
            self.selected_provider_id = providers.first().map(|provider| provider.id);
        }
    }

    fn sync_editor_from_provider(
        &mut self,
        provider: Option<&Provider>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (kind, key, url) = match provider {
            Some(provider) => (
                provider.kind,
                provider.api_key.clone(),
                provider.base_url.clone(),
            ),
            None => (ProviderKind::OpenAI, String::new(), String::new()),
        };
        self.new_kind = kind;
        self.key_input
            .update(cx, |state, cx| state.set_value(&key, window, cx));
        self.url_input
            .update(cx, |state, cx| state.set_value(&url, window, cx));
        self.error = None;
    }

    fn commit_editor(&mut self, cx: &mut Context<Self>) {
        let Some(provider_id) = self.selected_provider_id else {
            return;
        };
        let key = self.key_input.read(cx).value().trim().to_string();
        let url = self.url_input.read(cx).value().trim().to_string();
        let kind = self.new_kind;
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| {
                app.update_provider(provider_id, kind, key, url, cx);
            });
        }
    }

    fn select_provider(&mut self, provider_id: usize, cx: &mut Context<Self>) {
        if self.selected_provider_id == Some(provider_id) {
            return;
        }

        self.commit_editor(cx);
        self.selected_provider_id = Some(provider_id);
        self.error = None;
        cx.notify();
    }

    fn add_provider(&mut self, cx: &mut Context<Self>) {
        self.commit_editor(cx);
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let kind = self.new_kind;
        let id = app.update(cx, |app, cx| {
            app.add_provider(kind, String::new(), String::new(), cx)
        });
        self.selected_provider_id = Some(id);
        self.error = None;
        cx.notify();
    }

    fn delete_selected_now(&mut self, provider_id: usize, cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| app.delete_provider(provider_id, cx));
        }
        if self.selected_provider_id == Some(provider_id) {
            self.selected_provider_id = None;
        }
        cx.notify();
    }

    fn toggle_reveal_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.key_revealed = !self.key_revealed;
        let masked = !self.key_revealed;
        self.key_input
            .update(cx, |state, cx| state.set_masked(masked, window, cx));
        cx.notify();
    }

    fn model_group_collapsed(&self, provider_id: usize, group_key: &str) -> bool {
        self.collapsed_model_groups
            .contains(&(provider_id, group_key.to_string()))
    }

    fn toggle_model_group(
        &mut self,
        provider_id: usize,
        group_key: String,
        cx: &mut Context<Self>,
    ) {
        let key = (provider_id, group_key);
        if !self.collapsed_model_groups.insert(key.clone()) {
            self.collapsed_model_groups.remove(&key);
        }
        cx.notify();
    }

    fn fetch_models(&mut self, provider_id: usize, window: &mut Window, cx: &mut Context<Self>) {
        // Persist any pending edits first, so the fetch uses the latest creds.
        self.commit_editor(cx);
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Some((kind, base_url, key)) = app
            .read(cx)
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .map(|provider| {
                (
                    provider.kind,
                    provider.effective_base_url(),
                    provider.api_key.clone(),
                )
            })
        else {
            return;
        };
        self.fetching.insert(provider_id);
        self.error = None;
        cx.notify();

        let rx = genai_backend::list_models(kind, base_url, key);
        cx.spawn_in(window, async move |this, cx| {
            let result = rx.await;
            _ = this.update(cx, |this, cx| {
                this.fetching.remove(&provider_id);
                match result {
                    Ok(Ok(ids)) => {
                        if let Some(app) = this.app.upgrade() {
                            app.update(cx, |app, cx| app.set_provider_models(provider_id, ids, cx));
                        }
                    }
                    Ok(Err(err)) => this.error = Some(err.into()),
                    Err(_) => this.error = Some(crate::tr!("provider.fetch_cancelled")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_search(&self) -> impl IntoElement {
        div()
            .h(px(42.))
            .px_3()
            .rounded(px(12.))
            .border_1()
            .border_color(search_border())
            .bg(search_bg())
            .flex()
            .items_center()
            .child(
                Input::new(&self.provider_search_input)
                    .appearance(false)
                    .bordered(false)
                    .w_full()
                    .prefix(Icon::new(IconName::Search).small().text_color(text_3()))
                    .suffix(Icon::new(IconName::Settings2).small().text_color(text_3())),
            )
    }

    fn render_avatar(&self, kind: ProviderKind, size: f32) -> impl IntoElement {
        div()
            .size(px(size))
            .rounded(px(size * 0.30))
            .bg(avatar_color(kind))
            .border_1()
            .border_color(white_color().opacity(0.54))
            .text_color(white_color())
            .text_size(px(size * 0.40))
            .font_weight(FontWeight::BOLD)
            .flex()
            .items_center()
            .justify_center()
            .child(kind.label().chars().next().unwrap_or('P').to_string())
    }

    fn render_status_dot(&self, enabled: bool) -> impl IntoElement {
        div().size(px(7.)).rounded_full().bg(if enabled {
            enabled_badge_text()
        } else {
            disabled_badge_text()
        })
    }

    fn render_status_badge(&self, enabled: bool) -> impl IntoElement {
        div()
            .px_2()
            .h(px(20.))
            .rounded_full()
            .bg(if enabled {
                enabled_badge_bg()
            } else {
                disabled_badge_bg()
            })
            .text_color(if enabled {
                enabled_badge_text()
            } else {
                disabled_badge_text()
            })
            .text_size(px(10.5))
            .font_weight(FontWeight::BOLD)
            .flex()
            .items_center()
            .gap_1()
            .child(self.render_status_dot(enabled))
            .child(if enabled {
                crate::tr!("provider.enabled_badge")
            } else {
                crate::tr!("provider.paused_badge")
            })
    }

    fn render_count_pill(&self, count: usize) -> impl IntoElement {
        div()
            .px_2()
            .h(px(20.))
            .rounded_full()
            .bg(muted_surface())
            .text_color(text_2())
            .text_size(px(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .flex()
            .items_center()
            .child(count.to_string())
    }

    fn render_badge(
        &self,
        label: impl Into<SharedString>,
        bg: Hsla,
        color: Hsla,
    ) -> impl IntoElement {
        div()
            .px_2()
            .h(px(20.))
            .rounded_full()
            .bg(bg)
            .text_color(color)
            .text_size(px(10.5))
            .font_weight(FontWeight::SEMIBOLD)
            .flex()
            .items_center()
            .child(label.into())
    }

    fn render_provider_row(
        &self,
        provider: &Provider,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let provider_id = provider.id;
        let name = provider.kind.label();
        let host = short_url(&provider.effective_base_url());
        let enabled = provider.enabled;
        let missing_key = provider.api_key.trim().is_empty();

        h_flex()
            .id(("provider-row", provider_id))
            .w_full()
            .h(px(68.))
            .px_3()
            .rounded(px(14.))
            .border_1()
            .border_color(if selected {
                selected_row_border()
            } else {
                divider()
            })
            .items_center()
            .gap_3()
            .cursor_pointer()
            .bg(if selected {
                selected_row_bg()
            } else {
                row_bg()
            })
            .hover(|this| {
                this.bg(if selected {
                    clay_hover_bg()
                } else {
                    row_hover_bg()
                })
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_provider(provider_id, cx);
            }))
            .child(self.render_avatar(provider.kind, 34.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .child(self.render_status_dot(enabled))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(text_color())
                                    .child(name),
                            )
                            .child(
                                div()
                                    .px_1p5()
                                    .h(px(18.))
                                    .rounded_full()
                                    .bg(field_bg())
                                    .text_color(text_3())
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .flex()
                                    .items_center()
                                    .child(kind_caption(provider.kind)),
                            ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(text_3())
                            .child(host),
                    ),
            )
            .when(missing_key, |this| {
                this.child(
                    Icon::new(IconName::TriangleAlert)
                        .xsmall()
                        .text_color(warning_text()),
                )
            })
    }

    fn render_provider_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let providers = match self.app.upgrade() {
            Some(app) => app.read(cx).providers.clone(),
            None => Vec::new(),
        };
        let query = self
            .provider_search_input
            .read(cx)
            .value()
            .trim()
            .to_lowercase();

        let mut list = v_flex().gap_1p5().flex_1().min_h_0().overflow_y_scrollbar();
        let filtered: Vec<_> = providers
            .iter()
            .filter(|provider| {
                if query.is_empty() {
                    return true;
                }

                provider.kind.label().to_lowercase().contains(&query)
                    || kind_caption(provider.kind).to_lowercase().contains(&query)
                    || short_url(&provider.effective_base_url())
                        .to_lowercase()
                        .contains(&query)
                    || provider
                        .models
                        .iter()
                        .any(|model| model.id.to_lowercase().contains(&query))
            })
            .cloned()
            .collect();

        if filtered.is_empty() {
            list = list.child(
                div()
                    .mt_2()
                    .text_size(px(12.5))
                    .text_color(text_2())
                    .child(crate::tr!("provider.no_matches")),
            );
        } else {
            for provider in filtered {
                let selected = self.selected_provider_id == Some(provider.id);
                list = list.child(self.render_provider_row(&provider, selected, cx));
            }
        }

        v_flex()
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(text_color())
                                    .child(crate::tr!("provider.title")),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(text_3())
                                    .child(crate::tr!("provider.subtitle")),
                            ),
                    )
                    .child(self.render_count_pill(providers.len())),
            )
            .child(self.render_search())
            .child(list)
            .child(
                Button::new("provider-add-quick")
                    .primary()
                    .rounded(ButtonRounded::Large)
                    .icon(IconName::Plus)
                    .label(crate::tr!("provider.add"))
                    .on_click(cx.listener(|this, _, _, cx| this.add_provider(cx))),
            )
    }

    fn render_provider_kind_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = h_flex()
            .w_full()
            .gap_1()
            .flex_wrap()
            .items_center()
            .content_start();

        for kind in ProviderKind::all().iter().copied() {
            let selected = self.new_kind == kind;
            let button = Button::new(SharedString::from(format!(
                "provider-kind-{}",
                kind.label()
            )))
            .xsmall()
            .min_w(px(88.))
            .rounded(ButtonRounded::Large)
            .label(kind.label())
            .tooltip(kind_caption(kind))
            .when(selected, |this| this.primary())
            .when(!selected, |this| this.ghost());
            row = row.child(button.on_click(cx.listener(move |this, _, _, cx| {
                this.new_kind = kind;
                this.commit_editor(cx);
                this.error = None;
                cx.notify();
            })));
        }

        div()
            .w_full()
            .min_w_0()
            .h(px(78.))
            .p_1()
            .rounded(px(14.))
            .bg(muted_surface())
            .overflow_y_scrollbar()
            .child(row)
    }

    fn render_model_row(
        &self,
        provider_id: usize,
        model_id: &str,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let model_key = model_id.to_string();
        let row_id = SharedString::from(format!("model-row-{provider_id}-{model_id}"));
        let click_model_key = model_key.clone();

        h_flex()
            .id(row_id)
            .w_full()
            .h(px(34.))
            .pl_1()
            .pr_2()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(divider().opacity(0.55))
            .cursor_pointer()
            .hover(|this| this.bg(field_bg()))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(app) = this.app.upgrade() {
                    app.update(cx, |app, cx| {
                        app.toggle_model_selected(provider_id, &click_model_key, cx);
                    });
                }
            }))
            .child(
                div()
                    .w(px(20.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |this| {
                        this.child(Icon::new(IconName::Check).xsmall().text_color(clay()))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.8))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text_color())
                    .child(model_key),
            )
            .child(
                div()
                    .w(px(54.))
                    .flex_shrink_0()
                    .text_color(if selected { clay() } else { text_3() })
                    .text_size(px(10.5))
                    .font_weight(FontWeight::MEDIUM)
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(if selected {
                        crate::tr!("provider.model_enabled")
                    } else {
                        crate::tr!("provider.model_disabled")
                    }),
            )
    }

    fn render_model_group_header(
        &self,
        provider_id: usize,
        group_key: String,
        model_count: usize,
        selected_count: usize,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let header_id = SharedString::from(format!("model-group-{provider_id}-{group_key}"));
        let click_group_key = group_key.clone();
        let has_selected = selected_count > 0;

        h_flex()
            .id(header_id)
            .w_full()
            .h(px(32.))
            .px_2()
            .gap_2()
            .items_center()
            .rounded(px(8.))
            .bg(muted_surface())
            .cursor_pointer()
            .hover(|this| this.bg(row_hover_bg()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_model_group(provider_id, click_group_key.clone(), cx);
            }))
            .child(
                div()
                    .w(px(18.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .xsmall()
                        .text_color(text_2()),
                    ),
            )
            .child(Icon::new(IconName::Folder).xsmall().text_color(text_2()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::BOLD)
                    .text_color(text_color())
                    .child(group_key),
            )
            .child(
                div()
                    .w(px(46.))
                    .flex_shrink_0()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if has_selected { clay() } else { text_3() })
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(format!("{selected_count}/{model_count}")),
            )
    }

    fn render_model_group(
        &self,
        provider_id: usize,
        group: ModelGroup<'_>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let collapsed = self.model_group_collapsed(provider_id, &group.key);
        let selected_count = group.models.iter().filter(|model| model.selected).count();
        let model_count = group.models.len();

        let mut group_content = v_flex()
            .ml_4()
            .pl_3()
            .pr_1()
            .border_l_1()
            .border_color(divider());
        for model in group.models {
            group_content = group_content.child(self.render_model_row(
                provider_id,
                &model.id,
                model.selected,
                cx,
            ));
        }

        Collapsible::new()
            .w_full()
            .gap_1()
            .open(!collapsed)
            .child(self.render_model_group_header(
                provider_id,
                group.key.clone(),
                model_count,
                selected_count,
                collapsed,
                cx,
            ))
            .content(group_content)
    }

    fn render_field_label(
        &self,
        label: impl Into<SharedString>,
        hint: Option<SharedString>,
    ) -> impl IntoElement {
        let label = label.into();
        h_flex()
            .items_center()
            .gap_1p5()
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text_color())
                    .child(label),
            )
            .when_some(hint, |this, hint| {
                this.child(Icon::new(IconName::Info).xsmall().text_color(text_3()))
                    .child(div().text_size(px(11.)).text_color(text_3()).child(hint))
            })
    }

    fn render_input_box(
        &self,
        input: &Entity<InputState>,
        suffix: Option<AnyElement>,
    ) -> impl IntoElement {
        div()
            .h(px(48.))
            .px_3()
            .rounded(px(14.))
            .border_1()
            .border_color(divider())
            .bg(elevated_bg())
            .hover(|this| this.border_color(search_border()))
            .flex()
            .items_center()
            .child(
                Input::new(input)
                    .appearance(false)
                    .bordered(false)
                    .w_full()
                    .when_some(suffix, |this, suffix| this.suffix(suffix)),
            )
    }

    fn render_editor(
        &self,
        provider: Option<Provider>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let provider_id = provider.as_ref().map(|provider| provider.id);
        let header_title = provider
            .as_ref()
            .map(|provider| provider.kind.label().to_string())
            .unwrap_or_else(|| crate::tr!("provider.new_provider").to_string());
        let enabled = provider.as_ref().map(|provider| provider.enabled);
        let model_count = provider
            .as_ref()
            .map_or(0, |provider| provider.models.len());
        let selected_model_count = provider.as_ref().map_or(0, |provider| {
            provider
                .models
                .iter()
                .filter(|model| model.selected)
                .count()
        });
        let is_fetching = provider_id.is_some_and(|id| self.fetching.contains(&id));
        let key_value = self.key_input.read(cx).value().trim().to_string();
        let url_value = self.url_input.read(cx).value().trim().to_string();
        let key_empty = key_value.is_empty();
        let key_dirty = provider
            .as_ref()
            .is_some_and(|provider| provider.api_key.trim() != key_value);
        let url_dirty = provider
            .as_ref()
            .is_some_and(|provider| provider.base_url.trim() != url_value);
        let header_subtitle = provider
            .as_ref()
            .map(|provider| {
                format!(
                    "{} · {}",
                    kind_caption(provider.kind),
                    short_url(&provider.effective_base_url())
                )
            })
            .unwrap_or_else(|| {
                crate::tr!("provider.create_provider", provider = self.new_kind.label()).to_string()
            });

        // -- API address preview (live, from the input or the kind default).
        let raw_url = self.url_input.read(cx).value().trim().to_string();
        let using_default_url = raw_url.is_empty();
        let mut base = if raw_url.is_empty() {
            self.new_kind.default_base_url()
        } else {
            raw_url
        };
        if !base.ends_with('/') {
            base.push('/');
        }
        let preview_endpoint = format!("{base}{}", preview_path(self.new_kind));
        let preview = crate::tr!("provider.preview", endpoint = preview_endpoint.clone());

        // -- Model list.
        let mut models_list = v_flex().w_full().gap_1p5().pr_2();
        match provider.as_ref() {
            Some(provider) if self.fetching.contains(&provider.id) => {
                models_list =
                    models_list.child(self.render_hint(crate::tr!("provider.syncing_models")));
            }
            Some(provider) if provider.models.is_empty() => {
                models_list =
                    models_list.child(self.render_hint(crate::tr!("provider.no_models_synced")));
            }
            Some(provider) => {
                for group in build_model_groups(&provider.models) {
                    models_list =
                        models_list.child(self.render_model_group(provider.id, group, cx));
                }
            }
            None => {
                models_list =
                    models_list.child(self.render_hint(crate::tr!("provider.select_or_create")));
            }
        }

        // -- Key field suffix: reveal toggle.
        let key_suffix = Some(
            Button::new("provider-key-eye")
                .ghost()
                .small()
                .icon(if self.key_revealed {
                    IconName::EyeOff
                } else {
                    IconName::Eye
                })
                .tooltip(if self.key_revealed {
                    crate::tr!("provider.hide_key")
                } else {
                    crate::tr!("provider.show_key")
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_reveal_key(window, cx);
                }))
                .into_any_element(),
        );

        v_flex()
            .size_full()
            .gap_4()
            .bg(panel_bg())
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .min_w_0()
                            .child(self.render_avatar(self.new_kind, 42.))
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(23.))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(text_color())
                                                    .child(header_title),
                                            )
                                            .when_some(enabled, |this, enabled| {
                                                this.child(self.render_status_badge(enabled))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(12.5))
                                            .text_color(text_3())
                                            .child(header_subtitle),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .when(provider_id.is_none(), |this| {
                                this.child(
                                    Button::new("provider-create-selected")
                                        .primary()
                                        .small()
                                        .rounded(ButtonRounded::Large)
                                        .icon(IconName::Plus)
                                        .label(crate::tr!(
                                            "provider.create_provider_short",
                                            provider = self.new_kind.label()
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.add_provider(cx);
                                        })),
                                )
                            })
                            .when_some(provider_id, |this, provider_id| {
                                this.child(
                                    Button::new(("provider-delete", provider_id))
                                        .ghost()
                                        .small()
                                        .icon(IconName::CircleX)
                                        .tooltip(crate::tr!("provider.delete_tooltip"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.delete_selected_now(provider_id, cx);
                                        })),
                                )
                            })
                            .when_some(provider_id.zip(enabled), |this, (provider_id, enabled)| {
                                let app = self.app.clone();
                                this.child(
                                    Switch::new(("provider-enabled", provider_id))
                                        .checked(enabled)
                                        .color(clay())
                                        .tooltip(if enabled {
                                            crate::tr!("provider.pause_tooltip")
                                        } else {
                                            crate::tr!("provider.enable_tooltip")
                                        })
                                        .on_click(move |checked, _, cx| {
                                            if let Some(app) = app.upgrade() {
                                                app.update(cx, |app, cx| {
                                                    app.set_provider_enabled(
                                                        provider_id,
                                                        *checked,
                                                        cx,
                                                    );
                                                });
                                            }
                                        }),
                                )
                            }),
                    ),
            )
            .child(
                v_flex()
                    .gap_4()
                    .p_4()
                    .rounded(px(18.))
                    .border_1()
                    .border_color(divider())
                    .bg(paper_bg())
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .child(self.render_field_label(
                                        crate::tr!("provider.platform_type"),
                                        None,
                                    ))
                                    .child(self.render_badge(
                                        kind_caption(self.new_kind),
                                        clay_soft_bg(),
                                        clay(),
                                    )),
                            )
                            .child(self.render_provider_kind_tabs(cx)),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        self.render_field_label(
                                            crate::tr!("provider.api_key"),
                                            None,
                                        ),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_1p5()
                                            .items_center()
                                            .when(key_empty, |this| {
                                                this.child(self.render_badge(
                                                    crate::tr!("provider.missing"),
                                                    warning_bg(),
                                                    warning_text(),
                                                ))
                                            })
                                            .when(key_dirty, |this| {
                                                this.child(self.render_badge(
                                                    crate::tr!("provider.unsaved"),
                                                    warning_bg(),
                                                    warning_text(),
                                                ))
                                            }),
                                    ),
                            )
                            .child(self.render_input_box(&self.key_input, key_suffix))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(text_3())
                                    .child(crate::tr!("provider.multiple_keys")),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(self.render_field_label(
                                                crate::tr!("provider.api_url"),
                                                None,
                                            ))
                                            .when_some(provider_id, |this, provider_id| {
                                                this.child(
                                                    Button::new("provider-key-test")
                                                        .ghost()
                                                        .small()
                                                        .icon(IconName::LoaderCircle)
                                                        .label(if is_fetching {
                                                            crate::tr!("provider.syncing")
                                                        } else {
                                                            crate::tr!("provider.test")
                                                        })
                                                        .loading(is_fetching)
                                                        .disabled(is_fetching)
                                                        .tooltip(crate::tr!(
                                                            "provider.sync_current"
                                                        ))
                                                        .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                this.fetch_models(
                                                                    provider_id,
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                )
                                            }),
                                    )
                                    .when(using_default_url, |this| {
                                        this.child(self.render_badge(
                                            crate::tr!("provider.default_url"),
                                            muted_surface(),
                                            text_2(),
                                        ))
                                    })
                                    .when(url_dirty, |this| {
                                        this.child(self.render_badge(
                                            crate::tr!("provider.unsaved"),
                                            warning_bg(),
                                            warning_text(),
                                        ))
                                    }),
                            )
                            .child(self.render_input_box(&self.url_input, None))
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .text_size(px(11.))
                                    .text_color(text_3())
                                    .child(Icon::new(IconName::Globe).xsmall())
                                    .child(div().flex_1().min_w_0().truncate().child(preview))
                                    .child(
                                        Button::new("provider-copy-preview-url")
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Copy)
                                            .tooltip(crate::tr!("provider.copy_preview_url"))
                                            .on_click({
                                                let preview_endpoint = preview_endpoint.clone();
                                                move |_, window, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(
                                                            preview_endpoint.clone(),
                                                        ),
                                                    );
                                                    window.push_notification(
                                                        Notification::info(crate::tr!(
                                                            "provider.api_url_copied"
                                                        )),
                                                        cx,
                                                    );
                                                }
                                            }),
                                    ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .p_4()
                    .rounded(px(18.))
                    .border_1()
                    .border_color(divider())
                    .bg(elevated_bg())
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(text_color())
                                            .child(crate::tr!("provider.models")),
                                    )
                                    .child(self.render_badge(
                                        crate::tr!(
                                            "provider.models_available",
                                            selected = selected_model_count,
                                            total = model_count
                                        ),
                                        muted_surface(),
                                        text_2(),
                                    )),
                            )
                            .when_some(provider_id, |this, provider_id| {
                                this.child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .when(model_count > 0, |this| {
                                            let app_enable = self.app.clone();
                                            let app_disable = self.app.clone();
                                            this.child(
                                                Button::new(("provider-enable-all", provider_id))
                                                    .ghost()
                                                    .small()
                                                    .rounded(ButtonRounded::Large)
                                                    .label(crate::tr!("provider.enable_all"))
                                                    .on_click(move |_, window, cx| {
                                                        if let Some(app) = app_enable.upgrade() {
                                                            app.update(cx, |app, cx| {
                                                                app.set_provider_models_selected(
                                                                    provider_id,
                                                                    true,
                                                                    cx,
                                                                );
                                                            });
                                                            window.push_notification(
                                                                Notification::info(crate::tr!(
                                                                    "provider.all_enabled"
                                                                )),
                                                                cx,
                                                            );
                                                        }
                                                    }),
                                            )
                                            .child(
                                                Button::new(("provider-disable-all", provider_id))
                                                    .ghost()
                                                    .small()
                                                    .rounded(ButtonRounded::Large)
                                                    .label(crate::tr!("provider.disable_all"))
                                                    .on_click(move |_, window, cx| {
                                                        if let Some(app) = app_disable.upgrade() {
                                                            app.update(cx, |app, cx| {
                                                                app.set_provider_models_selected(
                                                                    provider_id,
                                                                    false,
                                                                    cx,
                                                                );
                                                            });
                                                            window.push_notification(
                                                                Notification::info(crate::tr!(
                                                                    "provider.all_disabled"
                                                                )),
                                                                cx,
                                                            );
                                                        }
                                                    }),
                                            )
                                        })
                                        .child(
                                            Button::new(("provider-refresh", provider_id))
                                                .outline()
                                                .small()
                                                .rounded(ButtonRounded::Large)
                                                .icon(IconName::LoaderCircle)
                                                .label(if is_fetching {
                                                    crate::tr!("provider.syncing")
                                                } else {
                                                    crate::tr!("provider.sync_models")
                                                })
                                                .loading(is_fetching)
                                                .disabled(is_fetching)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.fetch_models(provider_id, window, cx);
                                                    },
                                                )),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div().flex_1().min_h_0().child(
                            div()
                                .id(("provider-models-scroll", provider_id.unwrap_or(usize::MAX)))
                                .size_full()
                                .overflow_y_scrollbar()
                                .child(models_list),
                        ),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    h_flex()
                        .rounded(px(14.))
                        .bg(accent().opacity(0.08))
                        .border_1()
                        .border_color(accent().opacity(0.28))
                        .px_3()
                        .py_2()
                        .gap_2()
                        .items_start()
                        .text_size(px(12.))
                        .text_color(accent())
                        .child(Icon::new(IconName::TriangleAlert).small().flex_shrink_0())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .max_h(px(48.))
                                .overflow_y_scrollbar()
                                .whitespace_normal()
                                .child(compact_error_message(error.as_ref())),
                        ),
                )
            })
    }

    fn render_hint(&self, text: impl Into<SharedString>) -> impl IntoElement {
        div()
            .rounded(px(12.))
            .border_1()
            .border_color(divider())
            .bg(field_bg())
            .px_4()
            .py_3()
            .text_size(px(12.))
            .text_color(text_2())
            .child(text.into())
    }

    pub(crate) fn render_management(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let providers = match self.app.upgrade() {
            Some(app) => app.read(cx).providers.clone(),
            None => Vec::new(),
        };
        self.sync_selected_provider(&providers);

        // Re-sync the editor inputs only when the selected provider changed,
        // never while the user is typing (which would wipe their edits).
        if self.selected_provider_id != self.synced_provider {
            let provider = self
                .selected_provider_id
                .and_then(|id| providers.iter().find(|provider| provider.id == id));
            self.sync_editor_from_provider(provider, window, cx);
            self.synced_provider = self.selected_provider_id;
        }

        let selected_provider = self.selected_provider_id.and_then(|selected_id| {
            providers
                .iter()
                .find(|provider| provider.id == selected_id)
                .cloned()
        });

        h_flex()
            .size_full()
            .bg(bg_color())
            .child(
                div()
                    .w(px(300.))
                    .h_full()
                    .px_3p5()
                    .py_5()
                    .border_r_1()
                    .border_color(divider())
                    .bg(sidebar_bg())
                    .child(self.render_provider_list(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .px_6()
                    .py_5()
                    .bg(panel_bg())
                    .child(self.render_editor(selected_provider, cx)),
            )
    }

    pub(crate) fn render_theme_stub(&self) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(panel_bg())
            .child(
                div()
                    .text_size(px(28.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(text_color())
                    .child(crate::tr!("provider.theme_title")),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(text_3())
                    .child(crate::tr!("provider.theme_stub")),
            )
    }
}

impl Render for ProviderSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_management(window, cx)
    }
}
