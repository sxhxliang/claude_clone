//! Real LLM backend for Chat mode, bridged across executors. `genai` (and its
//! `reqwest`) run on a dedicated Tokio runtime; results are forwarded over
//! runtime-agnostic `futures` channels that GPUI's `cx.spawn_in` consumes. Two
//! entry points: `stream_chat` (provider-routed streaming completion) and
//! `list_models` (remote model-name fetch for the provider manager).
use futures::StreamExt as _;
use futures::channel::mpsc::{self, UnboundedReceiver};
use futures::channel::oneshot;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::time::timeout;

use genai::adapter::AdapterKind;
use genai::chat::{
    ChatMessage as GenaiMessage, ChatOptions, ChatRequest, ChatResponse, ChatStreamEvent,
    ContentPart, MessageContent, ReasoningEffort, Tool as GenaiTool, ToolCall as GenaiToolCall,
    ToolResponse as GenaiToolResponse, Usage,
};
use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
use genai::{Client, ModelIden, ServiceTarget};

use crate::mcp_backend::{McpSession, McpTool};
use crate::models::{ChatRole, ProviderKind};

const SYSTEM_PROMPT: &str = "You are Claude, a helpful AI assistant.";
const MAX_MCP_TOOL_ROUNDS: usize = 6;
const CHAT_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const IMAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const MODEL_LIST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) enum StreamMsg {
    Delta(String),
    ReasoningDelta(String),
    ReasoningFinal(String),
    TokenUsage {
        output_tokens: usize,
    },
    ToolStarted {
        title: String,
        input: String,
    },
    ToolFinished {
        title: String,
        output: String,
        is_error: bool,
    },
    Error(String),
}

#[derive(Clone, Default)]
pub(crate) struct GenerationCancel {
    cancelled: Arc<AtomicBool>,
}

impl GenerationCancel {
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

pub(crate) struct StreamHandle {
    pub(crate) receiver: UnboundedReceiver<StreamMsg>,
    pub(crate) cancel: GenerationCancel,
}

fn output_tokens_from_usage(usage: Option<&Usage>) -> Option<usize> {
    let tokens = usage?.completion_tokens?;
    usize::try_from(tokens).ok().filter(|tokens| *tokens > 0)
}

/// Everything a Chat send needs to reach one provider's endpoint. `base_url`
/// is already normalised (non-empty, trailing `/`) by `Provider::effective_base_url`.
pub(crate) struct ChatRoute {
    pub(crate) kind: ProviderKind,
    pub(crate) model_id: String,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
}

#[derive(Clone)]
pub(crate) struct ChatImage {
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) url: String,
    pub(crate) path: Option<PathBuf>,
}

#[derive(Clone)]
pub(crate) struct ChatTurn {
    pub(crate) role: ChatRole,
    pub(crate) content: String,
    pub(crate) images: Vec<ChatImage>,
}

pub(crate) struct GeneratedImageData {
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) detail: String,
}

pub(crate) struct ImageGenerationResult {
    pub(crate) text: String,
    pub(crate) images: Vec<GeneratedImageData>,
}

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| Runtime::new().expect("failed to start tokio runtime"))
}

fn adapter(kind: ProviderKind) -> AdapterKind {
    match kind {
        ProviderKind::OpenAI => AdapterKind::OpenAI,
        ProviderKind::OpenAIResp => AdapterKind::OpenAIResp,
        ProviderKind::Gemini => AdapterKind::Gemini,
        ProviderKind::Anthropic => AdapterKind::Anthropic,
        ProviderKind::Fireworks => AdapterKind::Fireworks,
        ProviderKind::Together => AdapterKind::Together,
        ProviderKind::Groq => AdapterKind::Groq,
        ProviderKind::Aihubmix => AdapterKind::Aihubmix,
        ProviderKind::Mimo => AdapterKind::Mimo,
        ProviderKind::Moonshot => AdapterKind::Moonshot,
        ProviderKind::Nebius => AdapterKind::Nebius,
        ProviderKind::Xai => AdapterKind::Xai,
        ProviderKind::DeepSeek => AdapterKind::DeepSeek,
        ProviderKind::Zai => AdapterKind::Zai,
        ProviderKind::BigModel => AdapterKind::BigModel,
        ProviderKind::Aliyun => AdapterKind::Aliyun,
        ProviderKind::Baidu => AdapterKind::Baidu,
        ProviderKind::Cohere => AdapterKind::Cohere,
        ProviderKind::Ollama => AdapterKind::Ollama,
        ProviderKind::OllamaCloud => AdapterKind::OllamaCloud,
        ProviderKind::Vertex => AdapterKind::Vertex,
        ProviderKind::GithubCopilot => AdapterKind::GithubCopilot,
        ProviderKind::OpenCodeGo => AdapterKind::OpenCodeGo,
        ProviderKind::BedrockApi => AdapterKind::BedrockApi,
        ProviderKind::OpenRouter => AdapterKind::OpenRouter,
        ProviderKind::MiniMax => AdapterKind::MiniMax,
    }
}

fn client_for_route(kind: ProviderKind, base_url: String, api_key: String) -> Client {
    let kind = adapter(kind);
    let resolver = ServiceTargetResolver::from_resolver_fn(
        move |service_target: ServiceTarget| -> Result<ServiceTarget, genai::resolver::Error> {
            let ServiceTarget { model, .. } = service_target;
            Ok(ServiceTarget {
                endpoint: Endpoint::from_owned(base_url.clone()),
                auth: AuthData::from_single(api_key.clone()),
                model: ModelIden::new(kind, model.model_name),
            })
        },
    );

    Client::builder()
        .with_service_target_resolver(resolver)
        .build()
}

fn build_chat_request(history: Vec<ChatTurn>) -> Result<ChatRequest, String> {
    let mut req = ChatRequest::default().with_system(SYSTEM_PROMPT);
    for turn in history {
        req = req.append_message(genai_message_from_turn(turn)?);
    }
    Ok(req)
}

fn genai_message_from_turn(turn: ChatTurn) -> Result<GenaiMessage, String> {
    match turn.role {
        ChatRole::User => Ok(GenaiMessage::user(user_content_from_turn(turn)?)),
        ChatRole::Ai => Ok(GenaiMessage::assistant(turn.content)),
    }
}

fn user_content_from_turn(turn: ChatTurn) -> Result<MessageContent, String> {
    if turn.images.is_empty() {
        return Ok(MessageContent::from(turn.content));
    }

    let mut parts = Vec::new();
    let text = if turn.content.trim().is_empty() {
        "Please respond to the attached image(s).".to_string()
    } else {
        turn.content
    };
    parts.push(ContentPart::from_text(text));

    for image in turn.images {
        let mut label = format!("Attached image: {}", image.title);
        if !image.detail.is_empty() {
            label.push_str(" (");
            label.push_str(&image.detail);
            label.push(')');
        }
        parts.push(ContentPart::from_text(label));
        parts.push(content_part_from_image(image)?);
    }

    Ok(MessageContent::from_parts(parts))
}

fn content_part_from_image(image: ChatImage) -> Result<ContentPart, String> {
    if let Some(path) = image.path {
        return ContentPart::from_binary_file(&path).map_err(|err| {
            format!(
                "Failed to read attached image '{}': {err}",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&image.title)
            )
        });
    }

    if let Some((content_type, content)) = parse_base64_data_url(&image.url) {
        return Ok(ContentPart::from_binary_base64(
            content_type,
            content,
            Some(image.title),
        ));
    }

    if image.url.trim().is_empty() {
        return Err(format!(
            "Attached image '{}' has no readable source",
            image.title
        ));
    }

    Ok(ContentPart::from_binary_url(
        image_content_type(&image.url),
        image.url,
        Some(image.title),
    ))
}

fn parse_base64_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (header, content) = rest.split_once(',')?;
    if !header
        .split(';')
        .any(|part| part.eq_ignore_ascii_case("base64"))
    {
        return None;
    }
    let content_type = header
        .split(';')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("application/octet-stream");
    Some((content_type.to_string(), content.to_string()))
}

fn image_content_type(value: &str) -> &'static str {
    let lower = value
        .split(['?', '#'])
        .next()
        .unwrap_or(value)
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match lower.as_str() {
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Fetch the provider's available model ids via a remote GET `/models`.
pub(crate) fn list_models(
    kind: ProviderKind,
    base_url: String,
    api_key: String,
) -> oneshot::Receiver<Result<Vec<String>, String>> {
    let (tx, rx) = oneshot::channel();
    runtime().spawn(async move {
        let result = timeout(
            MODEL_LIST_TIMEOUT,
            Client::default().all_model_names(
                adapter(kind),
                (
                    Endpoint::from_owned(base_url),
                    AuthData::from_single(api_key),
                ),
            ),
        )
        .await
        .map_err(|_| "Timed out while fetching model list.".to_string())
        .and_then(|result| result.map_err(|err| err.to_string()));
        let _ = tx.send(result);
    });
    rx
}

/// Stream a chat completion through a specific provider (forced adapter +
/// endpoint + key), returning a receiver of text deltas. The sender drops when
/// the stream ends, which the UI reads as completion.
pub(crate) fn stream_chat(
    route: ChatRoute,
    history: Vec<ChatTurn>,
    mcp_config_path: Option<PathBuf>,
    mcp_enabled: bool,
    mcp_server_enabled: HashMap<String, bool>,
    adaptive_thinking: bool,
) -> StreamHandle {
    let (tx, rx) = mpsc::unbounded();
    let cancel = GenerationCancel::default();
    let task_cancel = cancel.clone();
    runtime().spawn(async move {
        let mut req = match build_chat_request(history) {
            Ok(req) => req,
            Err(err) => {
                let _ = tx.unbounded_send(StreamMsg::Error(err));
                return;
            }
        };

        let model_id = route.model_id;
        let client = client_for_route(route.kind, route.base_url, route.api_key);
        let mut options = ChatOptions::default()
            .with_capture_usage(true)
            .with_capture_reasoning_content(true)
            .with_normalize_reasoning_content(true);
        if adaptive_thinking {
            options = options.with_reasoning_effort(ReasoningEffort::Medium);
        }

        let mcp_session =
            match McpSession::connect_from_config(mcp_config_path, mcp_enabled, mcp_server_enabled)
                .await
            {
                Ok(Some(session)) if !session.tools().is_empty() => {
                    req = req.with_tools(session.tools().iter().map(genai_tool_from_mcp));
                    Some(session)
                }
                Ok(_) => None,
                Err(err) => {
                    let _ = tx.unbounded_send(StreamMsg::Error(format!("MCP: {err}")));
                    return;
                }
            };
        if mcp_session.is_some() {
            options = options
                .with_capture_content(true)
                .with_capture_tool_calls(true);
        }

        let mut total_output_tokens = 0usize;
        for round in 0..=MAX_MCP_TOOL_ROUNDS {
            if task_cancel.is_cancelled() {
                return;
            }
            let mut stream_end = None;
            match timeout(
                CHAT_REQUEST_TIMEOUT,
                client.exec_chat_stream(model_id.as_str(), req.clone(), Some(&options)),
            )
            .await
            {
                Err(_) => {
                    let _ = tx.unbounded_send(StreamMsg::Error(
                        "Chat request timed out. Try again or use a smaller prompt.".to_string(),
                    ));
                    return;
                }
                Ok(resp) => {
                    let resp = match resp {
                        Ok(resp) => resp,
                        Err(err) => {
                            let _ = tx.unbounded_send(StreamMsg::Error(err.to_string()));
                            return;
                        }
                    };
                    let mut stream = resp.stream;
                    while let Some(event) = stream.next().await {
                        if task_cancel.is_cancelled() {
                            return;
                        }
                        match event {
                            Ok(ChatStreamEvent::Chunk(chunk)) => {
                                if tx.unbounded_send(StreamMsg::Delta(chunk.content)).is_err() {
                                    return; // receiver dropped (panel closed)
                                }
                            }
                            Ok(ChatStreamEvent::ReasoningChunk(chunk)) => {
                                if tx
                                    .unbounded_send(StreamMsg::ReasoningDelta(chunk.content))
                                    .is_err()
                                {
                                    return; // receiver dropped (panel closed)
                                }
                            }
                            Ok(ChatStreamEvent::End(end)) => {
                                if let Some(reasoning) = end.captured_reasoning_content.as_deref() {
                                    if !reasoning.is_empty()
                                        && tx
                                            .unbounded_send(StreamMsg::ReasoningFinal(
                                                reasoning.to_string(),
                                            ))
                                            .is_err()
                                    {
                                        return; // receiver dropped (panel closed)
                                    }
                                }
                                if let Some(output_tokens) =
                                    output_tokens_from_usage(end.captured_usage.as_ref())
                                {
                                    total_output_tokens =
                                        total_output_tokens.saturating_add(output_tokens);
                                    if tx
                                        .unbounded_send(StreamMsg::TokenUsage {
                                            output_tokens: total_output_tokens,
                                        })
                                        .is_err()
                                    {
                                        return; // receiver dropped (panel closed)
                                    }
                                }
                                stream_end = Some(end);
                            }
                            Ok(_) => {}
                            Err(err) => {
                                let _ = tx.unbounded_send(StreamMsg::Error(err.to_string()));
                                return;
                            }
                        }
                    }
                }
            };

            let Some(session) = mcp_session.as_ref() else {
                return;
            };
            let Some(end) = stream_end else {
                return;
            };
            let tool_calls = end
                .captured_tool_calls()
                .unwrap_or_default()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            if tool_calls.is_empty() {
                return;
            }
            if round == MAX_MCP_TOOL_ROUNDS {
                let _ = tx.unbounded_send(StreamMsg::Error(
                    "Stopped after too many MCP tool-call rounds.".to_string(),
                ));
                return;
            }

            if let Some(content) = end.captured_content {
                req = req.append_message(GenaiMessage::assistant(content));
            } else {
                req = req.append_message(tool_calls.clone());
            }

            let mut tool_responses = Vec::new();
            for tool_call in tool_calls {
                if task_cancel.is_cancelled() {
                    return;
                }
                tool_responses.push(run_mcp_tool_call(session, &tool_call, &tx).await);
            }
            req = req.append_message(tool_responses);
        }
    });
    StreamHandle {
        receiver: rx,
        cancel,
    }
}

fn genai_tool_from_mcp(tool: &McpTool) -> GenaiTool {
    let description = mcp_tool_description(tool);
    GenaiTool::new(tool.alias.clone())
        .with_description(description)
        .with_schema(tool.input_schema.clone())
}

fn mcp_tool_description(tool: &McpTool) -> String {
    let mut description = tool
        .description
        .clone()
        .or_else(|| tool.title.clone())
        .unwrap_or_else(|| format!("MCP tool `{}`.", tool.name));
    description.push_str(&format!(
        "\nMCP server: {}. Original tool name: {}.",
        tool.server_name, tool.name
    ));
    description
}

async fn run_mcp_tool_call(
    session: &McpSession,
    tool_call: &GenaiToolCall,
    tx: &mpsc::UnboundedSender<StreamMsg>,
) -> GenaiToolResponse {
    let title = session
        .tool_by_alias(&tool_call.fn_name)
        .map(mcp_tool_title)
        .unwrap_or_else(|| format!("MCP tool {}", tool_call.fn_name));
    let input = serde_json::to_string_pretty(&tool_call.fn_arguments)
        .unwrap_or_else(|_| tool_call.fn_arguments.to_string());
    let _ = tx.unbounded_send(StreamMsg::ToolStarted {
        title: title.clone(),
        input,
    });

    let (output, is_error) = match session
        .call_tool(&tool_call.fn_name, tool_call.fn_arguments.clone())
        .await
    {
        Ok(result) => (result.output, result.is_error),
        Err(err) => (format!("MCP tool error: {err}"), true),
    };

    let _ = tx.unbounded_send(StreamMsg::ToolFinished {
        title,
        output: output.clone(),
        is_error,
    });

    GenaiToolResponse::from_tool_call(tool_call, output)
}

fn mcp_tool_title(tool: &McpTool) -> String {
    match tool.title.as_deref() {
        Some(title) if !title.trim().is_empty() => {
            format!("{} · {}", tool.server_name, title)
        }
        _ => format!("{} · {}", tool.server_name, tool.name),
    }
}

pub(crate) fn generate_images(
    route: ChatRoute,
    history: Vec<ChatTurn>,
) -> oneshot::Receiver<Result<ImageGenerationResult, String>> {
    let (tx, rx) = oneshot::channel();
    runtime().spawn(async move {
        let result = async move {
            let req = build_chat_request(history)?;
            let model_id = route.model_id;
            let client = client_for_route(route.kind, route.base_url, route.api_key);
            let response = timeout(
                IMAGE_REQUEST_TIMEOUT,
                client.exec_chat(model_id.as_str(), req, None),
            )
            .await
            .map_err(|_| "Image generation timed out.".to_string())?
            .map_err(|err| err.to_string())?;
            Ok(image_generation_result(response))
        }
        .await;
        let _ = tx.send(result);
    });
    rx
}

fn image_generation_result(response: ChatResponse) -> ImageGenerationResult {
    let mut text_parts = Vec::new();
    let mut images = Vec::new();

    for part in response.content.into_parts() {
        match part {
            ContentPart::Text(text) => {
                if !text.trim().is_empty() {
                    text_parts.push(text);
                }
            }
            ContentPart::Binary(binary) if binary.is_image() => {
                let title = binary
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Generated image {}", images.len() + 1));
                let detail = binary.content_type.clone();
                let url = binary.into_url();
                images.push(GeneratedImageData { title, url, detail });
            }
            _ => {}
        }
    }

    let text = if text_parts.is_empty() {
        if images.is_empty() {
            "No image data returned by the model.".to_string()
        } else {
            "Generated image.".to_string()
        }
    } else {
        text_parts.join("\n\n")
    };

    ImageGenerationResult { text, images }
}
