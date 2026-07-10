//! File export: render a conversation or a single message to Markdown / HTML /
//! plain text and save it to disk via a native "Save As" dialog, plus resolve
//! image-artifact bytes for saving. Reuses `store::write_bytes` for atomic
//! writes and `system_file::reveal` to surface the saved file afterwards.
//!
//! Bodies are taken from `ChatMessage.content` (the plain-text reply, or the
//! Cowork summary line). Cowork's rich block content lives only inside
//! `TextViewState` entities whose source text is not exposed by the published
//! `gpui-component` API, so it is intentionally omitted here.
use std::path::PathBuf;

use base64::Engine as _;
use gpui::{Context, Window};
use gpui_component::{WindowExt as _, notification::Notification};

use crate::chat_view::ImageAttachment;
use crate::models::{ChatMessage, ChatMode, ChatRole};

/// Export target format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    Markdown,
    Html,
    Text,
}

impl ExportFormat {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Html => "html",
            ExportFormat::Text => "txt",
        }
    }
}

/// Render a whole conversation to the requested format.
pub(crate) fn render_conversation(
    title: &str,
    messages: &[ChatMessage],
    format: ExportFormat,
) -> String {
    let heading = document_title(title);
    let mut body = format!("# {heading}\n\n");
    for message in messages {
        body.push_str(&message_section(message));
        body.push_str("\n\n");
    }
    finalize(body.trim_end(), &heading, format)
}

/// Render a single message to the requested format.
pub(crate) fn render_message(message: &ChatMessage, format: ExportFormat) -> String {
    finalize(&message_section(message), &role_label(message), format)
}

fn document_title(title: &str) -> String {
    if title.trim().is_empty() {
        crate::tr!("conversation.untitled_short").to_string()
    } else {
        title.trim().to_string()
    }
}

fn message_section(message: &ChatMessage) -> String {
    format!(
        "## {}\n\n{}",
        message_header(message),
        message.content.trim()
    )
}

fn message_header(message: &ChatMessage) -> String {
    match message.role {
        ChatRole::User => crate::tr!("search.you").to_string(),
        ChatRole::Ai => format!(
            "{} · {} · {}",
            crate::tr!("search.claude"),
            message.model,
            mode_label(message.mode)
        ),
    }
}

fn role_label(message: &ChatMessage) -> String {
    match message.role {
        ChatRole::User => crate::tr!("search.you").to_string(),
        ChatRole::Ai => crate::tr!("search.claude").to_string(),
    }
}

fn mode_label(mode: ChatMode) -> &'static str {
    match mode {
        ChatMode::Chat => "Chat",
        ChatMode::Cowork => "Cowork",
        ChatMode::Code => "Code",
    }
}

fn finalize(markdown_body: &str, title: &str, format: ExportFormat) -> String {
    match format {
        // `.txt` keeps the Markdown text verbatim (readable as plain text);
        // proper Markdown-stripping is intentionally out of scope.
        ExportFormat::Markdown | ExportFormat::Text => format!("{markdown_body}\n"),
        ExportFormat::Html => to_html(markdown_body, title),
    }
}

fn to_html(markdown_body: &str, title: &str) -> String {
    let rendered = markdown::to_html(markdown_body);
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{title}</title>\n<style>{HTML_CSS}</style>\n</head>\n<body>\n{rendered}\n</body>\n</html>\n",
        title = html_escape(title),
    )
}

const HTML_CSS: &str = "body{max-width:52rem;margin:2.5rem auto;padding:0 1.25rem;\
font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,'PingFang SC','Microsoft YaHei',sans-serif;\
line-height:1.7;color:#1f2328}h1,h2,h3{line-height:1.3}\
h2{margin-top:2rem;border-bottom:1px solid #e5e7eb;padding-bottom:.3rem}\
pre{background:#f6f8fa;padding:1rem;border-radius:8px;overflow:auto}\
code{background:#f6f8fa;padding:.15em .35em;border-radius:4px}pre code{background:none;padding:0}\
blockquote{color:#57606a;border-left:.25rem solid #d0d7de;margin:0;padding:0 1rem}\
table{border-collapse:collapse}th,td{border:1px solid #d0d7de;padding:.4rem .75rem}img{max-width:100%}";

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Sanitize a title into a safe file stem (no path separators, bounded length).
pub(crate) fn sanitize_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let stem = if trimmed.is_empty() {
        crate::tr!("conversation.untitled_short").to_string()
    } else {
        trimmed.to_string()
    };
    stem.chars().take(60).collect()
}

/// Suggested filename `<sanitized-title>.<ext>`.
pub(crate) fn suggested_filename(title: &str, format: ExportFormat) -> String {
    format!("{}.{}", sanitize_filename(title), format.extension())
}

/// Resolve the exportable `(filename, bytes)` for an image attachment.
///
/// Uploaded images are read from disk; model-generated images are decoded from
/// their `data:` URL. Remote http(s) images return `None` (they are not
/// downloaded).
pub(crate) fn image_export_target(attachment: &ImageAttachment) -> Option<(String, Vec<u8>)> {
    if let Some(path) = attachment.path.as_ref() {
        let bytes = std::fs::read(path).ok()?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| sanitize_filename(&attachment.title));
        return Some((name, bytes));
    }

    let (content_type, encoded) = crate::genai_backend::parse_base64_data_url(&attachment.url)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .ok()?;
    let ext = image_extension(&content_type);
    Some((
        format!("{}.{ext}", sanitize_filename(&attachment.title)),
        bytes,
    ))
}

fn image_extension(content_type: &str) -> &'static str {
    match content_type.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "image/tiff" => "tiff",
        "image/avif" => "avif",
        _ => "png",
    }
}

fn default_export_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(crate::store::default_storage_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Show a native "Save As" dialog, then write `bytes` to the chosen path on a
/// background thread, reveal it, and notify. Generic over the calling entity so
/// conversation, message, and artifact call sites can all reuse it.
pub(crate) fn save_bytes<T: 'static>(
    suggested_name: String,
    bytes: Vec<u8>,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    let directory = default_export_dir();
    let path_rx = cx.prompt_for_new_path(&directory, Some(suggested_name.as_str()));
    cx.spawn_in(window, async move |entity, cx| {
        let Ok(Ok(Some(path))) = path_rx.await else {
            return;
        };
        let write = cx
            .background_executor()
            .spawn(async move { crate::store::write_bytes(path.clone(), bytes).map(|()| path) })
            .await;
        let Some(entity) = entity.upgrade() else {
            return;
        };
        let _ = entity.update_in(cx, |_, window, cx| match write {
            Ok(path) => {
                let _ = crate::system_file::reveal(&path);
                window.push_notification(Notification::info(crate::tr!("export.saved")), cx);
            }
            Err(err) => {
                window.push_notification(Notification::error(err), cx);
            }
        });
    })
    .detach();
}
