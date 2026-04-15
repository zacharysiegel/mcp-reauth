use std::path::PathBuf;

use serde_json::Value;

use crate::cache;
use crate::config::{ServerConfig, ServerMode};
use crate::error::Error;
use crate::log;
use crate::logging;
use crate::token;

const HOOK_NONCE: &str = "f7a3b1c9-4e82-4d0f-9b6a-2c8e5d1f0a37";

fn release_binary_path() -> Result<String, Error> {
    let current = std::env::current_exe()
        .map_err(|error| Error::from_error_default(Box::new(error)))?;
    let path = current.to_string_lossy();
    let release_path = path.replace("/target/debug/", "/target/release/");
    Ok(release_path)
}

fn settings_path() -> Result<PathBuf, Error> {
    let home = std::env::var("HOME").map_err(|error| Error::from_error_default(Box::new(error)))?;
    Ok(PathBuf::from(&home).join(".claude").join("settings.json"))
}

fn read_settings(path: &PathBuf) -> Result<Value, Error> {
    if path.exists() {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    } else {
        Ok(serde_json::json!({}))
    }
}

fn write_settings(path: &PathBuf, settings: &Value) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(settings)? + "\n")?;
    Ok(())
}

fn command_is_ours(command: &str) -> bool {
    command.contains(&format!("MCP_REAUTH_NONCE={HOOK_NONCE}"))
}

fn remove_our_hooks(entries: &mut Vec<Value>) -> usize {
    let original_len = entries.len();
    entries.retain(|entry| {
        let is_ours = entry
            .get("hooks")
            .and_then(|hooks| hooks.as_array())
            .map(|hooks| {
                hooks.iter().any(|hook| {
                    hook.get("command")
                        .and_then(|c| c.as_str())
                        .map(|c| command_is_ours(c))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        !is_ours
    });
    original_len - entries.len()
}

fn hook_already_installed(settings: &Value, event: &str, command: &str) -> bool {
    settings
        .pointer(&format!("/hooks/{event}"))
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("hooks")
                    .and_then(|hooks| hooks.as_array())
                    .map(|hooks| {
                        hooks
                            .iter()
                            .any(|hook| hook.get("command").and_then(|c| c.as_str()) == Some(command))
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn add_hook(settings: &mut Value, event: &str, matcher: &str, command: &str) -> Result<(), Error> {
    let hooks = settings
        .as_object_mut()
        .ok_or_else(|| Error::new("settings.json is not an object"))?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    hooks
        .as_object_mut()
        .ok_or_else(|| Error::new("hooks is not an object"))?
        .entry(event)
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| Error::new(&format!("{event} is not an array")))?
        .push(serde_json::json!({
            "matcher": matcher,
            "hooks": [{"type": "command", "command": command}],
        }));

    Ok(())
}

fn resolve_server_name(server_config: &ServerConfig) -> Result<String, Error> {
    if let Some(name) = &server_config.server_name {
        return Ok(name.clone());
    }

    if server_config.mode() == ServerMode::Command {
        return Ok(server_config.id.clone());
    }

    if let Some(cached) = cache::read(&server_config.id) {
        return Ok(cached.server_name);
    }

    let keychain_data = crate::keychain::read()
        .unwrap_or_else(|_| serde_json::json!({"mcpOAuth": {}}));
    token::server_name(&keychain_data, server_config.resource_url().unwrap_or(""))
        .ok_or_else(|| Error::new(&format!(
            "[{}] could not determine MCP server name; set server_name in config or authenticate once manually",
            server_config.id,
        )))
}

pub fn install_hook(server_id: Option<&str>) -> Result<(), Error> {
    let config = crate::config::load()?;
    let servers = config.resolve_servers(server_id)?;
    let binary_path = release_binary_path()?;
    let settings_path = settings_path()?;

    if let Some(parent) = std::path::Path::new(logging::STDERR_LOG).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut settings = read_settings(&settings_path)?;

    // Remove all our existing hooks (clean slate for reinstall)
    for event in ["PreToolUse", "SessionStart"] {
        if let Some(entries) = settings
            .pointer_mut(&format!("/hooks/{event}"))
            .and_then(|value| value.as_array_mut())
        {
            remove_our_hooks(entries);
        }
    }

    let log_suffix = format!(
        ">> {} 2>> {} || true",
        logging::STDOUT_LOG,
        logging::STDERR_LOG,
    );

    let nonce_prefix = format!("MCP_REAUTH_NONCE={HOOK_NONCE}");

    let mut installed = Vec::new();

    // One PreToolUse hook per server
    for server_config in &servers {
        let server_name = resolve_server_name(server_config)?;
        let pre_tool_use_matcher = format!("mcp__{server_name}__");

        let pre_tool_use_command = format!(
            "{nonce_prefix} {}=1 {}=PreToolUse {binary_path} --server {} {log_suffix}",
            crate::ENV_HOOK,
            crate::ENV_HOOK_TYPE,
            server_config.id,
        );

        if !hook_already_installed(&settings, "PreToolUse", &pre_tool_use_command) {
            add_hook(&mut settings, "PreToolUse", &pre_tool_use_matcher, &pre_tool_use_command)?;
            installed.push(format!("[{}] PreToolUse (matcher: {pre_tool_use_matcher})", server_config.id));
        }
    }

    // Single SessionStart hook for all servers
    let session_start_command = format!(
        "{nonce_prefix} {}=1 {}=SessionStart {binary_path} {log_suffix}",
        crate::ENV_HOOK,
        crate::ENV_HOOK_TYPE,
    );

    if !hook_already_installed(&settings, "SessionStart", &session_start_command) {
        add_hook(&mut settings, "SessionStart", "", &session_start_command)?;
        installed.push("SessionStart".to_string());
    }

    if installed.is_empty() {
        log!("Hooks already installed.");
        return Ok(());
    }

    write_settings(&settings_path, &settings)?;

    for hook in &installed {
        log!("Installed {hook} hook.");
    }
    Ok(())
}

pub fn uninstall_hook(server_id: Option<&str>) -> Result<(), Error> {
    let settings_path = settings_path()?;
    let mut settings = read_settings(&settings_path)?;

    let mut total_removed = 0;

    if let Some(server_id) = server_id {
        // Remove hooks for a specific server by matching --server <id> in command
        let server_pattern = format!("--server {server_id}");
        for event in ["PreToolUse", "SessionStart"] {
            if let Some(entries) = settings
                .pointer_mut(&format!("/hooks/{event}"))
                .and_then(|value| value.as_array_mut())
            {
                let original_len = entries.len();
                entries.retain(|entry| {
                    let matches = entry
                        .get("hooks")
                        .and_then(|hooks| hooks.as_array())
                        .map(|hooks| {
                            hooks.iter().any(|hook| {
                                let cmd = hook.get("command").and_then(|c| c.as_str()).unwrap_or("");
                                command_is_ours(cmd) && cmd.contains(&server_pattern)
                            })
                        })
                        .unwrap_or(false);
                    !matches
                });
                total_removed += original_len - entries.len();
            }
        }
    } else {
        // Remove all our hooks
        for event in ["PreToolUse", "SessionStart"] {
            if let Some(entries) = settings
                .pointer_mut(&format!("/hooks/{event}"))
                .and_then(|value| value.as_array_mut())
            {
                total_removed += remove_our_hooks(entries);
            }
        }
    }

    if total_removed == 0 {
        log!("No hooks found — nothing to uninstall.");
        return Ok(());
    }

    write_settings(&settings_path, &settings)?;

    log!("Uninstalled {total_removed} hook(s).");
    Ok(())
}

#[cfg(test)]
#[path = "hook_test.rs"]
mod test;
