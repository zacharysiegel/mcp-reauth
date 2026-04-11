use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cache;
use crate::config::{ResolvedServerConfig, ServerConfig};
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
    let server_key = match find_server_key(data, server_config.resource_url()) {
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
        .or_else(|| server_name(data, server_config.resource_url()))
}

fn resolve_server_name_from_resolved(resolved: &ResolvedServerConfig, data: &Value) -> Option<String> {
    resolved
        .server_name
        .clone()
        .or_else(|| server_name(data, &resolved.resource))
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

    log!("[{}] Token refreshed successfully. Expires in {} hours.", resolved.id, expires_in / 3600);
    cache::write(&resolved.id, &name, expires_at_ms);
    Ok(())
}

pub fn invalidate_token(server_id: Option<&str>) -> Result<(), Error> {
    let config = crate::config::load()?;
    let servers = config.resolve_servers(server_id)?;
    let mut data = crate::keychain::read()?;
    let mut invalidated = 0;

    for server_config in &servers {
        let key = match find_server_key(&data, server_config.resource_url()) {
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

    Ok(())
}

fn refresh_single_server(server_config: &ServerConfig) -> Result<(), Error> {
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
