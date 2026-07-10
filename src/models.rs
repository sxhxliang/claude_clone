//! Core data models for the Claude clone: chat roles/messages, the per-mode
//! conversation type, the persisted `Conversation` record, and app-wide
//! `AppSettings`.
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::SharedString;
use gpui_component::{IconName, dock::DockAreaState};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::chat_view::{ImageAttachment, MessageBlock};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Ai,
}

#[derive(Clone, Copy)]
pub struct TokenUsageStats {
    pub output_tokens: usize,
    pub tokens_per_second: f64,
}

#[derive(Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: SharedString,
    pub thinking: SharedString,
    pub model: SharedString,
    pub mode: ChatMode,
    pub created_at_ms: Option<u64>,
    pub token_stats: Option<TokenUsageStats>,
    pub attachments: Vec<ImageAttachment>,
    /// When present, the AI message renders as a structured block list
    /// (used for `ChatMode::Cowork`). When `None`, falls back to plain `content`.
    pub blocks: Option<Vec<MessageBlock>>,
}

pub(crate) fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[derive(Clone)]
pub struct BranchOrigin {
    pub(crate) source_conversation_id: usize,
    pub(crate) source_title: SharedString,
    pub(crate) message_count: usize,
}

/// Conversation type. Each variant routes to a different (simulated) backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMode {
    Chat,
    Cowork,
    Code,
}

impl ChatMode {
    pub(crate) fn icon(self) -> IconName {
        match self {
            ChatMode::Chat => IconName::Bot,
            ChatMode::Cowork => IconName::SortAscending,
            ChatMode::Code => IconName::SquareTerminal,
        }
    }

    pub(crate) fn from_index(ix: usize) -> Self {
        match ix {
            0 => ChatMode::Chat,
            1 => ChatMode::Cowork,
            _ => ChatMode::Code,
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            ChatMode::Chat => 0,
            ChatMode::Cowork => 1,
            ChatMode::Code => 2,
        }
    }
}

/// Persisted record of a conversation. The live editor is `ConversationPanel`,
/// which syncs its state back into this record via `snapshot()`.
#[derive(Clone)]
pub(crate) struct Conversation {
    pub(crate) id: usize,
    pub(crate) title: SharedString,
    pub(crate) pinned: bool,
    pub(crate) pending: bool,
    pub(crate) project_id: Option<usize>,
    pub(crate) branch_origin: Option<BranchOrigin>,
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) cowork_user_expanded: Vec<bool>,
    pub(crate) tool_expanded: HashMap<(usize, usize), bool>,
}

impl Conversation {
    pub(crate) fn new(
        id: usize,
        title: impl Into<SharedString>,
        messages: Vec<ChatMessage>,
    ) -> Self {
        let message_count = messages.len();
        Self {
            id,
            title: title.into(),
            pinned: false,
            pending: false,
            project_id: None,
            branch_origin: None,
            messages,
            cowork_user_expanded: vec![false; message_count],
            tool_expanded: HashMap::new(),
        }
    }

    pub(crate) fn empty(id: usize, title: impl Into<SharedString>) -> Self {
        Self::new(id, title, Vec::new())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Project {
    pub(crate) id: usize,
    pub(crate) name: String,
}

impl Project {
    pub(crate) fn new(id: usize, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }
}

/// App-wide settings surfaced in the composer and menus.
#[derive(Clone)]
pub(crate) struct AppSettings {
    pub(crate) current_model: SharedString,
    pub(crate) locale: SharedString,
    pub(crate) adaptive_thinking: bool,
    pub(crate) web_search: bool,
    pub(crate) memory_enabled: bool,
    pub(crate) show_typing: bool,
    pub(crate) persist_conversations: bool,
    pub(crate) document_parsing_enabled: bool,
    pub(crate) document_ocr_enabled: bool,
    pub(crate) audio_input_device: SharedString,
    pub(crate) mcp_enabled: bool,
    pub(crate) mcp_server_enabled: HashMap<String, bool>,
    pub(crate) storage_dir: SharedString,
    pub(crate) config_dir: SharedString,
    pub(crate) mode: ChatMode,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            current_model: "".into(),
            locale: crate::i18n::DEFAULT_LOCALE.into(),
            adaptive_thinking: false,
            web_search: true,
            memory_enabled: false,
            show_typing: true,
            persist_conversations: true,
            document_parsing_enabled: true,
            document_ocr_enabled: false,
            audio_input_device: "".into(),
            mcp_enabled: true,
            mcp_server_enabled: HashMap::new(),
            storage_dir: "".into(),
            config_dir: "".into(),
            mode: ChatMode::Chat,
        }
    }
}

/// Provider type. Selects the genai adapter and default endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ProviderKind {
    OpenAI,
    OpenAIResp,
    Gemini,
    Anthropic,
    Fireworks,
    Together,
    Groq,
    Aihubmix,
    Mimo,
    Moonshot,
    Nebius,
    Xai,
    DeepSeek,
    Zai,
    BigModel,
    Aliyun,
    Baidu,
    Cohere,
    Ollama,
    OllamaCloud,
    Vertex,
    GithubCopilot,
    OpenCodeGo,
    BedrockApi,
    OpenRouter,
    MiniMax,
}

impl ProviderKind {
    const ALL: [ProviderKind; 26] = [
        ProviderKind::OpenAI,
        ProviderKind::OpenAIResp,
        ProviderKind::Gemini,
        ProviderKind::Anthropic,
        ProviderKind::Fireworks,
        ProviderKind::Together,
        ProviderKind::Groq,
        ProviderKind::Aihubmix,
        ProviderKind::Mimo,
        ProviderKind::Moonshot,
        ProviderKind::Nebius,
        ProviderKind::Xai,
        ProviderKind::DeepSeek,
        ProviderKind::Zai,
        ProviderKind::BigModel,
        ProviderKind::Aliyun,
        ProviderKind::Baidu,
        ProviderKind::Cohere,
        ProviderKind::Ollama,
        ProviderKind::OllamaCloud,
        ProviderKind::Vertex,
        ProviderKind::GithubCopilot,
        ProviderKind::OpenCodeGo,
        ProviderKind::BedrockApi,
        ProviderKind::OpenRouter,
        ProviderKind::MiniMax,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            ProviderKind::OpenAI => "OpenAI",
            ProviderKind::OpenAIResp => "OpenAI Responses",
            ProviderKind::Gemini => "Gemini",
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::Fireworks => "Fireworks",
            ProviderKind::Together => "Together",
            ProviderKind::Groq => "Groq",
            ProviderKind::Aihubmix => "AIHubMix",
            ProviderKind::Mimo => "Mimo",
            ProviderKind::Moonshot => "Moonshot",
            ProviderKind::Nebius => "Nebius",
            ProviderKind::Xai => "xAI",
            ProviderKind::DeepSeek => "DeepSeek",
            ProviderKind::Zai => "Z.ai",
            ProviderKind::BigModel => "BigModel",
            ProviderKind::Aliyun => "Aliyun",
            ProviderKind::Baidu => "Baidu",
            ProviderKind::Cohere => "Cohere",
            ProviderKind::Ollama => "Ollama",
            ProviderKind::OllamaCloud => "Ollama Cloud",
            ProviderKind::Vertex => "Vertex AI",
            ProviderKind::GithubCopilot => "GitHub Models",
            ProviderKind::OpenCodeGo => "OpenCode Go",
            ProviderKind::BedrockApi => "Bedrock API",
            ProviderKind::OpenRouter => "OpenRouter",
            ProviderKind::MiniMax => "MiniMax",
        }
    }

    pub(crate) fn default_base_url(self) -> String {
        match self {
            ProviderKind::OpenAI => "https://api.openai.com/v1/".to_string(),
            ProviderKind::OpenAIResp => "https://api.openai.com/v1/".to_string(),
            ProviderKind::Gemini => "https://generativelanguage.googleapis.com/v1beta/".to_string(),
            ProviderKind::Anthropic => "https://api.anthropic.com/v1/".to_string(),
            ProviderKind::Fireworks => "https://api.fireworks.ai/inference/v1/".to_string(),
            ProviderKind::Together => "https://api.together.xyz/v1/".to_string(),
            ProviderKind::Groq => "https://api.groq.com/openai/v1/".to_string(),
            ProviderKind::Aihubmix => "https://aihubmix.com/v1/".to_string(),
            ProviderKind::Mimo => "https://api.xiaomimimo.com/v1/".to_string(),
            ProviderKind::Moonshot => "https://api.moonshot.cn/v1/".to_string(),
            ProviderKind::Nebius => "https://api.studio.nebius.ai/v1/".to_string(),
            ProviderKind::Xai => "https://api.x.ai/v1/".to_string(),
            ProviderKind::DeepSeek => "https://api.deepseek.com/v1/".to_string(),
            ProviderKind::Zai => "https://api.z.ai/api/paas/v4/".to_string(),
            ProviderKind::BigModel => "https://open.bigmodel.cn/api/paas/v4/".to_string(),
            ProviderKind::Aliyun => {
                "https://dashscope.aliyuncs.com/compatible-mode/v1/".to_string()
            }
            ProviderKind::Baidu => "https://qianfan.baidubce.com/v2/".to_string(),
            ProviderKind::Cohere => "https://api.cohere.com/v1/".to_string(),
            ProviderKind::Ollama => "http://localhost:11434/".to_string(),
            ProviderKind::OllamaCloud => "https://ollama.com/".to_string(),
            ProviderKind::Vertex => {
                let project_id = std::env::var("VERTEX_PROJECT_ID").unwrap_or_default();
                match std::env::var("VERTEX_LOCATION") {
                    Ok(location) => format!(
                        "https://{location}-aiplatform.googleapis.com/v1/projects/{project_id}/locations/{location}/"
                    ),
                    Err(_) => format!(
                        "https://aiplatform.googleapis.com/v1/projects/{project_id}/locations/global/"
                    ),
                }
            }
            ProviderKind::GithubCopilot => "https://models.github.ai/inference/".to_string(),
            ProviderKind::OpenCodeGo => "https://opencode.ai/zen/go/v1/".to_string(),
            ProviderKind::BedrockApi => {
                let region = std::env::var("AWS_REGION")
                    .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
                    .unwrap_or_else(|_| "us-east-1".to_string());
                format!("https://bedrock-runtime.{region}.amazonaws.com/")
            }
            ProviderKind::OpenRouter => "https://openrouter.ai/api/v1/".to_string(),
            ProviderKind::MiniMax => "https://api.minimax.io/anthropic/v1/".to_string(),
        }
    }

    pub(crate) fn all() -> &'static [ProviderKind] {
        &Self::ALL
    }
}

/// A single model offered by a provider. `selected` means the user added it
/// (via `+`) to the pool of conversation-available models. Display name = id.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ProviderModel {
    pub(crate) id: String,
    pub(crate) selected: bool,
}

/// A configured provider: credentials + (remotely fetched) model list.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Provider {
    pub(crate) id: usize,
    pub(crate) kind: ProviderKind,
    #[serde(default = "provider_enabled_default")]
    pub(crate) enabled: bool,
    pub(crate) api_key: String,
    /// Empty means "use `kind.default_base_url()`".
    pub(crate) base_url: String,
    pub(crate) models: Vec<ProviderModel>,
}

impl Provider {
    /// Base URL actually used for requests: the configured one or the kind
    /// default, always with a trailing `/` (genai concatenates `"{base}models"`).
    pub(crate) fn effective_base_url(&self) -> String {
        let trimmed = self.base_url.trim();
        let url = if trimmed.is_empty() {
            self.kind.default_base_url()
        } else {
            trimmed.to_string()
        };
        if url.ends_with('/') {
            url
        } else {
            format!("{url}/")
        }
    }
}

fn provider_enabled_default() -> bool {
    true
}

/// Points at the model currently driving conversations (one selected model).
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct CurrentModel {
    pub(crate) provider_id: usize,
    pub(crate) model_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ConversationPanelLayout {
    #[serde(alias = "id")]
    pub(crate) conversation_id: usize,
    #[serde(default)]
    pub(crate) title: String,
}

#[derive(Serialize, Deserialize)]
struct StoredImageAttachment {
    title: String,
    url: String,
    detail: String,
    path: Option<std::path::PathBuf>,
    #[serde(default)]
    parsed_text: Option<String>,
    #[serde(default)]
    parse_error: Option<String>,
}

impl From<&ImageAttachment> for StoredImageAttachment {
    fn from(attachment: &ImageAttachment) -> Self {
        Self {
            title: attachment.title.to_string(),
            url: attachment.url.to_string(),
            detail: attachment.detail.to_string(),
            path: attachment.path.clone(),
            parsed_text: attachment.parsed_text.as_ref().map(|text| text.to_string()),
            parse_error: attachment.parse_error.as_ref().map(|err| err.to_string()),
        }
    }
}

impl From<StoredImageAttachment> for ImageAttachment {
    fn from(attachment: StoredImageAttachment) -> Self {
        Self {
            title: attachment.title.into(),
            url: attachment.url.into(),
            detail: attachment.detail.into(),
            path: attachment.path,
            parsed_text: attachment.parsed_text.map(Into::into),
            parse_error: attachment.parse_error.map(Into::into),
        }
    }
}

impl Serialize for ImageAttachment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        StoredImageAttachment::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImageAttachment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StoredImageAttachment::deserialize(deserializer).map(Into::into)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredChatMessage {
    role: ChatRole,
    content: String,
    #[serde(default)]
    thinking: String,
    model: String,
    mode: ChatMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token_stats: Option<StoredTokenUsageStats>,
    #[serde(default)]
    attachments: Vec<ImageAttachment>,
}

#[derive(Serialize, Deserialize)]
struct StoredTokenUsageStats {
    output_tokens: usize,
    tokens_per_second: f64,
}

impl From<&TokenUsageStats> for StoredTokenUsageStats {
    fn from(stats: &TokenUsageStats) -> Self {
        Self {
            output_tokens: stats.output_tokens,
            tokens_per_second: stats.tokens_per_second,
        }
    }
}

impl From<StoredTokenUsageStats> for TokenUsageStats {
    fn from(stats: StoredTokenUsageStats) -> Self {
        Self {
            output_tokens: stats.output_tokens,
            tokens_per_second: stats.tokens_per_second,
        }
    }
}

impl From<&ChatMessage> for StoredChatMessage {
    fn from(message: &ChatMessage) -> Self {
        Self {
            role: message.role.clone(),
            content: message.content.to_string(),
            thinking: message.thinking.to_string(),
            model: message.model.to_string(),
            mode: message.mode,
            created_at_ms: message.created_at_ms,
            token_stats: message.token_stats.as_ref().map(Into::into),
            attachments: message.attachments.clone(),
        }
    }
}

impl From<StoredChatMessage> for ChatMessage {
    fn from(message: StoredChatMessage) -> Self {
        Self {
            role: message.role,
            content: message.content.into(),
            thinking: message.thinking.into(),
            model: message.model.into(),
            mode: message.mode,
            created_at_ms: message.created_at_ms,
            token_stats: message.token_stats.map(Into::into),
            attachments: message.attachments,
            blocks: None,
        }
    }
}

impl Serialize for ChatMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        StoredChatMessage::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ChatMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StoredChatMessage::deserialize(deserializer).map(Into::into)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredBranchOrigin {
    source_conversation_id: usize,
    source_title: String,
    message_count: usize,
}

impl From<&BranchOrigin> for StoredBranchOrigin {
    fn from(origin: &BranchOrigin) -> Self {
        Self {
            source_conversation_id: origin.source_conversation_id,
            source_title: origin.source_title.to_string(),
            message_count: origin.message_count,
        }
    }
}

impl From<StoredBranchOrigin> for BranchOrigin {
    fn from(origin: StoredBranchOrigin) -> Self {
        Self {
            source_conversation_id: origin.source_conversation_id,
            source_title: origin.source_title.into(),
            message_count: origin.message_count,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredToolExpansion {
    message: usize,
    block: usize,
    expanded: bool,
}

impl StoredToolExpansion {
    fn from_entry(entry: (&(usize, usize), &bool)) -> Self {
        let ((message, block), expanded) = entry;
        Self {
            message: *message,
            block: *block,
            expanded: *expanded,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredConversation {
    id: usize,
    title: String,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    project_id: Option<usize>,
    #[serde(default)]
    branch_origin: Option<StoredBranchOrigin>,
    #[serde(default)]
    messages: Vec<ChatMessage>,
    #[serde(default)]
    cowork_user_expanded: Vec<bool>,
    #[serde(default)]
    tool_expanded: Vec<StoredToolExpansion>,
}

impl From<&Conversation> for StoredConversation {
    fn from(conversation: &Conversation) -> Self {
        Self {
            id: conversation.id,
            title: conversation.title.to_string(),
            pinned: conversation.pinned,
            project_id: conversation.project_id,
            branch_origin: conversation
                .branch_origin
                .as_ref()
                .map(StoredBranchOrigin::from),
            messages: conversation.messages.clone(),
            cowork_user_expanded: conversation.cowork_user_expanded.clone(),
            tool_expanded: conversation
                .tool_expanded
                .iter()
                .map(StoredToolExpansion::from_entry)
                .collect(),
        }
    }
}

impl From<StoredConversation> for Conversation {
    fn from(conversation: StoredConversation) -> Self {
        let tool_expanded = conversation
            .tool_expanded
            .into_iter()
            .map(|entry| ((entry.message, entry.block), entry.expanded))
            .collect();
        let mut cowork_user_expanded = conversation.cowork_user_expanded;
        if cowork_user_expanded.len() < conversation.messages.len() {
            cowork_user_expanded.resize(conversation.messages.len(), false);
        }

        Self {
            id: conversation.id,
            title: conversation.title.into(),
            pinned: conversation.pinned,
            pending: false,
            project_id: conversation.project_id,
            branch_origin: conversation.branch_origin.map(Into::into),
            messages: conversation.messages,
            cowork_user_expanded,
            tool_expanded,
        }
    }
}

impl Serialize for Conversation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        StoredConversation::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Conversation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        StoredConversation::deserialize(deserializer).map(Into::into)
    }
}

fn persist_conversations_default() -> bool {
    true
}

fn document_parsing_enabled_default() -> bool {
    true
}

fn document_ocr_enabled_default() -> bool {
    false
}

fn mcp_enabled_default() -> bool {
    true
}

fn locale_default() -> String {
    crate::i18n::DEFAULT_LOCALE.to_string()
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PersistedAppSettings {
    #[serde(default = "locale_default")]
    pub(crate) locale: String,
    #[serde(default = "persist_conversations_default")]
    pub(crate) persist_conversations: bool,
    #[serde(default = "document_parsing_enabled_default")]
    pub(crate) document_parsing_enabled: bool,
    #[serde(default = "document_ocr_enabled_default")]
    pub(crate) document_ocr_enabled: bool,
    #[serde(default)]
    pub(crate) audio_input_device: String,
    #[serde(default = "mcp_enabled_default")]
    pub(crate) mcp_enabled: bool,
    #[serde(default)]
    pub(crate) mcp_server_enabled: HashMap<String, bool>,
    #[serde(default)]
    pub(crate) storage_dir: String,
}

impl Default for PersistedAppSettings {
    fn default() -> Self {
        Self {
            locale: locale_default(),
            persist_conversations: true,
            document_parsing_enabled: true,
            document_ocr_enabled: false,
            audio_input_device: String::new(),
            mcp_enabled: true,
            mcp_server_enabled: HashMap::new(),
            storage_dir: String::new(),
        }
    }
}

/// The slice of app state persisted to disk.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct PersistedState {
    pub(crate) providers: Vec<Provider>,
    pub(crate) next_provider_id: usize,
    pub(crate) current: Option<CurrentModel>,
    #[serde(default)]
    pub(crate) settings: PersistedAppSettings,
    #[serde(default)]
    pub(crate) conversations: Vec<Conversation>,
    #[serde(default)]
    pub(crate) projects: Vec<Project>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) dock_layout: Option<DockAreaState>,
}
