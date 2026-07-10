//! JSON persistence for the Claude clone example. A missing or corrupt file
//! yields defaults, while save paths return errors so the UI can surface failed
//! writes. Note: provider API keys are stored in plaintext, which is acceptable
//! for this example. The filename is historical; it now also stores dock layout
//! state.
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use gpui_component::dock::DockAreaState;

use crate::models::{Conversation, PersistedState, Project};

const APP_DIR: &str = "claude_clone";
const CONFIG_FILE: &str = "providers.json";
const MCP_CONFIG_FILE: &str = "mcp.json";
const HISTORY_FILE: &str = "conversations.json";
const LOCATION_FILE: &str = "locations.json";

#[derive(Default, Serialize, Deserialize)]
struct StoreLocations {
    config_dir: Option<PathBuf>,
}

#[derive(Default, Serialize, Deserialize)]
struct ConversationStore {
    conversations: Vec<Conversation>,
    #[serde(default)]
    projects: Vec<Project>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dock_layout: Option<DockAreaState>,
}

fn default_config_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_DIR))
}

pub(crate) fn default_storage_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(APP_DIR))
}

fn location_path() -> Option<PathBuf> {
    Some(default_config_dir()?.join(LOCATION_FILE))
}

fn load_locations() -> StoreLocations {
    let Some(path) = location_path() else {
        return StoreLocations::default();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return StoreLocations::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn write_json_pretty<T: Serialize>(path: PathBuf, value: &T) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(value).map_err(|err| {
        crate::tr!("errors.serialize_config_failed", error = err.to_string()).to_string()
    })?;
    write_atomic(path, json)
}

/// Write arbitrary bytes to `path` atomically. Exposed for the file-export flow.
pub(crate) fn write_bytes(path: PathBuf, bytes: Vec<u8>) -> Result<(), String> {
    write_atomic(path, bytes)
}

fn write_atomic(path: PathBuf, bytes: Vec<u8>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            crate::tr!(
                "errors.create_dir_failed",
                path = parent.display().to_string(),
                error = err.to_string()
            )
            .to_string()
        })?;
    }

    let tmp = path.with_extension(format!(
        "tmp-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::write(&tmp, bytes).map_err(|err| {
        crate::tr!(
            "errors.write_tmp_failed",
            path = tmp.display().to_string(),
            error = err.to_string()
        )
        .to_string()
    })?;
    std::fs::rename(&tmp, &path).map_err(|err| {
        let _ = std::fs::remove_file(&tmp);
        crate::tr!(
            "errors.save_file_failed",
            path = path.display().to_string(),
            error = err.to_string()
        )
        .to_string()
    })
}

pub(crate) fn ensure_writable_dir(path: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|err| {
        crate::tr!(
            "errors.create_dir_failed",
            path = path.display().to_string(),
            error = err.to_string()
        )
        .to_string()
    })?;
    let probe = path.join(format!(
        ".claude_clone_write_test_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::write(&probe, b"ok").map_err(|err| {
        crate::tr!(
            "errors.dir_not_writable",
            path = path.display().to_string(),
            error = err.to_string()
        )
        .to_string()
    })?;
    std::fs::remove_file(&probe).map_err(|err| {
        crate::tr!(
            "errors.cleanup_probe_failed",
            path = probe.display().to_string(),
            error = err.to_string()
        )
        .to_string()
    })
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    load_locations().config_dir.or_else(default_config_dir)
}

pub(crate) fn set_config_dir(path: PathBuf) -> Result<(), String> {
    ensure_writable_dir(&path)?;
    write_json_pretty(
        location_path()
            .ok_or_else(|| crate::tr!("errors.default_config_dir_unavailable").to_string())?,
        &StoreLocations {
            config_dir: Some(path),
        },
    )
}

pub(crate) fn reset_config_dir() -> Result<(), String> {
    let dir = default_config_dir()
        .ok_or_else(|| crate::tr!("errors.default_config_dir_unavailable").to_string())?;
    ensure_writable_dir(&dir)?;
    write_json_pretty(
        location_path()
            .ok_or_else(|| crate::tr!("errors.default_config_dir_unavailable").to_string())?,
        &StoreLocations { config_dir: None },
    )
}

fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join(CONFIG_FILE))
}

pub(crate) fn mcp_config_path() -> Option<PathBuf> {
    Some(config_dir()?.join(MCP_CONFIG_FILE))
}

pub(crate) fn default_mcp_config_text() -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "mcpServers": {}
    }))
    .unwrap_or_else(|_| "{\n  \"mcpServers\": {}\n}".to_string())
}

pub(crate) fn load_mcp_config_text() -> Result<String, String> {
    let Some(path) = mcp_config_path() else {
        return Err(crate::tr!("errors.config_dir_unavailable").to_string());
    };
    if !path.exists() {
        return Ok(default_mcp_config_text());
    }
    std::fs::read_to_string(&path).map_err(|err| {
        crate::tr!(
            "errors.read_mcp_failed",
            path = path.display().to_string(),
            error = err.to_string()
        )
        .to_string()
    })
}

pub(crate) fn save_mcp_config_text(text: &str) -> Result<(PathBuf, String), String> {
    let value = serde_json::from_str::<serde_json::Value>(text).map_err(|err| {
        crate::tr!("errors.mcp_json_invalid", error = err.to_string()).to_string()
    })?;
    if !value.is_object() {
        return Err(crate::tr!("errors.mcp_must_be_object").to_string());
    }
    let formatted = serde_json::to_string_pretty(&value).map_err(|err| {
        crate::tr!("errors.format_mcp_failed", error = err.to_string()).to_string()
    })?;
    let Some(path) = mcp_config_path() else {
        return Err(crate::tr!("errors.config_dir_unavailable").to_string());
    };
    write_atomic(path.clone(), formatted.as_bytes().to_vec())
        .map_err(|err| crate::tr!("errors.save_mcp_failed", error = err).to_string())?;
    Ok((path, formatted))
}

fn storage_dir_from_setting(path: &str) -> Option<PathBuf> {
    if path.trim().is_empty() {
        default_storage_dir()
    } else {
        Some(PathBuf::from(path))
    }
}

fn history_path(path: &str) -> Option<PathBuf> {
    Some(storage_dir_from_setting(path)?.join(HISTORY_FILE))
}

fn load_conversations(path: &str) -> Option<ConversationStore> {
    let bytes = std::fs::read(history_path(path)?).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_conversations_result(path: &str, conversations: &ConversationStore) -> Result<(), String> {
    let path = history_path(path)
        .ok_or_else(|| crate::tr!("errors.storage_dir_unavailable").to_string())?;
    write_json_pretty(path, conversations)
}

pub(crate) fn load() -> PersistedState {
    let Some(path) = config_path() else {
        return PersistedState::default();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return PersistedState::default();
    };
    let mut state = serde_json::from_slice::<PersistedState>(&bytes).unwrap_or_default();
    if state.settings.persist_conversations {
        if let Some(history) = load_conversations(&state.settings.storage_dir) {
            state.conversations = history.conversations;
            state.projects = history.projects;
            state.dock_layout = history.dock_layout;
        }
    } else {
        state.conversations.clear();
        state.projects.clear();
        state.dock_layout = None;
    }
    state
}

pub(crate) fn save(state: &PersistedState) -> Result<(), String> {
    let path =
        config_path().ok_or_else(|| crate::tr!("errors.config_dir_unavailable").to_string())?;
    let config_state = PersistedState {
        providers: state.providers.clone(),
        next_provider_id: state.next_provider_id,
        current: state.current.clone(),
        settings: state.settings.clone(),
        conversations: Vec::new(),
        projects: Vec::new(),
        dock_layout: None,
    };
    write_json_pretty(path, &config_state)?;

    if state.settings.persist_conversations {
        save_conversations_result(
            &state.settings.storage_dir,
            &ConversationStore {
                conversations: state.conversations.clone(),
                projects: state.projects.clone(),
                dock_layout: state.dock_layout.clone(),
            },
        )?;
    }
    Ok(())
}

pub(crate) fn clear_saved_conversations(path: &str) -> Result<(), String> {
    let path = history_path(path)
        .ok_or_else(|| crate::tr!("errors.storage_dir_unavailable").to_string())?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(crate::tr!(
            "errors.clear_history_failed",
            path = path.display().to_string(),
            error = err.to_string()
        )
        .to_string()),
    }
}
