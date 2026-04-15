use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cache;
use crate::config::{ResolvedServerConfig, ServerConfig, ServerMode};
use crate::error::Error;
use crate::launchd;
use crate::log;
use crate::oauth;

const EXPIRY_BUFFER_SECONDS: u64 = 3600;

pub(crate) fn server_name(data: &Value, resource_url: &str) -> Option<String> {
    let mcp_oauth = data.pointer("/mcpOAuth")?.as_object()?;
    let (key, _) = mcp_oauth.iter().find(|(_, entry)| {
        entry
            .get("serverUrl")
            .and_then(|value| value.as_str())
            .map(|url| url == resource_url)
            .unwrap_or(false)
    })?;

    key.split('|').next().map(str::to_owned)
}

fn find_server_key(data: &Value, resource_url: &str) -> Option<String> {
    let name = server_name(data, resource_url)?;
    let prefix = format!("{name}|");
    data.pointer("/mcpOAuth")?
        .as_object()?
        .keys()
        .find(|key| key.starts_with(&prefix))
        .map(String::to_owned)
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn token_needs_refresh(server_config: &ServerConfig, data: &Value) -> bool {
    let resource_url = match server_config.resource_url() {
        Some(url) => url,
        None => {
            log!("[{}] No resource URL configured.", server_config.id);
            return true;
        }
    };
    let server_key = match find_server_key(data, resource_url) {
        Some(key) => key,
        None => {
            log!("[{}] No MCP entry found in keychain.", server_config.id);
            return true;
        }
    };

    let server_entry = data
        .pointer("/mcpOAuth")
        .and_then(|mcp_oauth| mcp_oauth.get(server_key.as_str()));

    let has_token = server_entry
        .and_then(|entry| entry.get("accessToken"))
        .and_then(|value| value.as_str())
        .is_some_and(|token| !token.is_empty());

    if !has_token {
        log!("[{}] No access token found.", server_config.id);
        return true;
    }

    let expires_at_ms = server_entry
        .and_then(|entry| entry.get("expiresAt"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);

    let remaining_seconds = expires_at_ms.saturating_sub(now_epoch_ms()) / 1000;

    if remaining_seconds >= EXPIRY_BUFFER_SECONDS {
        log!("[{}] Token still valid ({} minutes remaining).", server_config.id, remaining_seconds / 60);
        if let Some(name) = resolve_server_name_from_config(server_config, data) {
            cache::write(&server_config.id, &name, expires_at_ms);
        }
        return false;
    }

    log!("[{}] Token expires in {} minutes.", server_config.id, remaining_seconds / 60);
    true
}

fn resolve_server_name_from_config(server_config: &ServerConfig, data: &Value) -> Option<String> {
    server_config
        .server_name
        .clone()
        .or_else(|| server_name(data, server_config.resource_url()?))
}

fn resolve_server_name_from_resolved(resolved: &ResolvedServerConfig, data: &Value) -> Option<String> {
    resolved
        .server_name
        .clone()
        .or_else(|| server_name(data, &resolved.resource))
}

fn log_token_refreshed(id: &str, expires_at_ms: u64) {
    let remaining_seconds = expires_at_ms.saturating_sub(now_epoch_ms()) / 1000;
    let minutes = remaining_seconds / 60;
    let seconds = remaining_seconds % 60;
    log!("[{id}] Token refreshed successfully. Expires in {minutes}m:{seconds:02}s.");
}

fn update_keychain_data(
    resolved: &ResolvedServerConfig,
    data: &mut Value,
    token_response: &Value,
) -> Result<(), Error> {
    let access_token = token_response
        .get("access_token")
        .and_then(|value| value.as_str())
        .ok_or_else(|| Error::new("no access_token in response"))?;

    let expires_in = token_response
        .get("expires_in")
        .and_then(|value| value.as_u64())
        .unwrap_or(28800);

    let expires_at_ms = now_epoch_ms() + (expires_in * 1000);

    let name = resolve_server_name_from_resolved(resolved, data)
        .ok_or_else(|| Error::new(&format!("[{}] could not determine MCP server name", resolved.id)))?;

    let mcp_oauth = data
        .as_object_mut()
        .ok_or_else(|| Error::new("keychain data is not an object"))?
        .entry("mcpOAuth")
        .or_insert_with(|| serde_json::json!({}));

    let server_key = mcp_oauth
        .as_object()
        .and_then(|object| {
            let prefix = format!("{name}|");
            object.keys().find(|key| key.starts_with(&prefix)).map(String::to_owned)
        })
        .unwrap_or_else(|| format!("{name}|new"));

    mcp_oauth[server_key] = serde_json::json!({
        "serverName": name,
        "serverUrl": resolved.resource,
        "accessToken": access_token,
        "expiresAt": expires_at_ms,
        "discoveryState": {
            "authorizationServerUrl": resolved.authorization_server_url,
        },
        "clientId": resolved.client_id,
    });

    log_token_refreshed(&resolved.id, expires_at_ms);
    cache::write(&resolved.id, &name, expires_at_ms);
    Ok(())
}

pub fn invalidate_token(server_id: Option<&str>) -> Result<(), Error> {
    let config = crate::config::load()?;
    let servers = config.resolve_servers(server_id)?;

    let (command_servers, oauth_servers): (Vec<_>, Vec<_>) = servers
        .into_iter()
        .partition(|server| server.mode() == ServerMode::Command);

    for server_config in &command_servers {
        cache::remove(&server_config.id);
        log!("[{}] Cache invalidated (command mode).", server_config.id);
    }

    if !oauth_servers.is_empty() {
        let mut data = crate::keychain::read()?;
        let mut invalidated = 0;

        for server_config in &oauth_servers {
            let resource_url = match server_config.resource_url() {
                Some(url) => url,
                None => {
                    log!("[{}] No resource URL configured — skipping.", server_config.id);
                    continue;
                }
            };
            let key = match find_server_key(&data, resource_url) {
                Some(key) => key,
                None => {
                    log!("[{}] No MCP entry found in keychain — skipping.", server_config.id);
                    continue;
                }
            };

            let entry = match data.pointer_mut(&format!("/mcpOAuth/{key}")) {
                Some(entry) => entry,
                None => continue,
            };

            entry["expiresAt"] = serde_json::json!(0);
            cache::remove(&server_config.id);
            log!("[{}] Token invalidated.", server_config.id);
            invalidated += 1;
        }

        if invalidated > 0 {
            crate::keychain::write(&data)?;
        }
    }

    Ok(())
}

fn decode_jwt_exp(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload)
        .or_else(|_| {
            use base64::engine::general_purpose::STANDARD;
            STANDARD.decode(payload)
        })
        .ok()?;
    let claims: Value = serde_json::from_slice(&decoded).ok()?;
    claims.get("exp")?.as_u64()
}

fn refresh_command_server(server_config: &ServerConfig) -> Result<(), Error> {
    let server_name = server_config.server_name.as_deref()
        .unwrap_or(&server_config.id);

    if let Some(cached) = cache::read(&server_config.id) {
        let remaining_seconds = cached.expires_at_ms.saturating_sub(now_epoch_ms()) / 1000;
        if remaining_seconds >= EXPIRY_BUFFER_SECONDS {
            log!(
                "[{}] Token still valid ({} minutes remaining).",
                server_config.id,
                remaining_seconds / 60,
            );
            return Ok(());
        }
    }

    let command = server_config.token_command.as_ref().unwrap();
    log!("[{}] Running token command...", server_config.id);

    let output = std::process::Command::new("/bin/sh")
        .args(["-c", command])
        .output()
        .map_err(|error| Error::new(&format!(
            "[{}] failed to run token_command: {error}",
            server_config.id,
        )))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::new(&format!(
            "[{}] token_command exited with {}: {stderr}",
            server_config.id,
            output.status.code().unwrap_or(-1),
        )));
    }

    let token = String::from_utf8(output.stdout)
        .map_err(|error| Error::from_error_default(Box::new(error)))?
        .trim()
        .to_string();

    if token.is_empty() {
        return Err(Error::new(&format!(
            "[{}] token_command produced empty output",
            server_config.id,
        )));
    }

    let expires_at_ms = if let Some(exp) = decode_jwt_exp(&token) {
        exp * 1000
    } else if let Some(ttl) = server_config.token_ttl {
        log!(
            "[{}] Could not decode JWT exp; using token_ttl={ttl}s.",
            server_config.id,
        );
        now_epoch_ms() + (ttl * 1000)
    } else {
        return Err(Error::new(&format!(
            "[{}] Could not decode JWT exp and no token_ttl set; \
            set token_ttl in config or ensure token_command outputs a JWT with an exp claim.",
            server_config.id,
        )));
    };

    crate::claude_json::update_server_token(server_name, &token)?;

    log_token_refreshed(&server_config.id, expires_at_ms);
    cache::write(&server_config.id, server_name, expires_at_ms);

    Ok(())
}

fn refresh_single_server(server_config: &ServerConfig) -> Result<(), Error> {
    if server_config.mode() == ServerMode::Command {
        return refresh_command_server(server_config);
    }

    if let Some(cached) = cache::read(&server_config.id) {
        let remaining_seconds = cached.expires_at_ms.saturating_sub(now_epoch_ms()) / 1000;
        if remaining_seconds >= EXPIRY_BUFFER_SECONDS {
            log!("[{}] Token still valid ({} minutes remaining).", server_config.id, remaining_seconds / 60);
            return Ok(());
        }
    }

    let mut data = crate::keychain::read().unwrap_or_else(|_| serde_json::json!({"mcpOAuth": {}}));

    if !token_needs_refresh(server_config, &data) {
        return Ok(());
    }

    if std::env::var(crate::ENV_HOOK).is_ok()
        && std::env::var(crate::ENV_LAUNCHD).is_err()
    {
        return launchd::respawn();
    }

    let resolved = server_config.resolve()?;
    let token_response = oauth::authenticate(&resolved)?;
    update_keychain_data(&resolved, &mut data, &token_response)?;
    crate::keychain::write(&data)?;

    Ok(())
}

pub fn refresh_token(server_id: Option<&str>) -> Result<(), Error> {
    let config = crate::config::load()?;
    let servers = config.resolve_servers(server_id)?;

    for server_config in &servers {
        if let Err(error) = refresh_single_server(server_config) {
            log!("[{}] {error}", server_config.id);
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "token_test.rs"]
mod test;
