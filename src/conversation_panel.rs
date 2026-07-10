//! The conversation panel: a dock `Panel` that owns the live state of one
//! conversation (messages, expansion flags, composer input) and renders the
//! chat transcript via `chat_view`. It is the single editor of a conversation;
//! it syncs its state back into the app's persisted `Conversation` record.
use futures::StreamExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    Disableable as _, Icon, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dock::{Panel, PanelEvent, PanelInfo, PanelState, TabPanel},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{PopupMenu, PopupMenuItem},
    notification::Notification,
    popover::Popover,
    scroll::{ScrollableElement as _, Scrollbar},
    text::{TextView, TextViewState},
    v_flex,
};
use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};
use std::time::Instant;

use crate::chat_view::{
    self, ArtifactHighlightTarget, ChatViewState, GeneratedImage, ImageAttachment, MessageBlock,
    ToolCall, ToolStep,
};
use crate::document_parser;
use crate::genai_backend::{
    self, ChatImage, ChatTurn, GenerationCancel, ImageGenerationResult, StreamMsg,
};
use crate::menus::{add_menu_content, mcp_menu_content, model_menu_content};
use crate::mock_backend;
use crate::models::{
    BranchOrigin, ChatMessage, ChatMode, ChatRole, Conversation, ConversationPanelLayout,
    TokenUsageStats, current_time_ms,
};
use crate::theme::{
    accent, bg_color, border_color, green, pill_hover_bg, setup_row_hover_bg, text_2, text_3,
    text_color, white_color,
};
use crate::voice_input::{VoiceEvent, VoiceRecorder};
use crate::{ClaudeApp, ConversationTabCloseScope};

pub(crate) const PANEL_NAME: &str = "ClaudeConversationPanel";

#[derive(Default)]
struct StreamingTokenCounter {
    started_at: Option<Instant>,
    output: String,
    provider_output_tokens: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum VoiceInputState {
    #[default]
    Idle,
    Starting,
    Recording,
    Transcribing,
}

impl StreamingTokenCounter {
    fn record_delta(&mut self, delta: &str) -> Option<TokenUsageStats> {
        if delta.is_empty() {
            return self.stats();
        }

        self.started_at.get_or_insert_with(Instant::now);
        self.output.push_str(delta);
        self.stats()
    }

    fn record_usage(&mut self, output_tokens: usize) -> Option<TokenUsageStats> {
        self.provider_output_tokens = Some(output_tokens);
        self.stats()
    }

    fn stats(&self) -> Option<TokenUsageStats> {
        let started_at = self.started_at?;
        let output_tokens = self
            .provider_output_tokens
            .unwrap_or_else(|| estimate_output_tokens(&self.output));
        if output_tokens == 0 {
            return None;
        }

        let elapsed_secs = started_at.elapsed().as_secs_f64().max(0.1);
        Some(TokenUsageStats {
            output_tokens,
            tokens_per_second: output_tokens as f64 / elapsed_secs,
        })
    }
}

fn estimate_output_tokens(text: &str) -> usize {
    fn flush_ascii_run(output_tokens: &mut usize, ascii_chars: &mut usize) {
        if *ascii_chars > 0 {
            *output_tokens += ascii_chars.div_ceil(4).max(1);
            *ascii_chars = 0;
        }
    }

    let mut output_tokens = 0;
    let mut ascii_chars = 0;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch.is_ascii_whitespace() {
            ascii_chars += 1;
        } else if ch.is_ascii_punctuation() {
            flush_ascii_run(&mut output_tokens, &mut ascii_chars);
            output_tokens += 1;
        } else if ch.is_whitespace() {
            flush_ascii_run(&mut output_tokens, &mut ascii_chars);
        } else {
            flush_ascii_run(&mut output_tokens, &mut ascii_chars);
            output_tokens += 1;
        }
    }
    flush_ascii_run(&mut output_tokens, &mut ascii_chars);

    output_tokens
}

pub(crate) struct ConversationPanel {
    focus_handle: FocusHandle,
    chat_scroll_handle: ScrollHandle,
    message_scroll_anchors: Vec<ScrollAnchor>,
    app: WeakEntity<ClaudeApp>,
    pub(crate) tab_panel: Option<WeakEntity<TabPanel>>,
    pub(crate) id: usize,
    title: SharedString,
    pinned: bool,
    pending: bool,
    generation_task: Option<Task<()>>,
    generation_cancel: Option<GenerationCancel>,
    voice_state: VoiceInputState,
    voice_recorder: Option<VoiceRecorder>,
    voice_task: Option<Task<()>>,
    voice_base_input: String,
    voice_committed_text: String,
    project_id: Option<usize>,
    branch_origin: Option<BranchOrigin>,
    mode: ChatMode,
    messages: Vec<ChatMessage>,
    pending_images: Vec<ImageAttachment>,
    editing_user_ix: Option<usize>,
    highlighted_artifact_target: Option<ArtifactHighlightTarget>,
    cowork_user_expanded: Vec<bool>,
    tool_expanded: HashMap<(usize, usize), bool>,
    input: Entity<InputState>,
    setup_done: [bool; 3],
    _subscriptions: Vec<Subscription>,
}

impl ConversationPanel {
    pub(crate) fn new(
        mut conversation: Conversation,
        app: WeakEntity<ClaudeApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 6)
                .placeholder(crate::tr!("chat.home_placeholder"))
        });
        let subscriptions = vec![
            cx.subscribe_in(&input, window, {
                move |this, _, ev: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { shift, .. } = ev {
                        if !*shift {
                            let value = this.input.read(cx).value().to_string();
                            this.send(value, window, cx);
                        }
                    }
                }
            }),
            cx.on_focus_in(&focus_handle, window, |this, window, cx| {
                this.activate_in_app(window, cx);
            }),
        ];
        let mode = conversation
            .messages
            .iter()
            .rev()
            .find(|message| message.role == ChatRole::User)
            .map(|message| message.mode)
            .unwrap_or(ChatMode::Chat);
        Self::hydrate_chat_message_blocks(&mut conversation.messages, cx);
        let chat_scroll_handle = ScrollHandle::new();
        let message_scroll_anchors = (0..conversation.messages.len())
            .map(|_| ScrollAnchor::for_handle(chat_scroll_handle.clone()))
            .collect();

        Self {
            focus_handle,
            chat_scroll_handle,
            message_scroll_anchors,
            app,
            tab_panel: None,
            id: conversation.id,
            title: conversation.title,
            pinned: conversation.pinned,
            pending: conversation.pending,
            generation_task: None,
            generation_cancel: None,
            voice_state: VoiceInputState::Idle,
            voice_recorder: None,
            voice_task: None,
            voice_base_input: String::new(),
            voice_committed_text: String::new(),
            project_id: conversation.project_id,
            branch_origin: conversation.branch_origin,
            mode,
            messages: conversation.messages,
            pending_images: Vec::new(),
            editing_user_ix: None,
            highlighted_artifact_target: None,
            cowork_user_expanded: conversation.cowork_user_expanded,
            tool_expanded: conversation.tool_expanded,
            input,
            setup_done: [false; 3],
            _subscriptions: subscriptions,
        }
    }

    fn hydrate_chat_message_blocks(messages: &mut [ChatMessage], cx: &mut Context<Self>) {
        for message in messages {
            if message.role == ChatRole::Ai
                && message.mode == ChatMode::Chat
                && message.blocks.is_none()
            {
                let mut blocks = Vec::new();
                if !message.thinking.is_empty() {
                    blocks.push(MessageBlock::Thinking(chat_view::ThinkingBlock {
                        state: cx.new(|cx| TextViewState::markdown(&message.thinking, cx)),
                        done: true,
                        expanded: false,
                    }));
                }
                if !message.content.is_empty() {
                    blocks.push(MessageBlock::Markdown(
                        cx.new(|cx| TextViewState::markdown(&message.content, cx)),
                    ));
                }
                if !blocks.is_empty() {
                    message.blocks = Some(blocks);
                }
            }
        }
    }

    fn activate_in_app(&self, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = self.snapshot();
        let tab_panel = self.tab_panel.clone();
        if let Some(app) = self.app.upgrade() {
            window.defer(cx, move |_, cx| {
                app.update(cx, |app, cx| {
                    app.activate_conversation_panel(&snapshot, tab_panel, cx);
                });
            });
        }
    }

    fn title_or_untitled(&self) -> SharedString {
        if self.title.is_empty() {
            crate::tr!("conversation.untitled")
        } else {
            self.title.clone()
        }
    }

    fn ensure_message_scroll_anchors(&mut self) {
        let target_len = self.messages.len();
        if self.message_scroll_anchors.len() > target_len {
            self.message_scroll_anchors.truncate(target_len);
        }
        while self.message_scroll_anchors.len() < target_len {
            self.message_scroll_anchors
                .push(ScrollAnchor::for_handle(self.chat_scroll_handle.clone()));
        }
    }

    pub(crate) fn reveal_artifact(
        &mut self,
        target: ArtifactHighlightTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if target.message_ix >= self.messages.len() {
            window.push_notification(
                Notification::info(crate::tr!("conversation.source_not_found")),
                cx,
            );
            return;
        }

        self.ensure_message_scroll_anchors();
        self.highlighted_artifact_target = Some(target);
        if let Some(anchor) = self.message_scroll_anchors.get(target.message_ix) {
            anchor.scroll_to(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn snapshot(&self) -> Conversation {
        Conversation {
            id: self.id,
            title: self.title.clone(),
            pinned: self.pinned,
            pending: self.pending,
            project_id: self.project_id,
            branch_origin: self.branch_origin.clone(),
            messages: self.messages.clone(),
            cowork_user_expanded: self.cowork_user_expanded.clone(),
            tool_expanded: self.tool_expanded.clone(),
        }
    }

    pub(crate) fn set_project_id(&mut self, project_id: Option<usize>, cx: &mut Context<Self>) {
        self.project_id = project_id;
        cx.notify();
    }

    fn root_settings(&self, cx: &App) -> (SharedString, bool, bool, ChatMode) {
        if let Some(app) = self.app.upgrade() {
            let settings = &app.read(cx).settings;
            (
                settings.current_model.clone(),
                settings.adaptive_thinking,
                settings.web_search,
                settings.mode,
            )
        } else {
            ("Sonnet 4.6".into(), false, true, self.mode)
        }
    }

    fn mcp_settings(&self, cx: &App) -> (bool, HashMap<String, bool>) {
        self.app
            .upgrade()
            .map(|app| {
                let settings = &app.read(cx).settings;
                (settings.mcp_enabled, settings.mcp_server_enabled.clone())
            })
            .unwrap_or_else(|| (true, HashMap::new()))
    }

    fn document_parsing_enabled(&self, cx: &App) -> bool {
        self.app
            .upgrade()
            .map(|app| app.read(cx).settings.document_parsing_enabled)
            .unwrap_or(true)
    }

    fn document_ocr_enabled(&self, cx: &App) -> bool {
        self.app
            .upgrade()
            .map(|app| app.read(cx).settings.document_ocr_enabled)
            .unwrap_or(false)
    }

    fn sync_to_app(&self, cx: &mut Context<Self>) {
        let snapshot = self.snapshot();
        if let Some(app) = self.app.upgrade() {
            app.update(cx, |app, cx| {
                app.upsert_conversation_snapshot(snapshot, cx);
            });
        }
    }

    fn message_history_turn(message: &ChatMessage) -> ChatTurn {
        ChatTurn {
            role: message.role.clone(),
            content: Self::message_content_with_documents(message),
            images: message
                .attachments
                .iter()
                .filter(|attachment| !attachment.is_document())
                .map(Self::chat_image_from_attachment)
                .collect(),
        }
    }

    fn message_content_with_documents(message: &ChatMessage) -> String {
        let mut content = message.content.to_string();
        let mut parsed_documents = message
            .attachments
            .iter()
            .filter_map(|attachment| {
                attachment
                    .parsed_text
                    .as_ref()
                    .map(|text| (attachment, text.as_ref()))
            })
            .peekable();

        if parsed_documents.peek().is_none() {
            return content;
        }

        if !content.trim().is_empty() {
            content.push_str("\n\n");
        }
        content.push_str("Attached documents:\n");

        for (attachment, text) in parsed_documents {
            content.push_str("\n---\n");
            content.push_str("Document: ");
            content.push_str(attachment.title.as_ref());
            if !attachment.detail.is_empty() {
                content.push_str("\nDetail: ");
                content.push_str(attachment.detail.as_ref());
            }
            content.push_str("\nContent:\n");
            content.push_str(text);
            content.push('\n');
        }

        content
    }

    fn chat_image_from_attachment(attachment: &ImageAttachment) -> ChatImage {
        ChatImage {
            title: attachment.title.to_string(),
            detail: attachment.detail.to_string(),
            url: attachment.url.to_string(),
            path: attachment.path.clone(),
        }
    }

    fn push_plain_error_reply(
        &mut self,
        body: SharedString,
        model: SharedString,
        mode: ChatMode,
        cx: &mut Context<Self>,
    ) {
        self.messages.push(ChatMessage {
            role: ChatRole::Ai,
            content: body,
            thinking: SharedString::default(),
            model,
            mode,
            created_at_ms: Some(current_time_ms()),
            token_stats: None,
            attachments: Vec::new(),
            blocks: None,
        });
        self.cowork_user_expanded.push(false);
        self.pending = false;
        self.sync_to_app(cx);
        cx.notify();
    }

    fn push_image_generation_reply(
        &mut self,
        result: ImageGenerationResult,
        prompt: &str,
        model: SharedString,
        mode: ChatMode,
        cx: &mut Context<Self>,
    ) {
        let body: SharedString = result.text.clone().into();
        let mut blocks = Vec::new();

        if !result.text.trim().is_empty() {
            blocks.push(MessageBlock::Markdown(
                cx.new(|cx| TextViewState::markdown(&result.text, cx)),
            ));
        }

        for image in result.images {
            blocks.push(MessageBlock::GeneratedImage(GeneratedImage {
                title: image.title.into(),
                prompt: prompt.to_string().into(),
                url: image.url.into(),
                size: image.detail.into(),
            }));
        }

        self.messages.push(ChatMessage {
            role: ChatRole::Ai,
            content: body,
            thinking: SharedString::default(),
            model,
            mode,
            created_at_ms: Some(current_time_ms()),
            token_stats: None,
            attachments: Vec::new(),
            blocks: Some(blocks),
        });
        self.cowork_user_expanded.push(false);
        self.pending = false;
        self.sync_to_app(cx);
        cx.notify();
    }

    fn push_streaming_ai_reply(&mut self, model: SharedString, mode: ChatMode) {
        self.messages.push(ChatMessage {
            role: ChatRole::Ai,
            content: SharedString::default(),
            thinking: SharedString::default(),
            model,
            mode,
            created_at_ms: Some(current_time_ms()),
            token_stats: None,
            attachments: Vec::new(),
            blocks: Some(Vec::new()),
        });
        self.cowork_user_expanded.push(false);
    }

    fn ensure_streaming_ai_reply(
        &mut self,
        started: &mut bool,
        model: &SharedString,
        mode: ChatMode,
    ) {
        if !*started {
            self.push_streaming_ai_reply(model.clone(), mode);
            *started = true;
        }
    }

    fn current_streaming_ai_message_mut(&mut self) -> Option<&mut ChatMessage> {
        let message = self.messages.last_mut()?;
        (message.role == ChatRole::Ai).then_some(message)
    }

    fn update_streaming_token_stats(&mut self, stats: Option<TokenUsageStats>) {
        let Some(stats) = stats else {
            return;
        };
        if stats.output_tokens == 0 || !stats.tokens_per_second.is_finite() {
            return;
        }
        if let Some(message) = self.current_streaming_ai_message_mut()
            && message.mode == ChatMode::Chat
        {
            message.token_stats = Some(stats);
        }
    }

    fn clear_streaming_token_stats(&mut self) {
        if let Some(message) = self.current_streaming_ai_message_mut() {
            message.token_stats = None;
        }
    }

    fn chat_is_at_bottom(&self) -> bool {
        const AUTO_SCROLL_BOTTOM_TOLERANCE: Pixels = px(24.);

        let offset = self.chat_scroll_handle.offset();
        let max_offset = self.chat_scroll_handle.max_offset();
        (offset.y + max_offset.y).abs() <= AUTO_SCROLL_BOTTOM_TOLERANCE
    }

    fn scroll_chat_to_bottom_if(&self, should_scroll: bool) {
        if should_scroll {
            self.chat_scroll_handle.scroll_to_bottom();
        }
    }

    fn ensure_thinking_block(
        message: &mut ChatMessage,
        state: &Entity<TextViewState>,
        done: bool,
        expanded: bool,
    ) {
        let blocks = message.blocks.get_or_insert_with(Vec::new);
        if let Some(block) = blocks.iter_mut().find_map(|block| match block {
            MessageBlock::Thinking(block) => Some(block),
            _ => None,
        }) {
            block.state = state.clone();
            block.done = done;
            block.expanded = expanded;
        } else {
            blocks.insert(
                0,
                MessageBlock::Thinking(chat_view::ThinkingBlock {
                    state: state.clone(),
                    done,
                    expanded,
                }),
            );
        }
    }

    fn ensure_markdown_block(message: &mut ChatMessage, state: &Entity<TextViewState>) {
        let blocks = message.blocks.get_or_insert_with(Vec::new);
        if !blocks
            .iter()
            .any(|block| matches!(block, MessageBlock::Markdown(_)))
        {
            blocks.push(MessageBlock::Markdown(state.clone()));
        }
    }

    fn finish_thinking_blocks(message: &mut ChatMessage) {
        if let Some(blocks) = message.blocks.as_mut() {
            for block in blocks {
                if let MessageBlock::Thinking(thinking) = block {
                    thinking.done = true;
                    thinking.expanded = false;
                }
            }
        }
    }

    fn append_streaming_thinking_delta(
        &mut self,
        delta: String,
        model: &SharedString,
        mode: ChatMode,
        started: &mut bool,
        thinking_state: &mut Option<Entity<TextViewState>>,
        cx: &mut Context<Self>,
    ) {
        if delta.is_empty() {
            return;
        }

        let should_scroll = self.chat_is_at_bottom();
        self.ensure_streaming_ai_reply(started, model, mode);
        let state = if let Some(state) = thinking_state.as_ref() {
            state.clone()
        } else {
            let initial = self
                .messages
                .last()
                .map(|message| message.thinking.to_string())
                .unwrap_or_default();
            let state = cx.new(|cx| TextViewState::markdown(&initial, cx));
            *thinking_state = Some(state.clone());
            state
        };

        if let Some(message) = self.current_streaming_ai_message_mut() {
            message.thinking = format!("{}{}", message.thinking, delta).into();
            Self::ensure_thinking_block(message, &state, false, true);
        }
        state.update(cx, |state, cx| state.push_str(&delta, cx));
        self.scroll_chat_to_bottom_if(should_scroll);
    }

    fn set_streaming_thinking(
        &mut self,
        thinking: String,
        model: &SharedString,
        mode: ChatMode,
        started: &mut bool,
        thinking_state: &mut Option<Entity<TextViewState>>,
        cx: &mut Context<Self>,
    ) {
        if thinking.is_empty() {
            return;
        }

        let should_scroll = self.chat_is_at_bottom();
        self.ensure_streaming_ai_reply(started, model, mode);
        let state = if let Some(state) = thinking_state.as_ref() {
            state.clone()
        } else {
            let state = cx.new(|cx| TextViewState::markdown("", cx));
            *thinking_state = Some(state.clone());
            state
        };
        let thinking: SharedString = thinking.into();
        state.update(cx, |state, cx| state.set_text(&thinking, cx));
        if let Some(message) = self.current_streaming_ai_message_mut() {
            message.thinking = thinking;
            Self::ensure_thinking_block(message, &state, true, false);
        }
        self.scroll_chat_to_bottom_if(should_scroll);
    }

    fn append_streaming_answer_delta(
        &mut self,
        delta: String,
        model: &SharedString,
        mode: ChatMode,
        started: &mut bool,
        markdown_state: &mut Option<Entity<TextViewState>>,
        cx: &mut Context<Self>,
    ) {
        if delta.is_empty() {
            return;
        }

        let should_scroll = self.chat_is_at_bottom();
        self.ensure_streaming_ai_reply(started, model, mode);
        let state = if let Some(state) = markdown_state.as_ref() {
            state.clone()
        } else {
            let initial = self
                .messages
                .last()
                .map(|message| message.content.to_string())
                .unwrap_or_default();
            let state = cx.new(|cx| TextViewState::markdown(&initial, cx));
            *markdown_state = Some(state.clone());
            state
        };

        if let Some(message) = self.current_streaming_ai_message_mut() {
            Self::finish_thinking_blocks(message);
            message.content = format!("{}{}", message.content, delta).into();
            Self::ensure_markdown_block(message, &state);
        }
        state.update(cx, |state, cx| state.push_str(&delta, cx));
        self.scroll_chat_to_bottom_if(should_scroll);
    }

    fn set_streaming_answer(
        &mut self,
        body: SharedString,
        model: &SharedString,
        mode: ChatMode,
        started: &mut bool,
        markdown_state: &mut Option<Entity<TextViewState>>,
        cx: &mut Context<Self>,
    ) {
        let should_scroll = self.chat_is_at_bottom();
        self.ensure_streaming_ai_reply(started, model, mode);
        let state = if let Some(state) = markdown_state.as_ref() {
            state.clone()
        } else {
            let state = cx.new(|cx| TextViewState::markdown("", cx));
            *markdown_state = Some(state.clone());
            state
        };
        state.update(cx, |state, cx| state.set_text(&body, cx));
        if let Some(message) = self.current_streaming_ai_message_mut() {
            Self::finish_thinking_blocks(message);
            message.content = body;
            Self::ensure_markdown_block(message, &state);
        }
        self.scroll_chat_to_bottom_if(should_scroll);
    }

    fn finish_streaming_thinking(&mut self) {
        let should_scroll = self.chat_is_at_bottom();
        if let Some(message) = self.current_streaming_ai_message_mut() {
            Self::finish_thinking_blocks(message);
        }
        self.scroll_chat_to_bottom_if(should_scroll);
    }

    fn append_streaming_tool_started(
        &mut self,
        title: String,
        input: String,
        model: &SharedString,
        mode: ChatMode,
        started: &mut bool,
    ) {
        let should_scroll = self.chat_is_at_bottom();
        self.ensure_streaming_ai_reply(started, model, mode);
        let title: SharedString = title.into();
        let detail = Self::compact_tool_detail(&input);
        if let Some(message) = self.current_streaming_ai_message_mut() {
            Self::finish_thinking_blocks(message);
            message
                .blocks
                .get_or_insert_with(Vec::new)
                .push(MessageBlock::Tool(ToolCall {
                    title: title.clone(),
                    steps: vec![ToolStep {
                        icon: IconName::SquareTerminal,
                        title: "Calling MCP tool".into(),
                        detail: Some(detail.into()),
                        file_chip: None,
                        done: false,
                    }],
                }));
        }
        self.scroll_chat_to_bottom_if(should_scroll);
    }

    fn append_streaming_tool_finished(&mut self, title: String, output: String, is_error: bool) {
        let should_scroll = self.chat_is_at_bottom();
        let Some(message) = self.current_streaming_ai_message_mut() else {
            return;
        };
        let Some(blocks) = message.blocks.as_mut() else {
            return;
        };
        let detail = Self::compact_tool_detail(&output);
        for block in blocks.iter_mut().rev() {
            let MessageBlock::Tool(tool) = block else {
                continue;
            };
            if tool.title.as_ref() != title {
                continue;
            }
            if tool.steps.iter().any(|step| {
                step.title.as_ref() == "Done"
                    || step.title.as_ref() == "Error"
                    || step.title.as_ref() == "MCP tool returned an error"
            }) {
                continue;
            }

            tool.steps.push(ToolStep {
                icon: if is_error {
                    IconName::TriangleAlert
                } else {
                    IconName::File
                },
                title: if is_error {
                    "MCP tool returned an error".into()
                } else {
                    "MCP tool returned result".into()
                },
                detail: Some(detail.into()),
                file_chip: None,
                done: false,
            });
            tool.steps.push(ToolStep {
                icon: IconName::CircleCheck,
                title: if is_error {
                    "Error".into()
                } else {
                    "Done".into()
                },
                detail: None,
                file_chip: None,
                done: !is_error,
            });
            self.scroll_chat_to_bottom_if(should_scroll);
            return;
        }
    }

    fn compact_tool_detail(value: &str) -> String {
        const MAX_CHARS: usize = 900;
        let trimmed = value.trim();
        let mut chars = trimmed.chars();
        let compact: String = chars.by_ref().take(MAX_CHARS).collect();
        if chars.next().is_some() {
            format!("{compact}\n[truncated]")
        } else if compact.is_empty() {
            "(empty)".to_string()
        } else {
            compact
        }
    }

    fn is_image_generation_request(text: &str) -> bool {
        let lower = text.to_lowercase();
        let asks_for_image = lower.contains("generate image")
            || lower.contains("generate an image")
            || lower.contains("create image")
            || lower.contains("create an image")
            || lower.contains("image of")
            || lower.contains("draw ")
            || lower.contains("illustration")
            || lower.contains("poster")
            || lower.contains("生成图片")
            || lower.contains("生成一张")
            || lower.contains("画一张")
            || lower.contains("图像生成");
        asks_for_image && !text.trim().is_empty()
    }

    fn send(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if self.pending || (text.is_empty() && self.pending_images.is_empty()) {
            return;
        }
        let attachments = std::mem::take(&mut self.pending_images);
        let editing_user_ix = self.editing_user_ix.take();

        if self.title.is_empty() {
            self.title = if text.is_empty() {
                attachments
                    .first()
                    .map(|attachment| {
                        crate::tr!("chat.image_title", title = attachment.title.to_string())
                    })
                    .unwrap_or_else(|| crate::tr!("chat.image_chat"))
            } else {
                ClaudeApp::title_from_text(&text)
            };
        }

        let (model, _, _, mode) = self.root_settings(cx);
        self.mode = mode;
        if let Some(ix) = editing_user_ix {
            if matches!(self.messages.get(ix), Some(message) if message.role == ChatRole::User) {
                self.truncate_from_message(ix + 1);
                if let Some(message) = self.messages.get_mut(ix) {
                    message.content = text.clone().into();
                    message.thinking = SharedString::default();
                    message.model = model.clone();
                    message.mode = mode;
                    message.created_at_ms = Some(current_time_ms());
                    message.token_stats = None;
                    message.attachments = attachments;
                    message.blocks = None;
                }
                self.input.update(cx, |s, cx| {
                    s.set_value("", window, cx);
                    s.set_placeholder(crate::tr!("chat.reply_placeholder"), window, cx);
                });
                self.pending = true;
                self.sync_to_app(cx);
                cx.notify();

                self.start_reply_for_user(ix, window, cx);
                return;
            }
        }

        self.messages.push(ChatMessage {
            role: ChatRole::User,
            content: text.clone().into(),
            thinking: SharedString::default(),
            model: model.clone(),
            mode,
            created_at_ms: Some(current_time_ms()),
            token_stats: None,
            attachments: attachments.clone(),
            blocks: None,
        });
        self.cowork_user_expanded.push(false);
        self.input.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.set_placeholder(crate::tr!("chat.reply_placeholder"), window, cx);
        });
        self.pending = true;
        self.sync_to_app(cx);
        cx.notify();

        self.start_reply_for_user(self.messages.len().saturating_sub(1), window, cx);
    }

    fn stop_generation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pending {
            return;
        }

        if let Some(cancel) = self.generation_cancel.take() {
            cancel.cancel();
        }
        self.generation_task.take();
        self.finish_streaming_thinking();
        self.pending = false;
        self.sync_to_app(cx);
        cx.notify();
        window.push_notification(
            Notification::info(crate::tr!("conversation.response_stopped")),
            cx,
        );
    }

    fn handle_voice_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.voice_state {
            VoiceInputState::Idle => self.start_voice_recording(window, cx),
            VoiceInputState::Starting => {
                window.push_notification(Notification::info(crate::tr!("chat.voice_starting")), cx);
            }
            VoiceInputState::Recording => self.stop_voice_recording(window, cx),
            VoiceInputState::Transcribing => {
                window
                    .push_notification(Notification::info(crate::tr!("chat.voice_processing")), cx);
            }
        }
    }

    fn start_voice_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            window.push_notification(
                Notification::info(crate::tr!("conversation.wait_response")),
                cx,
            );
            return;
        }
        if self.voice_task.is_some() {
            window.push_notification(Notification::info(crate::tr!("chat.voice_processing")), cx);
            return;
        }

        let configured_device = self.app.upgrade().and_then(|app| {
            let device = app.read(cx).settings.audio_input_device.to_string();
            (!device.trim().is_empty()).then_some(device)
        });

        match VoiceRecorder::start(configured_device) {
            Ok(mut recorder) => {
                let Some(mut events) = recorder.take_events() else {
                    window.push_notification(
                        Notification::error("Voice input failed to create an event stream."),
                        cx,
                    );
                    return;
                };

                self.voice_base_input = self.input.read(cx).value().to_string();
                self.voice_committed_text.clear();
                self.voice_recorder = Some(recorder);
                self.voice_state = VoiceInputState::Starting;
                self.voice_task = Some(cx.spawn_in(window, async move |panel, cx| {
                    while let Some(event) = events.next().await {
                        let finished = matches!(event, VoiceEvent::Finished);
                        _ = panel.update_in(cx, |this, window, cx| {
                            this.handle_voice_event(event, window, cx);
                        });
                        if finished {
                            return;
                        }
                    }

                    _ = panel.update(cx, |this, cx| {
                        this.finish_voice_input(false);
                        cx.notify();
                    });
                }));
                window.push_notification(Notification::info(crate::tr!("chat.voice_starting")), cx);
                cx.notify();
            }
            Err(err) => {
                window.push_notification(Notification::error(err), cx);
            }
        }
    }

    fn stop_voice_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(recorder) = self.voice_recorder.as_ref() else {
            self.voice_state = VoiceInputState::Idle;
            cx.notify();
            return;
        };

        recorder.stop();
        self.voice_state = VoiceInputState::Transcribing;
        window.push_notification(Notification::info(crate::tr!("chat.voice_processing")), cx);
        cx.notify();
    }

    fn handle_voice_event(
        &mut self,
        event: VoiceEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            VoiceEvent::Ready { device_name } => {
                self.voice_state = VoiceInputState::Recording;
                window.push_notification(
                    Notification::info(crate::tr!(
                        "chat.voice_recording_device",
                        device = device_name
                    )),
                    cx,
                );
            }
            VoiceEvent::Preview(preview) => {
                self.update_voice_input_preview(Some(&preview), window, cx);
            }
            VoiceEvent::Commit(text) => {
                self.commit_voice_text(&text);
                self.update_voice_input_preview(None, window, cx);
            }
            VoiceEvent::Error(err) => {
                self.update_voice_input_preview(None, window, cx);
                self.finish_voice_input(true);
                window.push_notification(Notification::error(err), cx);
            }
            VoiceEvent::Finished => {
                let recognized = !self.voice_committed_text.trim().is_empty();
                self.finish_voice_input(true);
                if recognized {
                    window.push_notification(
                        Notification::success(crate::tr!("chat.voice_transcribed")),
                        cx,
                    );
                } else {
                    window.push_notification(
                        Notification::error(crate::tr!("chat.voice_no_speech")),
                        cx,
                    );
                }
            }
        }
        cx.notify();
    }

    fn commit_voice_text(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }

        self.voice_committed_text =
            Self::join_input_text(&self.voice_committed_text, text).to_string();
    }

    fn update_voice_input_preview(
        &mut self,
        preview: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut transcript = self.voice_committed_text.clone();
        if let Some(preview) = preview.map(str::trim).filter(|preview| !preview.is_empty()) {
            transcript = Self::join_input_text(&transcript, preview).to_string();
        }
        let value = Self::join_input_text(&self.voice_base_input, &transcript).to_string();
        self.input.update(cx, |state, cx| {
            state.set_value(value, window, cx);
            state.focus(window, cx);
        });
    }

    fn join_input_text(left: &str, right: &str) -> String {
        let left = left.trim_end();
        let right = right.trim();
        if left.is_empty() {
            right.to_string()
        } else if right.is_empty() {
            left.to_string()
        } else {
            format!("{left} {right}")
        }
    }

    fn finish_voice_input(&mut self, clear_transcript: bool) {
        self.voice_recorder = None;
        self.voice_task = None;
        self.voice_state = VoiceInputState::Idle;
        self.voice_base_input.clear();
        if clear_transcript {
            self.voice_committed_text.clear();
        }
    }

    fn voice_input_tooltip(&self) -> SharedString {
        match self.voice_state {
            VoiceInputState::Idle => crate::tr!("chat.voice_input"),
            VoiceInputState::Starting => crate::tr!("chat.voice_starting"),
            VoiceInputState::Recording => crate::tr!("chat.voice_stop_recording"),
            VoiceInputState::Transcribing => crate::tr!("chat.voice_processing"),
        }
    }

    fn start_reply_for_user(
        &mut self,
        user_ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(user_message) = self.messages.get(user_ix).cloned() else {
            self.pending = false;
            cx.notify();
            return;
        };
        let text = user_message.content.to_string();
        let model = user_message.model.clone();
        let mode = user_message.mode;
        self.mode = mode;

        if mode == ChatMode::Chat {
            let history = self
                .messages
                .iter()
                .take(user_ix + 1)
                .map(Self::message_history_turn)
                .collect::<Vec<_>>();
            let (route, adaptive_thinking) = if let Some(app) = self.app.upgrade() {
                let app = app.read(cx);
                (app.route_for_current(), app.settings.adaptive_thinking)
            } else {
                (None, false)
            };
            let Some(route) = route else {
                self.push_plain_error_reply(
                    format!("⚠ {}", crate::tr!("chat.no_model_selected")).into(),
                    model.clone(),
                    mode,
                    cx,
                );
                return;
            };

            if Self::is_image_generation_request(&text) {
                let prompt = text.clone();
                let rx = genai_backend::generate_images(route, history);
                self.generation_cancel = None;
                self.generation_task = Some(cx.spawn_in(window, async move |panel, cx| {
                    let result = rx.await;
                    _ = panel.update(cx, |this, cx| match result {
                        Ok(Ok(result)) => {
                            this.push_image_generation_reply(result, &prompt, model, mode, cx);
                        }
                        Ok(Err(err)) => {
                            this.push_plain_error_reply(format!("⚠ {err}").into(), model, mode, cx);
                        }
                        Err(_) => {
                            this.push_plain_error_reply(
                                format!("⚠ {}", crate::tr!("chat.generation_cancelled")).into(),
                                model,
                                mode,
                                cx,
                            );
                        }
                    });
                    _ = panel.update(cx, |this, cx| {
                        this.generation_task = None;
                        this.generation_cancel = None;
                        cx.notify();
                    });
                }));
                return;
            }

            let (mcp_enabled, mcp_server_enabled) = self.mcp_settings(cx);
            let stream = genai_backend::stream_chat(
                route,
                history,
                crate::store::mcp_config_path(),
                mcp_enabled,
                mcp_server_enabled,
                adaptive_thinking,
            );
            let mut rx = stream.receiver;
            self.generation_cancel = Some(stream.cancel);
            self.generation_task = Some(cx.spawn_in(window, async move |panel, cx| {
                let mut started = false;
                let mut markdown_state = None;
                let mut thinking_state = None;
                let mut token_counter = StreamingTokenCounter::default();
                while let Some(message) = rx.next().await {
                    _ = panel.update(cx, |this, cx| {
                        match message {
                            StreamMsg::Delta(delta) => {
                                let token_stats = token_counter.record_delta(&delta);
                                this.append_streaming_answer_delta(
                                    delta,
                                    &model,
                                    mode,
                                    &mut started,
                                    &mut markdown_state,
                                    cx,
                                );
                                this.update_streaming_token_stats(token_stats);
                            }
                            StreamMsg::ReasoningDelta(delta) => {
                                let token_stats = token_counter.record_delta(&delta);
                                this.append_streaming_thinking_delta(
                                    delta,
                                    &model,
                                    mode,
                                    &mut started,
                                    &mut thinking_state,
                                    cx,
                                );
                                this.update_streaming_token_stats(token_stats);
                            }
                            StreamMsg::ReasoningFinal(thinking) => {
                                this.set_streaming_thinking(
                                    thinking,
                                    &model,
                                    mode,
                                    &mut started,
                                    &mut thinking_state,
                                    cx,
                                );
                            }
                            StreamMsg::TokenUsage { output_tokens } => {
                                let token_stats = token_counter.record_usage(output_tokens);
                                this.update_streaming_token_stats(token_stats);
                            }
                            StreamMsg::ToolStarted { title, input } => {
                                this.append_streaming_tool_started(
                                    title,
                                    input,
                                    &model,
                                    mode,
                                    &mut started,
                                );
                            }
                            StreamMsg::ToolFinished {
                                title,
                                output,
                                is_error,
                            } => {
                                this.append_streaming_tool_finished(title, output, is_error);
                            }
                            StreamMsg::Error(err) => {
                                let body: SharedString = format!("⚠ {err}").into();
                                this.set_streaming_answer(
                                    body,
                                    &model,
                                    mode,
                                    &mut started,
                                    &mut markdown_state,
                                    cx,
                                );
                                this.clear_streaming_token_stats();
                                this.pending = false;
                            }
                        }
                        cx.notify();
                    });
                }
                _ = panel.update(cx, |this, cx| {
                    this.finish_streaming_thinking();
                    this.pending = false;
                    this.generation_task = None;
                    this.generation_cancel = None;
                    this.sync_to_app(cx);
                    cx.notify();
                });
            }));
            return;
        }

        self.generation_task = Some(cx.spawn_in(window, async move |panel, cx| {
            let (delay, reply) = mock_backend::backend_response(mode, &text);
            cx.background_executor().timer(delay).await;
            _ = panel.update(cx, |this, cx| {
                let blocks = if mode == ChatMode::Cowork {
                    Some(mock_backend::build_cowork_mock_reply(cx))
                } else {
                    None
                };
                this.messages.push(ChatMessage {
                    role: ChatRole::Ai,
                    content: reply,
                    thinking: SharedString::default(),
                    model,
                    mode,
                    created_at_ms: Some(current_time_ms()),
                    token_stats: None,
                    attachments: Vec::new(),
                    blocks,
                });
                this.cowork_user_expanded.push(false);
                this.pending = false;
                this.generation_task = None;
                this.generation_cancel = None;
                this.sync_to_app(cx);
                cx.notify();
            });
        }));
    }

    fn remove_message_at(&mut self, ix: usize) {
        self.remove_message_range(ix, ix + 1);
    }

    fn remove_message_range(&mut self, start_ix: usize, end_ix: usize) {
        if start_ix >= end_ix || start_ix >= self.messages.len() {
            return;
        }

        let end_ix = end_ix.min(self.messages.len());
        let removed_count = end_ix - start_ix;
        self.messages.drain(start_ix..end_ix);
        if start_ix < self.cowork_user_expanded.len() {
            let expanded_end = end_ix.min(self.cowork_user_expanded.len());
            self.cowork_user_expanded.drain(start_ix..expanded_end);
        }
        self.editing_user_ix = self.editing_user_ix.and_then(|ix| {
            if (start_ix..end_ix).contains(&ix) {
                None
            } else if ix >= end_ix {
                Some(ix - removed_count)
            } else {
                Some(ix)
            }
        });

        let old_tool_expanded = std::mem::take(&mut self.tool_expanded);
        self.tool_expanded = old_tool_expanded
            .into_iter()
            .filter_map(|((msg_ix, block_ix), expanded)| {
                if (start_ix..end_ix).contains(&msg_ix) {
                    None
                } else {
                    Some((
                        (
                            if msg_ix >= end_ix {
                                msg_ix - removed_count
                            } else {
                                msg_ix
                            },
                            block_ix,
                        ),
                        expanded,
                    ))
                }
            })
            .collect();
        self.adjust_branch_origin_after_remove(start_ix, end_ix);
    }

    fn restore_message_range(
        &mut self,
        start_ix: usize,
        messages: Vec<ChatMessage>,
        cowork_user_expanded: Vec<bool>,
        tool_expanded: HashMap<(usize, usize), bool>,
        branch_origin: Option<BranchOrigin>,
        cx: &mut Context<Self>,
    ) {
        if messages.is_empty() || start_ix > self.messages.len() {
            return;
        }

        let insert_count = messages.len();
        self.messages.splice(start_ix..start_ix, messages);
        let expanded = if cowork_user_expanded.len() == insert_count {
            cowork_user_expanded
        } else {
            vec![false; insert_count]
        };
        let expanded_start = start_ix.min(self.cowork_user_expanded.len());
        self.cowork_user_expanded
            .splice(expanded_start..expanded_start, expanded);
        self.tool_expanded = tool_expanded;
        self.branch_origin = branch_origin;
        self.sync_to_app(cx);
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    fn push_delete_undo_notification(
        &self,
        label: SharedString,
        start_ix: usize,
        messages: Vec<ChatMessage>,
        cowork_user_expanded: Vec<bool>,
        tool_expanded: HashMap<(usize, usize), bool>,
        branch_origin: Option<BranchOrigin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity().downgrade();
        window.push_notification(
            Notification::info(label).action(move |_, _, cx| {
                let panel = panel.clone();
                let messages = messages.clone();
                let cowork_user_expanded = cowork_user_expanded.clone();
                let tool_expanded = tool_expanded.clone();
                let branch_origin = branch_origin.clone();
                Button::new("undo-message-delete")
                    .primary()
                    .label(crate::tr!("common.undo"))
                    .on_click(cx.listener(move |notification, _, window, cx| {
                        if let Some(panel) = panel.upgrade() {
                            panel.update(cx, |panel, cx| {
                                panel.restore_message_range(
                                    start_ix,
                                    messages.clone(),
                                    cowork_user_expanded.clone(),
                                    tool_expanded.clone(),
                                    branch_origin.clone(),
                                    cx,
                                );
                            });
                        }
                        notification.dismiss(window, cx);
                    }))
            }),
            cx,
        );
    }

    fn adjust_branch_origin_after_remove(&mut self, start_ix: usize, end_ix: usize) {
        let Some(origin) = self.branch_origin.as_mut() else {
            return;
        };
        if start_ix >= origin.message_count {
            return;
        }

        let removed_before_marker = end_ix.min(origin.message_count) - start_ix;
        origin.message_count -= removed_before_marker;
    }

    fn user_turn_end_ix(&self, user_ix: usize) -> usize {
        self.messages
            .iter()
            .enumerate()
            .skip(user_ix + 1)
            .find_map(|(ix, message)| (message.role == ChatRole::User).then_some(ix))
            .unwrap_or(self.messages.len())
    }

    fn truncate_from_message(&mut self, ix: usize) {
        self.messages.truncate(ix);
        self.cowork_user_expanded.truncate(ix);
        self.tool_expanded.retain(|(msg_ix, _), _| *msg_ix < ix);
        if let Some(origin) = self.branch_origin.as_mut() {
            origin.message_count = origin.message_count.min(ix);
        }
        if self
            .editing_user_ix
            .is_some_and(|editing_ix| editing_ix >= ix)
        {
            self.editing_user_ix = None;
        }
    }

    fn cloned_branch_message(message: &ChatMessage) -> ChatMessage {
        let mut message = message.clone();
        let has_only_hydratable_blocks = message.blocks.as_ref().is_some_and(|blocks| {
            blocks
                .iter()
                .all(|block| matches!(block, MessageBlock::Thinking(_) | MessageBlock::Markdown(_)))
        });
        if message.role == ChatRole::Ai
            && message.mode == ChatMode::Chat
            && has_only_hydratable_blocks
        {
            message.blocks = None;
        }
        message
    }

    fn branch_messages_through(&self, ix: usize) -> Option<Conversation> {
        if ix >= self.messages.len() {
            return None;
        }

        let message_count = ix + 1;
        let messages = self
            .messages
            .iter()
            .take(message_count)
            .map(Self::cloned_branch_message)
            .collect::<Vec<_>>();
        let mut cowork_user_expanded = self
            .cowork_user_expanded
            .iter()
            .take(message_count)
            .copied()
            .collect::<Vec<_>>();
        cowork_user_expanded.resize(message_count, false);
        let tool_expanded = self
            .tool_expanded
            .iter()
            .filter_map(|(&(msg_ix, block_ix), &expanded)| {
                (msg_ix < message_count).then_some(((msg_ix, block_ix), expanded))
            })
            .collect::<HashMap<_, _>>();

        Some(Conversation {
            id: self.id,
            title: self.title.clone(),
            pinned: false,
            pending: false,
            project_id: self.project_id,
            branch_origin: None,
            messages,
            cowork_user_expanded,
            tool_expanded,
        })
    }

    pub(crate) fn select_local_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let document_parsing_enabled = self.document_parsing_enabled(cx);
        let document_ocr_enabled = self.document_ocr_enabled(cx);
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(crate::tr!("chat.file_select_prompt")),
        });

        cx.spawn_in(window, async move |panel, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };

            let attachments = cx
                .background_executor()
                .spawn(async move {
                    let mut attachments = Vec::new();
                    for path in paths {
                        if let Some(image) = Self::image_attachment_from_path(path.clone()) {
                            attachments.push(image);
                        } else if document_parsing_enabled
                            && let Some(attachment) =
                                Self::document_attachment_from_path(path, document_ocr_enabled)
                                    .await
                        {
                            attachments.push(attachment);
                        }
                    }
                    attachments
                })
                .await;

            _ = panel.update(cx, |this, cx| {
                if !attachments.is_empty() {
                    this.pending_images.extend(attachments);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn send_image_generation_sample(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = crate::tr!("chat.image_generation_sample");
        self.input
            .update(cx, |s, cx| s.set_value(prompt.to_string(), window, cx));
        self.send(prompt.to_string(), window, cx);
    }

    fn image_attachment_from_path(path: PathBuf) -> Option<ImageAttachment> {
        if !Self::is_supported_image_path(&path) {
            return None;
        }

        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| crate::tr!("chat.local_image").to_string());
        let detail = Self::local_image_detail(&path);
        let url = path.to_string_lossy().to_string();

        Some(ImageAttachment {
            title: title.into(),
            url: url.into(),
            detail: detail.into(),
            path: Some(path),
            parsed_text: None,
            parse_error: None,
        })
    }

    async fn document_attachment_from_path(
        path: PathBuf,
        document_ocr_enabled: bool,
    ) -> Option<ImageAttachment> {
        if !document_parser::is_supported_document_path(&path) {
            return None;
        }

        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| crate::tr!("chat.local_document").to_string());
        let base_detail = Self::local_document_detail(&path);
        let url = path.to_string_lossy().to_string();
        let ocr_detail = Self::document_ocr_detail(&path, document_ocr_enabled);

        let (detail, parsed_text, parse_error) =
            match document_parser::parse_document(path.clone(), document_ocr_enabled).await {
                Ok(text) => {
                    let chars = text.chars().count();
                    (
                        format!("{base_detail} · Parsed {chars} chars · {ocr_detail}"),
                        Some(text.into()),
                        None,
                    )
                }
                Err(err) => (
                    format!("{base_detail} · Parse failed · {ocr_detail}"),
                    None,
                    Some(err.into()),
                ),
            };

        Some(ImageAttachment {
            title: title.into(),
            url: url.into(),
            detail: detail.into(),
            path: Some(path),
            parsed_text,
            parse_error,
        })
    }

    fn parsing_document_attachment(path: PathBuf, document_ocr_enabled: bool) -> ImageAttachment {
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| crate::tr!("chat.local_document").to_string());
        let base_detail = Self::local_document_detail(&path);
        let ocr_detail = Self::document_ocr_detail(&path, document_ocr_enabled);
        let url = path.to_string_lossy().to_string();

        ImageAttachment {
            title: title.into(),
            url: url.into(),
            detail: format!("{base_detail} · Parsing · {ocr_detail}").into(),
            path: Some(path),
            parsed_text: None,
            parse_error: None,
        }
    }

    fn is_supported_image_path(path: &FsPath) -> bool {
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            return false;
        };
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "avif"
                | "bmp"
                | "gif"
                | "ico"
                | "jpeg"
                | "jpg"
                | "png"
                | "svg"
                | "tif"
                | "tiff"
                | "webp"
        )
    }

    fn local_image_detail(path: &FsPath) -> String {
        Self::local_file_detail(path, "Image")
    }

    fn local_document_detail(path: &FsPath) -> String {
        Self::local_file_detail(path, "Document")
    }

    fn document_ocr_detail(path: &FsPath, document_ocr_enabled: bool) -> &'static str {
        if Self::is_plain_text_document(path) {
            "OCR not needed"
        } else if document_ocr_enabled {
            "OCR enabled"
        } else {
            "OCR disabled"
        }
    }

    fn is_plain_text_document(path: &FsPath) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "txt" | "md" | "markdown" | "log"
                )
            })
            .unwrap_or(false)
    }

    fn local_file_detail(path: &FsPath, fallback: &str) -> String {
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_uppercase())
            .unwrap_or_else(|| fallback.to_string());
        match std::fs::metadata(path).map(|metadata| metadata.len()) {
            Ok(bytes) => format!("{ext} · {}", Self::format_bytes(bytes)),
            Err(_) => ext,
        }
    }

    fn format_bytes(bytes: u64) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = 1024.0 * KB;
        let bytes = bytes as f64;
        if bytes >= MB {
            format!("{:.1} MB", bytes / MB)
        } else if bytes >= KB {
            format!("{:.0} KB", bytes / KB)
        } else {
            format!("{} B", bytes as u64)
        }
    }

    fn remove_pending_image(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.pending_images.len() {
            self.pending_images.remove(ix);
            cx.notify();
        }
    }

    fn retry_pending_document_parse(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let document_ocr_enabled = self.document_ocr_enabled(cx);
        let Some(path) = self
            .pending_images
            .get(ix)
            .and_then(|attachment| attachment.path.clone())
        else {
            return;
        };
        if !document_parser::is_supported_document_path(&path) {
            return;
        }

        self.pending_images[ix] =
            Self::parsing_document_attachment(path.clone(), document_ocr_enabled);
        cx.notify();

        cx.spawn_in(window, async move |panel, cx| {
            let attachment = cx
                .background_executor()
                .spawn(async move {
                    Self::document_attachment_from_path(path, document_ocr_enabled).await
                })
                .await;

            _ = panel.update(cx, |this, cx| {
                if ix < this.pending_images.len()
                    && let Some(attachment) = attachment
                {
                    this.pending_images[ix] = attachment;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn cancel_editing_user_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_user_ix = None;
        self.pending_images.clear();
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    fn render_pending_images(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = h_flex().px_3p5().pt_3p5().gap_2().flex_wrap();
        for (ix, image) in self.pending_images.iter().enumerate() {
            row = row.child(self.render_pending_image(ix, image, cx));
        }
        row
    }

    fn render_pending_image(
        &self,
        ix: usize,
        image: &ImageAttachment,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let title = image.title.clone();
        let detail = image.detail.clone();
        let preview = image.clone();
        let local_path = image.path.clone();
        let can_preview = local_path.is_none() && !image.is_document();
        let can_retry_parse = image.parse_error.is_some()
            && local_path
                .as_deref()
                .is_some_and(document_parser::is_supported_document_path);
        let image_id = format!("pending-image-{}-{ix}", self.id);
        let open_id = format!("pending-image-open-{}-{ix}", self.id);
        let remove_id = format!("pending-image-remove-{}-{ix}", self.id);

        v_flex()
            .id(image_id)
            .w(px(116.))
            .gap_1()
            .child(
                div()
                    .id(open_id)
                    .relative()
                    .h(px(78.))
                    .w_full()
                    .rounded_lg()
                    .overflow_hidden()
                    .border_1()
                    .border_color(border_color())
                    .bg(white_color())
                    .child(chat_view::render_attachment_preview(image))
                    .when_some(local_path.clone(), |this, path| {
                        this.cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.reveal_file(path.clone(), window, cx);
                            }))
                    })
                    .when(can_preview, |this| {
                        this.cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_image_viewer(preview.clone(), window, cx);
                            }))
                    })
                    .child(
                        div()
                            .id(remove_id)
                            .absolute()
                            .top_0()
                            .right_0()
                            .m_1()
                            .size_5()
                            .rounded_full()
                            .bg(white_color())
                            .border_1()
                            .border_color(border_color())
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(text_2())
                            .hover(|this| this.text_color(text_color()))
                            .child(Icon::new(IconName::Close).size_3())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.remove_pending_image(ix, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(text_2())
                    .truncate()
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(text_3())
                    .truncate()
                    .child(detail),
            )
            .when(can_retry_parse, |this| {
                this.child(
                    Button::new(format!("pending-image-retry-{}-{ix}", self.id))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Redo2)
                        .label(crate::tr!("common.retry"))
                        .tooltip(crate::tr!("chat.retry_document"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.retry_pending_document_parse(ix, window, cx);
                        })),
                )
            })
    }

    fn render_input_box(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let input = self.input.clone();
        let input_for_send = input.clone();
        let (current_model, adaptive, web_search, _) = self.root_settings(cx);
        let (mcp_enabled, mcp_server_enabled) = self.mcp_settings(cx);
        let mcp_config_path = crate::store::mcp_config_path();
        let mcp_servers = crate::mcp_backend::configured_servers(
            mcp_config_path.clone(),
            mcp_enabled,
            &mcp_server_enabled,
        );
        let mcp_active = mcp_servers
            .as_ref()
            .is_ok_and(|servers| servers.iter().any(|server| server.enabled));
        let model_label = if current_model.is_empty() {
            crate::tr!("chat.select_model")
        } else {
            current_model.clone()
        };
        let has_composer_content =
            !self.input.read(cx).value().trim().is_empty() || !self.pending_images.is_empty();
        let can_send =
            has_composer_content && !self.pending && self.voice_state == VoiceInputState::Idle;
        let app = self.app.clone();
        let id = self.id;
        let panel = cx.entity().downgrade();
        let voice_recording = self.voice_state == VoiceInputState::Recording;
        let voice_disabled = matches!(
            self.voice_state,
            VoiceInputState::Starting | VoiceInputState::Transcribing
        );
        let voice_tooltip = self.voice_input_tooltip();
        let mcp_trigger = if mcp_active {
            Button::new(format!("panel-mcp-btn-{id}"))
                .small()
                .icon(IconName::SquareTerminal)
                .label("MCP")
                .outline()
                .tooltip(crate::tr!("chat.mcp_enabled"))
        } else {
            Button::new(format!("panel-mcp-btn-{id}"))
                .small()
                .icon(IconName::SquareTerminal)
                .label("MCP")
                .ghost()
                .tooltip(crate::tr!("chat.mcp_disabled"))
        };

        v_flex()
            .w_full()
            .bg(white_color())
            .border_1()
            .border_color(border_color())
            .rounded_2xl()
            .shadow_sm()
            .when_some(self.editing_user_ix, |this, edit_ix| {
                this.child(
                    h_flex()
                        .px_3p5()
                        .pt_2p5()
                        .pb_1()
                        .items_center()
                        .justify_between()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .text_size(px(12.))
                                .text_color(text_3())
                                .child(Icon::new(IconName::Replace).size_3p5())
                                .child(crate::tr!("chat.editing_message")),
                        )
                        .child(
                            Button::new(format!("cancel-edit-message-{}-{edit_ix}", self.id))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip(crate::tr!("chat.cancel_edit"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.cancel_editing_user_message(window, cx);
                                })),
                        ),
                )
            })
            .when(!self.pending_images.is_empty(), |this| {
                this.child(self.render_pending_images(cx))
            })
            .child(
                h_flex()
                    .px_3p5()
                    .pt_3p5()
                    .gap_2p5()
                    .items_start()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(14.))
                            .text_color(text_color())
                            .child(Input::new(&input).appearance(false).bordered(false)),
                    )
                    .child(
                        div()
                            .size_2()
                            .rounded_full()
                            .bg(green())
                            .mt_2p5()
                            .flex_shrink_0(),
                    ),
            )
            .child(
                h_flex()
                    .px_2p5()
                    .pb_2p5()
                    .pt_2()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                Popover::new(format!("panel-add-menu-{id}"))
                                    .anchor(Anchor::BottomLeft)
                                    .p_0()
                                    .trigger(
                                        Button::new(format!("panel-add-btn-{id}"))
                                            .small()
                                            .icon(IconName::Plus)
                                            .outline()
                                            .tooltip(crate::tr!("chat.add_tooltip")),
                                    )
                                    .content(move |_, _, cx| {
                                        add_menu_content(cx, web_search, panel.clone())
                                            .into_any_element()
                                    }),
                            )
                            .child(
                                Popover::new(format!("panel-mcp-menu-{id}"))
                                    .anchor(Anchor::BottomLeft)
                                    .p_0()
                                    .trigger(mcp_trigger)
                                    .content({
                                        let app = app.clone();
                                        let mcp_config_path = mcp_config_path.clone();
                                        let mcp_servers = mcp_servers.clone();
                                        move |_, _, cx| {
                                            mcp_menu_content(
                                                cx,
                                                mcp_config_path.clone(),
                                                mcp_servers.clone(),
                                                app.clone(),
                                            )
                                            .into_any_element()
                                        }
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                Popover::new(format!("panel-model-menu-{id}"))
                                    .anchor(Anchor::BottomRight)
                                    .p_0()
                                    .trigger(
                                        Button::new(format!("panel-model-btn-{id}"))
                                            .ghost()
                                            .small()
                                            .label(model_label.clone())
                                            .icon(IconName::ChevronDown),
                                    )
                                    .content({
                                        let app = app.clone();
                                        move |_, _, cx| {
                                            model_menu_content(cx, adaptive, app.clone())
                                                .into_any_element()
                                        }
                                    }),
                            )
                            .child(
                                Button::new(format!("panel-mic-btn-{id}"))
                                    .ghost()
                                    .small()
                                    .disabled(voice_disabled)
                                    .tooltip(voice_tooltip.clone())
                                    .child(if voice_recording {
                                        Self::render_voice_wave_glyph(accent()).into_any_element()
                                    } else {
                                        Self::render_mic_glyph(text_color()).into_any_element()
                                    })
                                    .when(voice_recording, |this| this.outline())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.handle_voice_input(window, cx);
                                    })),
                            )
                            .child(if self.pending {
                                Button::new(format!("panel-stop-btn-{id}"))
                                    .outline()
                                    .small()
                                    .icon(IconName::Close)
                                    .tooltip(crate::tr!("chat.stop_response"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.stop_generation(window, cx);
                                    }))
                            } else {
                                Button::new(format!("panel-send-btn-{id}"))
                                    .primary()
                                    .small()
                                    .icon(IconName::ArrowUp)
                                    .disabled(!can_send)
                                    .tooltip(if can_send {
                                        crate::tr!("chat.send_message")
                                    } else {
                                        crate::tr!("chat.enter_message")
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let value = input_for_send.read(cx).value().to_string();
                                        this.send(value, window, cx);
                                    }))
                            }),
                    ),
            )
    }

    fn render_empty_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .max_w(px(674.))
            .items_center()
            .px_5()
            .pt(px(42.))
            .pb_8()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .justify_center()
                    .flex_wrap()
                    .mb(px(34.))
                    .child(Icon::new(IconName::Asterisk).size_8().text_color(accent()))
                    .child(
                        div()
                            .text_size(px(40.))
                            .line_height(relative(1.))
                            .text_center()
                            .text_color(text_color())
                            .child(crate::tr!("chat.empty_title")),
                    ),
            )
            .child(self.render_home_input_box(cx))
            .child(self.render_pills(cx))
            .child(self.render_setup(cx))
    }

    fn render_mic_glyph(color: Hsla) -> impl IntoElement {
        div()
            .relative()
            .size_5()
            .child(
                div()
                    .absolute()
                    .top(px(2.))
                    .left(px(7.))
                    .w(px(6.))
                    .h(px(10.))
                    .rounded_full()
                    .border_1()
                    .border_color(color),
            )
            .child(
                div()
                    .absolute()
                    .top(px(12.))
                    .left(px(9.5))
                    .w(px(1.))
                    .h(px(5.))
                    .rounded_full()
                    .bg(color),
            )
            .child(
                div()
                    .absolute()
                    .top(px(17.))
                    .left(px(6.))
                    .w(px(8.))
                    .h(px(1.))
                    .rounded_full()
                    .bg(color),
            )
    }

    fn render_voice_wave_glyph(color: Hsla) -> impl IntoElement {
        h_flex()
            .h(px(20.))
            .w(px(24.))
            .gap_0p5()
            .items_center()
            .justify_center()
            .children(
                [7., 12., 17., 12., 7.]
                    .into_iter()
                    .map(move |height| div().w(px(1.5)).h(px(height)).rounded_full().bg(color)),
            )
    }

    fn render_home_input_box(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let input = self.input.clone();
        let input_for_send = input.clone();
        let (current_model, adaptive, web_search, _) = self.root_settings(cx);
        let model_label = if current_model.is_empty() {
            crate::tr!("chat.select_model")
        } else {
            current_model.clone()
        };
        let show_effort = !current_model.is_empty();
        let has_composer_content =
            !self.input.read(cx).value().trim().is_empty() || !self.pending_images.is_empty();
        let can_send =
            has_composer_content && !self.pending && self.voice_state == VoiceInputState::Idle;
        let app = self.app.clone();
        let id = self.id;
        let panel = cx.entity().downgrade();
        let voice_recording = self.voice_state == VoiceInputState::Recording;
        let voice_disabled = matches!(
            self.voice_state,
            VoiceInputState::Starting | VoiceInputState::Transcribing
        );
        let voice_tooltip = self.voice_input_tooltip();

        v_flex()
            .w_full()
            .min_h(px(124.))
            .bg(white_color())
            .border_1()
            .border_color(border_color())
            .rounded(px(18.))
            .shadow_sm()
            .overflow_hidden()
            .when(!self.pending_images.is_empty(), |this| {
                this.child(self.render_pending_images(cx))
            })
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .min_h(px(60.))
                    .px_4()
                    .pt_3p5()
                    .text_size(px(14.5))
                    .text_color(text_color())
                    .child(Input::new(&input).appearance(false).bordered(false)),
            )
            .child(
                h_flex()
                    .px_3()
                    .pb_3()
                    .pt_1()
                    .items_center()
                    .justify_between()
                    .child(
                        Popover::new(format!("home-add-menu-{id}"))
                            .anchor(Anchor::BottomLeft)
                            .p_0()
                            .trigger(
                                Button::new(format!("home-add-btn-{id}"))
                                    .ghost()
                                    .small()
                                    .compact()
                                    .rounded(px(8.))
                                    .icon(IconName::Plus)
                                    .tooltip(crate::tr!("chat.add_tooltip")),
                            )
                            .content(move |_, _, cx| {
                                add_menu_content(cx, web_search, panel.clone()).into_any_element()
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .min_w_0()
                            .child(
                                Popover::new(format!("home-model-menu-{id}"))
                                    .anchor(Anchor::BottomRight)
                                    .p_0()
                                    .trigger(
                                        Button::new(format!("home-model-btn-{id}"))
                                            .ghost()
                                            .small()
                                            .rounded(px(8.))
                                            .child(
                                                h_flex()
                                                    .gap_1()
                                                    .items_center()
                                                    .min_w_0()
                                                    .child(
                                                        div()
                                                            .max_w(px(170.))
                                                            .truncate()
                                                            .child(model_label.clone()),
                                                    )
                                                    .when(show_effort, |this| {
                                                        this.child(
                                                            div()
                                                                .text_color(text_3())
                                                                .child("Medium"),
                                                        )
                                                    })
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
                                            model_menu_content(cx, adaptive, app.clone())
                                                .into_any_element()
                                        }
                                    }),
                            )
                            .child(
                                Button::new(format!("home-mic-btn-{id}"))
                                    .ghost()
                                    .small()
                                    .compact()
                                    .rounded(px(8.))
                                    .disabled(voice_disabled)
                                    .tooltip(voice_tooltip.clone())
                                    .child(if voice_recording {
                                        Self::render_voice_wave_glyph(accent()).into_any_element()
                                    } else {
                                        Self::render_mic_glyph(text_color()).into_any_element()
                                    })
                                    .when(voice_recording, |this| this.outline())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.handle_voice_input(window, cx);
                                    })),
                            )
                            .child(if can_send {
                                Button::new(format!("home-send-btn-{id}"))
                                    .primary()
                                    .small()
                                    .compact()
                                    .rounded(px(8.))
                                    .icon(IconName::ArrowUp)
                                    .tooltip(crate::tr!("chat.send_message"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let value = input_for_send.read(cx).value().to_string();
                                        this.send(value, window, cx);
                                    }))
                            } else {
                                Button::new(format!("home-voice-btn-{id}"))
                                    .ghost()
                                    .small()
                                    .compact()
                                    .rounded(px(8.))
                                    .disabled(voice_disabled)
                                    .tooltip(voice_tooltip.clone())
                                    .child(if voice_recording {
                                        Self::render_voice_wave_glyph(accent()).into_any_element()
                                    } else {
                                        Self::render_voice_wave_glyph(text_color())
                                            .into_any_element()
                                    })
                                    .when(voice_recording, |this| this.outline())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.handle_voice_input(window, cx);
                                    }))
                            }),
                    ),
            )
    }

    fn pill(
        &self,
        id: &'static str,
        icon: IconName,
        label: SharedString,
        sample: SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let panel_id = self.id;
        let sample_text = sample.to_string();
        h_flex()
            .id((id, panel_id))
            .px_2p5()
            .py_1p5()
            .gap_1p5()
            .items_center()
            .rounded(px(7.))
            .border_1()
            .border_color(border_color())
            .bg(white_color())
            .text_size(px(13.5))
            .text_color(text_color())
            .cursor_pointer()
            .shadow_sm()
            .hover(|this| this.bg(pill_hover_bg()).text_color(text_color()))
            .child(Icon::new(icon).size_3p5().text_color(text_2()))
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.input
                    .update(cx, |s, cx| s.set_value(sample_text.clone(), window, cx));
                this.send(sample_text.clone(), window, cx);
            }))
    }

    fn render_pills(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .mt_3()
            .gap_2()
            .flex_wrap()
            .justify_center()
            .child(self.pill(
                "panel-pill-write",
                IconName::CaseSensitive,
                crate::tr!("chat.pill_write"),
                crate::tr!("chat.sample_write"),
                cx,
            ))
            .child(self.pill(
                "panel-pill-learn",
                IconName::BookOpen,
                crate::tr!("chat.pill_learn"),
                crate::tr!("chat.sample_learn"),
                cx,
            ))
            .child(self.pill(
                "panel-pill-code",
                IconName::SquareTerminal,
                crate::tr!("chat.pill_code"),
                crate::tr!("chat.sample_code"),
                cx,
            ))
            .child(self.pill(
                "panel-pill-life",
                IconName::Heart,
                crate::tr!("chat.pill_life"),
                crate::tr!("chat.sample_life"),
                cx,
            ))
            .child(self.pill(
                "panel-pill-surprise",
                IconName::Star,
                crate::tr!("chat.pill_surprise"),
                crate::tr!("chat.sample_surprise"),
                cx,
            ))
    }

    fn render_setup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let labels = [
            crate::tr!("chat.setup_cowork"),
            crate::tr!("chat.setup_history"),
            crate::tr!("chat.setup_tools"),
        ];
        let done_count = self.setup_done.iter().filter(|x| **x).count();
        let total = labels.len();
        let panel_id = self.id;

        v_flex()
            .w_full()
            .max_w(px(700.))
            .px_5()
            .pt_4()
            .pb_8()
            .child(
                h_flex()
                    .items_center()
                    .gap_2p5()
                    .mb_3()
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_color())
                            .child(crate::tr!("chat.setup_title")),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(text_3())
                            .child(crate::tr!("chat.setup_progress", done = done_count)),
                    ),
            )
            .child(
                v_flex()
                    .border_1()
                    .border_color(border_color())
                    .rounded_xl()
                    .bg(white_color())
                    .overflow_hidden()
                    .shadow_sm()
                    .children(labels.iter().enumerate().map(|(ix, label)| {
                        let done = self.setup_done[ix];
                        h_flex()
                            .id(format!("panel-setup-row-{panel_id}-{ix}"))
                            .items_center()
                            .gap_3p5()
                            .px_4()
                            .py_3p5()
                            .cursor_pointer()
                            .when(ix < total - 1, |this: Stateful<Div>| {
                                this.border_b_1().border_color(border_color())
                            })
                            .hover(|this| this.bg(setup_row_hover_bg()))
                            .child(
                                div()
                                    .size_5()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(if done { text_color() } else { border_color() })
                                    .map(|this| if done { this.bg(text_color()) } else { this })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .map(|this| {
                                        if done {
                                            this.child(
                                                Icon::new(IconName::Check)
                                                    .size_3()
                                                    .text_color(white_color()),
                                            )
                                        } else {
                                            this
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .text_color(if done { text_3() } else { text_color() })
                                    .child(label.clone()),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.setup_done[ix] = !this.setup_done[ix];
                                cx.notify();
                            }))
                    })),
            )
    }

    fn image_preview_dialog_width(viewport_width: Pixels) -> Pixels {
        if viewport_width >= px(1280.) {
            px(1120.)
        } else if viewport_width >= px(760.) {
            viewport_width - px(96.)
        } else {
            (viewport_width - px(32.)).max(px(320.))
        }
    }

    fn image_preview_dialog_height(viewport_height: Pixels) -> Pixels {
        if viewport_height >= px(900.) {
            px(720.)
        } else if viewport_height >= px(560.) {
            viewport_height - px(180.)
        } else {
            (viewport_height - px(128.)).max(px(260.))
        }
    }

    fn render_image_preview_dialog_body(
        panel_id: usize,
        image: ImageAttachment,
        image_height: Pixels,
    ) -> impl IntoElement {
        let title = image.title.clone();
        let detail = image.detail.clone();

        v_flex()
            .id(("image-preview-dialog", panel_id))
            .overflow_hidden()
            .rounded_xl()
            .bg(hsla(0.0, 0.0, 0.04, 1.0))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(image_height)
                    .bg(hsla(0.0, 0.0, 0.04, 1.0))
                    .child(chat_view::render_attachment_image_fit(
                        &image,
                        ObjectFit::Contain,
                    ))
                    .child(
                        div()
                            .id(("image-preview-dialog-close", panel_id))
                            .absolute()
                            .top_0()
                            .right_0()
                            .m_3()
                            .size_8()
                            .rounded_full()
                            .bg(white_color().opacity(0.92))
                            .border_1()
                            .border_color(border_color())
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(text_2())
                            .hover(|this| this.bg(white_color()).text_color(text_color()))
                            .child(Icon::new(IconName::Close).size_4())
                            .on_click(|_, window, cx| {
                                cx.stop_propagation();
                                window.close_dialog(cx);
                            }),
                    ),
            )
            .child(
                div().px_4().py_3().bg(hsla(0.0, 0.0, 0.0, 0.82)).child(
                    v_flex()
                        .gap_0p5()
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(white_color())
                                .truncate()
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(white_color().opacity(0.68))
                                .truncate()
                                .child(detail),
                        ),
                ),
            )
    }

    fn open_image_preview_dialog(
        panel_id: usize,
        image: ImageAttachment,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.open_dialog(cx, move |dialog, window, _| {
            let viewport_size = window.viewport_size();
            let width = Self::image_preview_dialog_width(viewport_size.width);
            let image_height = Self::image_preview_dialog_height(viewport_size.height);
            let image = image.clone();

            dialog
                .w(width)
                .margin_top(px(28.))
                .p_0()
                .close_button(false)
                .overlay(true)
                .overlay_closable(true)
                .content(move |content, _, _| {
                    content
                        .gap_0()
                        .child(Self::render_image_preview_dialog_body(
                            panel_id,
                            image.clone(),
                            image_height,
                        ))
                })
        });
    }

    fn open_thinking_preview_dialog(
        panel_id: usize,
        title: SharedString,
        state: Entity<TextViewState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.open_dialog(cx, move |dialog, window, _| {
            let viewport_size = window.viewport_size();
            let width = if viewport_size.width >= px(960.) {
                px(840.)
            } else {
                (viewport_size.width - px(32.)).max(px(320.))
            };
            let height = if viewport_size.height >= px(760.) {
                px(620.)
            } else {
                (viewport_size.height - px(120.)).max(px(320.))
            };
            let title = title.clone();
            let state = state.clone();

            dialog
                .w(width)
                .margin_top(px(28.))
                .p_0()
                .close_button(false)
                .overlay(true)
                .overlay_closable(true)
                .content(move |content, _, _| {
                    content.gap_0().child(
                        v_flex()
                            .id(("thinking-preview-dialog", panel_id))
                            .rounded_xl()
                            .overflow_hidden()
                            .bg(white_color())
                            .child(
                                h_flex()
                                    .px_4()
                                    .py_3()
                                    .items_center()
                                    .justify_between()
                                    .border_b_1()
                                    .border_color(border_color())
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(text_color())
                                            .child(title.clone()),
                                    )
                                    .child(
                                        div()
                                            .id(("thinking-preview-close", panel_id))
                                            .size_8()
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .text_color(text_2())
                                            .hover(|this| {
                                                this.bg(bg_color()).text_color(text_color())
                                            })
                                            .child(Icon::new(IconName::Close).size_4())
                                            .on_click(|_, window, cx| {
                                                cx.stop_propagation();
                                                window.close_dialog(cx);
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .h(height)
                                    .overflow_y_scrollbar()
                                    .px_5()
                                    .py_4()
                                    .text_size(px(13.5))
                                    .line_height(relative(1.6))
                                    .text_color(text_2())
                                    .child(TextView::new(&state).selectable(true)),
                            ),
                    )
                })
        });
    }
}

impl ChatViewState for ConversationPanel {
    fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    fn cowork_user_expanded(&self) -> &[bool] {
        &self.cowork_user_expanded
    }

    fn tool_expanded(&self) -> &HashMap<(usize, usize), bool> {
        &self.tool_expanded
    }

    fn pending(&self) -> bool {
        self.pending
    }

    fn mode(&self) -> ChatMode {
        self.mode
    }

    fn branch_origin(&self) -> Option<&BranchOrigin> {
        self.branch_origin.as_ref()
    }

    fn highlighted_artifact_target(&self) -> Option<ArtifactHighlightTarget> {
        self.highlighted_artifact_target
    }

    fn message_scroll_anchor(&self, ix: usize) -> Option<ScrollAnchor> {
        self.message_scroll_anchors.get(ix).cloned()
    }

    fn open_image_viewer(
        &self,
        image: ImageAttachment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel_id = self.id;
        window.defer(cx, move |window, cx| {
            Self::open_image_preview_dialog(panel_id, image, window, cx);
        });
    }

    fn reveal_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match crate::system_file::reveal(&path) {
            Ok(()) => {
                window.push_notification(
                    Notification::info(crate::tr!("conversation.opened_file_location")),
                    cx,
                );
            }
            Err(err) => {
                window.push_notification(Notification::error(err), cx);
            }
        }
    }

    fn copy_ai_message(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(message) = self.messages.get(ix) else {
            return;
        };
        if message.role != ChatRole::Ai {
            return;
        }

        cx.write_to_clipboard(ClipboardItem::new_string(message.content.to_string()));
        window.push_notification(
            Notification::info(crate::tr!("conversation.response_copied")),
            cx,
        );
    }

    fn branch_conversation_from_message(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending {
            window.push_notification(
                Notification::info(crate::tr!("conversation.wait_response")),
                cx,
            );
            return;
        }

        let Some(conversation) = self.branch_messages_through(ix) else {
            return;
        };
        let app = self.app.clone();
        window.defer(cx, move |window, cx| {
            if let Some(app) = app.upgrade() {
                app.update(cx, |app, cx| {
                    app.branch_conversation(conversation, window, cx);
                });
            }
        });
    }

    fn open_branch_origin(
        &mut self,
        origin: BranchOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = self.app.clone();
        window.defer(cx, move |window, cx| {
            if let Some(app) = app.upgrade() {
                app.update(cx, |app, cx| {
                    app.select_conversation(origin.source_conversation_id, window, cx);
                });
            }
        });
    }

    fn delete_ai_message(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            window.push_notification(
                Notification::info(crate::tr!("conversation.wait_response")),
                cx,
            );
            return;
        }
        if !matches!(self.messages.get(ix), Some(message) if message.role == ChatRole::Ai) {
            return;
        }

        let removed_messages = self.messages[ix..ix + 1].to_vec();
        let removed_expanded = self
            .cowork_user_expanded
            .get(ix..ix + 1)
            .map(|slice| slice.to_vec())
            .unwrap_or_default();
        let tool_expanded = self.tool_expanded.clone();
        let branch_origin = self.branch_origin.clone();
        self.remove_message_at(ix);
        self.sync_to_app(cx);
        cx.notify();
        self.push_delete_undo_notification(
            crate::tr!("conversation.response_deleted"),
            ix,
            removed_messages,
            removed_expanded,
            tool_expanded,
            branch_origin,
            window,
            cx,
        );
    }

    fn regenerate_ai_message(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            window.push_notification(
                Notification::info(crate::tr!("conversation.wait_response")),
                cx,
            );
            return;
        }
        if !matches!(self.messages.get(ix), Some(message) if message.role == ChatRole::Ai) {
            return;
        }

        let Some(user_ix) = (0..ix)
            .rev()
            .find(|candidate| self.messages[*candidate].role == ChatRole::User)
        else {
            window.push_notification(
                Notification::info(crate::tr!("conversation.no_user_prompt")),
                cx,
            );
            return;
        };

        self.truncate_from_message(ix);
        self.pending = true;
        self.sync_to_app(cx);
        cx.notify();
        window.push_notification(
            Notification::info(crate::tr!("conversation.regenerating")),
            cx,
        );
        self.start_reply_for_user(user_ix, window, cx);
    }

    fn copy_user_message(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(message) = self.messages.get(ix) else {
            return;
        };
        if message.role != ChatRole::User {
            return;
        }

        cx.write_to_clipboard(ClipboardItem::new_string(message.content.to_string()));
        window.push_notification(
            Notification::info(crate::tr!("conversation.message_copied")),
            cx,
        );
    }

    fn delete_user_message(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            window.push_notification(
                Notification::info(crate::tr!("conversation.wait_response")),
                cx,
            );
            return;
        }
        if !matches!(self.messages.get(ix), Some(message) if message.role == ChatRole::User) {
            return;
        }

        let end_ix = self.user_turn_end_ix(ix);
        let removed_messages = self.messages[ix..end_ix].to_vec();
        let removed_expanded = self
            .cowork_user_expanded
            .get(ix..end_ix.min(self.cowork_user_expanded.len()))
            .map(|slice| slice.to_vec())
            .unwrap_or_default();
        let tool_expanded = self.tool_expanded.clone();
        let branch_origin = self.branch_origin.clone();
        self.remove_message_range(ix, end_ix);
        self.sync_to_app(cx);
        cx.notify();
        self.push_delete_undo_notification(
            crate::tr!("conversation.message_deleted"),
            ix,
            removed_messages,
            removed_expanded,
            tool_expanded,
            branch_origin,
            window,
            cx,
        );
    }

    fn edit_user_message(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            window.push_notification(
                Notification::info(crate::tr!("conversation.wait_response")),
                cx,
            );
            return;
        }
        let Some(message) = self.messages.get(ix) else {
            return;
        };
        if message.role != ChatRole::User {
            return;
        }
        let content = message.content.to_string();
        let attachments = message.attachments.clone();

        self.editing_user_ix = Some(ix);
        self.pending_images = attachments;
        self.input.update(cx, |state, cx| {
            state.set_value(content, window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    fn regenerate_from_user_message(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending {
            window.push_notification(
                Notification::info(crate::tr!("conversation.wait_response")),
                cx,
            );
            return;
        }
        if !matches!(self.messages.get(ix), Some(message) if message.role == ChatRole::User) {
            return;
        }

        self.truncate_from_message(ix + 1);
        self.pending = true;
        self.sync_to_app(cx);
        cx.notify();
        window.push_notification(
            Notification::info(crate::tr!("conversation.regenerating")),
            cx,
        );
        self.start_reply_for_user(ix, window, cx);
    }

    fn toggle_cowork_user_expanded(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(expanded) = self.cowork_user_expanded.get_mut(ix) {
            *expanded = !*expanded;
            self.sync_to_app(cx);
            cx.notify();
        }
    }

    fn toggle_tool_expanded(&mut self, key: (usize, usize), cx: &mut Context<Self>) {
        let current = self.tool_expanded.get(&key).copied().unwrap_or(true);
        self.tool_expanded.insert(key, !current);
        self.sync_to_app(cx);
        cx.notify();
    }

    fn toggle_thinking_expanded(&mut self, key: (usize, usize), cx: &mut Context<Self>) {
        let (msg_ix, block_ix) = key;
        let Some(MessageBlock::Thinking(thinking)) = self
            .messages
            .get_mut(msg_ix)
            .and_then(|message| message.blocks.as_mut())
            .and_then(|blocks| blocks.get_mut(block_ix))
        else {
            return;
        };

        if thinking.done {
            thinking.expanded = !thinking.expanded;
            self.sync_to_app(cx);
            cx.notify();
        }
    }

    fn open_thinking_viewer(
        &self,
        title: SharedString,
        state: Entity<TextViewState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel_id = self.id;
        window.defer(cx, move |window, cx| {
            Self::open_thinking_preview_dialog(panel_id, title, state, window, cx);
        });
    }
}

impl Panel for ConversationPanel {
    fn panel_name(&self) -> &'static str {
        PANEL_NAME
    }

    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.title_or_untitled())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1p5()
            .items_center()
            .min_w_0()
            .when(self.pinned, |this| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .text_color(text_3())
                        .child(Icon::new(IconName::StarFill).size_3()),
                )
            })
            .child(div().truncate().child(self.title_or_untitled()))
            .when(self.pending, |this| {
                this.child(div().size_1p5().rounded_full().bg(accent()).flex_shrink_0())
            })
    }

    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.activate_in_app(window, cx);
        }
    }

    fn on_added_to(
        &mut self,
        tab_panel: WeakEntity<TabPanel>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.tab_panel = Some(tab_panel);
    }

    fn on_removed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id;
        if let Some(app) = self.app.upgrade() {
            window.defer(cx, move |_, cx| {
                app.update(cx, |app, cx| app.mark_conversation_panel_closed(id, cx));
            });
        }
    }

    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> PopupMenu {
        let id = self.id;
        let app = self.app.clone();

        menu.item(
            PopupMenuItem::new(crate::tr!("menu.close_other_conversations")).on_click({
                let app = app.clone();
                move |_, window, cx| {
                    if let Some(app) = app.upgrade() {
                        app.update(cx, |app, cx| {
                            app.close_conversation_tabs(
                                id,
                                ConversationTabCloseScope::Others,
                                window,
                                cx,
                            );
                        });
                    }
                }
            }),
        )
        .item(
            PopupMenuItem::new(crate::tr!("menu.close_left_conversations")).on_click({
                let app = app.clone();
                move |_, window, cx| {
                    if let Some(app) = app.upgrade() {
                        app.update(cx, |app, cx| {
                            app.close_conversation_tabs(
                                id,
                                ConversationTabCloseScope::Left,
                                window,
                                cx,
                            );
                        });
                    }
                }
            }),
        )
        .item(
            PopupMenuItem::new(crate::tr!("menu.close_right_conversations")).on_click(
                move |_, window, cx| {
                    if let Some(app) = app.upgrade() {
                        app.update(cx, |app, cx| {
                            app.close_conversation_tabs(
                                id,
                                ConversationTabCloseScope::Right,
                                window,
                                cx,
                            );
                        });
                    }
                },
            ),
        )
    }

    fn dump(&self, _cx: &App) -> PanelState {
        let mut state = PanelState::new(self);
        state.info = PanelInfo::panel(
            serde_json::to_value(ConversationPanelLayout {
                conversation_id: self.id,
                title: self.title.to_string(),
            })
            .unwrap_or(serde_json::Value::Null),
        );
        state
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl EventEmitter<PanelEvent> for ConversationPanel {}

impl Focusable for ConversationPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ConversationPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_message_scroll_anchors();
        let empty = self.messages.is_empty() && !self.pending;
        v_flex()
            .id(("conversation-panel", self.id))
            .size_full()
            .relative()
            .bg(bg_color())
            .track_focus(&self.focus_handle)
            .child(
                div().flex_1().min_h_0().child(
                    div()
                        .id(("conversation-scroll-wrapper", self.id))
                        .size_full()
                        .relative()
                        .child(
                            div()
                                .id(("conversation-scroll", self.id))
                                .size_full()
                                .track_scroll(&self.chat_scroll_handle)
                                .overflow_y_scroll()
                                .child(div().w_full().flex().justify_center().map(|this| {
                                    if empty {
                                        this.child(self.render_empty_view(cx))
                                    } else {
                                        this.child(chat_view::render(self, cx))
                                    }
                                })),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .child(Scrollbar::vertical(&self.chat_scroll_handle)),
                        ),
                ),
            )
            .when(!empty, |this| {
                this.child(
                    div()
                        .px_4()
                        .py_3()
                        .border_t_1()
                        .border_color(border_color())
                        .child(self.render_input_box(cx)),
                )
            })
    }
}
