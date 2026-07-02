//! Minimal MCP client support for Chat mode.
//!
//! The example reads `mcp.json` from the app config directory. The supported
//! shape matches Claude Desktop-style stdio servers:
//!
//! {
//!   "mcpServers": {
//!     "filesystem": {
//!       "command": "npx",
//!       "args": ["-y", "@modelcontextprotocol/server-filesystem", "."],
//!       "env": {}
//!     }
//!   }
//! }
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, CallToolResult, JsonObject, ResourceContents};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt as _};
use serde::Deserialize;
use serde_json::Value;
use tokio::time::timeout;

const MAX_TOOL_OUTPUT_CHARS: usize = 20_000;
const MCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const MCP_TOOL_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct McpTool {
    pub(crate) alias: String,
    pub(crate) server_name: String,
    pub(crate) name: String,
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) input_schema: Value,
}

pub(crate) struct McpToolCallResult {
    pub(crate) output: String,
    pub(crate) is_error: bool,
}

#[derive(Clone)]
pub(crate) struct McpServerInfo {
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) enabled: bool,
    pub(crate) config_enabled: bool,
}

pub(crate) struct McpSession {
    servers: Vec<McpServerConnection>,
    tools: Vec<McpTool>,
}

struct McpServerConnection {
    name: String,
    client: RunningService<RoleClient, ()>,
}

#[derive(Default, Deserialize)]
struct McpConfigFile {
    #[serde(default, rename = "mcpServers", alias = "mcp_servers")]
    mcp_servers: HashMap<String, McpServerConfig>,
}

#[derive(Clone, Deserialize)]
struct McpServerConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    cwd: Option<PathBuf>,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    disabled: bool,
}

impl McpServerConfig {
    fn is_enabled(&self) -> bool {
        self.enabled && !self.disabled
    }

    fn command_label(&self) -> String {
        if self.args.is_empty() {
            self.command.clone()
        } else {
            format!("{} {}", self.command, self.args.join(" "))
        }
    }
}

fn default_enabled() -> bool {
    true
}

impl McpSession {
    pub(crate) async fn connect_from_config(
        path: Option<PathBuf>,
        global_enabled: bool,
        server_enabled: HashMap<String, bool>,
    ) -> Result<Option<Self>, String> {
        let Some(path) = path else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }

        let config = load_config(&path)?;
        if config.mcp_servers.is_empty() {
            return Ok(None);
        }

        let mut session = Self {
            servers: Vec::new(),
            tools: Vec::new(),
        };
        let mut used_aliases = HashSet::new();
        let mut failures = Vec::new();

        for (name, server) in config.mcp_servers {
            if !effective_server_enabled(&name, &server, global_enabled, &server_enabled) {
                continue;
            }

            match timeout(MCP_CONNECT_TIMEOUT, connect_server(&name, server)).await {
                Err(_) => failures.push(format!("{name}: connection timed out")),
                Ok(Ok((connection, server_tools))) => {
                    for tool in server_tools {
                        session
                            .tools
                            .push(mcp_tool_from_rmcp_tool(&name, tool, &mut used_aliases));
                    }
                    session.servers.push(connection);
                }
                Ok(Err(err)) => failures.push(format!("{name}: {err}")),
            }
        }

        if session.servers.is_empty() {
            if failures.is_empty() {
                return Ok(None);
            }
            return Err(format!(
                "No MCP servers connected from {}.\n{}",
                path.display(),
                failures.join("\n")
            ));
        }

        Ok(Some(session))
    }

    pub(crate) fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    pub(crate) fn tool_by_alias(&self, alias: &str) -> Option<&McpTool> {
        self.tools.iter().find(|tool| tool.alias == alias)
    }

    pub(crate) async fn call_tool(
        &self,
        alias: &str,
        arguments: Value,
    ) -> Result<McpToolCallResult, String> {
        let tool = self
            .tool_by_alias(alias)
            .ok_or_else(|| format!("Unknown MCP tool: {alias}"))?;
        let server = self
            .servers
            .iter()
            .find(|server| server.name == tool.server_name)
            .ok_or_else(|| format!("MCP server is not connected: {}", tool.server_name))?;
        let arguments = arguments_object(arguments)?;
        let result = timeout(
            MCP_TOOL_TIMEOUT,
            server.client.peer().call_tool(CallToolRequestParams {
                meta: None,
                name: Cow::Owned(tool.name.clone()),
                arguments: Some(arguments),
                task: None,
            }),
        )
        .await
        .map_err(|_| {
            format!(
                "MCP tool timed out after {} seconds",
                MCP_TOOL_TIMEOUT.as_secs()
            )
        })?
        .map_err(|err| err.to_string())?;

        Ok(McpToolCallResult {
            is_error: result.is_error.unwrap_or(false),
            output: format_call_result(&result),
        })
    }
}

pub(crate) fn configured_servers(
    path: Option<PathBuf>,
    global_enabled: bool,
    server_enabled: &HashMap<String, bool>,
) -> Result<Vec<McpServerInfo>, String> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    if !path.exists() {
        return Ok(Vec::new());
    }

    let config = load_config(&path)?;
    let mut servers = config
        .mcp_servers
        .into_iter()
        .map(|(name, server)| {
            let config_enabled = server.is_enabled();
            let enabled = effective_server_enabled(&name, &server, global_enabled, server_enabled);
            McpServerInfo {
                name,
                command: server.command_label(),
                enabled,
                config_enabled,
            }
        })
        .collect::<Vec<_>>();
    servers.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    Ok(servers)
}

fn effective_server_enabled(
    name: &str,
    server: &McpServerConfig,
    global_enabled: bool,
    server_enabled: &HashMap<String, bool>,
) -> bool {
    server_enabled
        .get(name)
        .copied()
        .unwrap_or_else(|| global_enabled && server.is_enabled())
}

fn load_config(path: &Path) -> Result<McpConfigFile, String> {
    let bytes = std::fs::read(path)
        .map_err(|err| format!("Failed to read MCP config {}: {err}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("Failed to parse MCP config {}: {err}", path.display()))
}

async fn connect_server(
    name: &str,
    server: McpServerConfig,
) -> Result<(McpServerConnection, Vec<rmcp::model::Tool>), String> {
    let mut command = tokio::process::Command::new(&server.command);
    command.args(&server.args);
    command.envs(&server.env);
    if let Some(cwd) = &server.cwd {
        command.current_dir(cwd);
    }

    let transport = TokioChildProcess::new(command).map_err(|err| err.to_string())?;
    let client = ().serve(transport).await.map_err(|err| err.to_string())?;
    let tools = timeout(MCP_CONNECT_TIMEOUT, client.peer().list_all_tools())
        .await
        .map_err(|_| "listing tools timed out".to_string())?
        .map_err(|err| err.to_string())?;

    Ok((
        McpServerConnection {
            name: name.to_string(),
            client,
        },
        tools,
    ))
}

fn mcp_tool_from_rmcp_tool(
    server_name: &str,
    tool: rmcp::model::Tool,
    used_aliases: &mut HashSet<String>,
) -> McpTool {
    let alias = unique_alias(server_name, tool.name.as_ref(), used_aliases);
    let input_schema = tool.schema_as_json_value();
    McpTool {
        alias,
        server_name: server_name.to_string(),
        name: tool.name.to_string(),
        title: tool.title,
        description: tool
            .description
            .as_ref()
            .map(|description| description.clone().into_owned()),
        input_schema,
    }
}

fn unique_alias(server_name: &str, tool_name: &str, used_aliases: &mut HashSet<String>) -> String {
    let server = sanitize_name_part(server_name, 24);
    let tool = sanitize_name_part(tool_name, 34);
    let mut alias = trim_alias(format!("mcp__{server}__{tool}"));
    if used_aliases.insert(alias.clone()) {
        return alias;
    }

    for ix in 2.. {
        let suffix = format!("_{ix}");
        let base = trim_alias(format!("mcp__{server}__{tool}"));
        let max_base_len = 64usize.saturating_sub(suffix.len());
        alias = format!("{}{}", truncate_chars(&base, max_base_len), suffix);
        if used_aliases.insert(alias.clone()) {
            return alias;
        }
    }

    unreachable!("alias generation loop is unbounded")
}

fn sanitize_name_part(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch)
        } else if ch == '_' || ch == '-' || ch == ' ' || ch == '.' {
            Some('_')
        } else {
            None
        };

        let Some(next) = next else {
            continue;
        };
        if next == '_' {
            if last_was_separator {
                continue;
            }
            last_was_separator = true;
        } else {
            last_was_separator = false;
        }
        output.push(next.to_ascii_lowercase());
    }

    let output = output.trim_matches('_');
    let mut output = if output.is_empty() {
        "tool".to_string()
    } else {
        truncate_chars(output, max_chars)
    };
    if output.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        output.insert(0, '_');
    }
    output
}

fn trim_alias(value: String) -> String {
    truncate_chars(&value, 64)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn arguments_object(arguments: Value) -> Result<JsonObject, String> {
    match arguments {
        Value::Object(arguments) => Ok(arguments),
        Value::Null => Ok(JsonObject::default()),
        other => Err(format!(
            "MCP tool arguments must be a JSON object, got {}",
            json_type_name(&other)
        )),
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn format_call_result(result: &CallToolResult) -> String {
    let mut parts = Vec::new();
    for content in &result.content {
        if let Some(text) = content.as_text() {
            if !text.text.trim().is_empty() {
                parts.push(text.text.clone());
            }
        } else if let Some(resource) = content.as_resource() {
            match &resource.resource {
                ResourceContents::TextResourceContents { uri, text, .. } => {
                    parts.push(format!("Resource {uri}:\n{text}"));
                }
                ResourceContents::BlobResourceContents { uri, mime_type, .. } => {
                    parts.push(format!(
                        "Resource {uri}: [{} blob omitted]",
                        mime_type.as_deref().unwrap_or("binary")
                    ));
                }
            }
        } else if let Some(image) = content.as_image() {
            parts.push(format!(
                "[image output omitted: {}, {} base64 characters]",
                image.mime_type,
                image.data.len()
            ));
        } else if let Some(link) = content.as_resource_link() {
            parts.push(format!("Resource link: {} ({})", link.name, link.uri));
        } else {
            parts.push(
                serde_json::to_string(&content.raw)
                    .unwrap_or_else(|_| "Unsupported MCP content".to_string()),
            );
        }
    }

    if let Some(structured) = &result.structured_content {
        parts.push(
            serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string()),
        );
    }

    let output = if parts.is_empty() {
        if result.is_error.unwrap_or(false) {
            "MCP tool returned an error without content.".to_string()
        } else {
            "MCP tool completed without content.".to_string()
        }
    } else {
        parts.join("\n\n")
    };

    truncate_tool_output(&output)
}

fn truncate_tool_output(output: &str) -> String {
    let mut truncated = output.chars().take(MAX_TOOL_OUTPUT_CHARS + 1);
    let value: String = truncated.by_ref().take(MAX_TOOL_OUTPUT_CHARS).collect();
    if truncated.next().is_some() {
        format!("{value}\n\n[truncated]")
    } else {
        value
    }
}
