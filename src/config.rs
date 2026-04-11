use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::error::Error;
use crate::log;

#[derive(Deserialize)]
pub struct Config {
    #[serde(rename = "servers")]
    pub servers: Vec<ServerConfig>,
}

#[derive(Deserialize, Clone)]
pub struct ServerConfig {
    pub id: String,
    pub url: String,
    pub client_id: String,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub authorization_endpoint: Option<String>,
    #[serde(default)]
    pub token_endpoint: Option<String>,
    #[serde(default)]
    pub authorization_server_url: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
}

pub struct ResolvedServerConfig {
    pub id: String,
    pub url: String,
    pub client_id: String,
    pub resource: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub authorization_server_url: String,
    pub server_name: Option<String>,
}

impl ServerConfig {
    pub fn resource_url(&self) -> &str {
        self.resource.as_deref().unwrap_or(&self.url)
    }

    fn needs_discovery(&self) -> bool {
        self.authorization_endpoint.is_none() || self.token_endpoint.is_none()
    }

    pub fn resolve(&self) -> Result<ResolvedServerConfig, Error> {
        let discovery = if self.needs_discovery() {
            Some(fetch_discovery(&self.url)?)
        } else {
            None
        };

        let authorization_endpoint = self
            .authorization_endpoint
            .clone()
            .or_else(|| discovery.as_ref()?.get("authorization_endpoint")?.as_str().map(String::from))
            .ok_or_else(|| Error::new(&format!("[{}] could not determine authorization_endpoint", self.id)))?;

        let token_endpoint = self
            .token_endpoint
            .clone()
            .or_else(|| discovery.as_ref()?.get("token_endpoint")?.as_str().map(String::from))
            .ok_or_else(|| Error::new(&format!("[{}] could not determine token_endpoint", self.id)))?;

        let authorization_server_url = self
            .authorization_server_url
            .clone()
            .or_else(|| discovery.as_ref()?.get("issuer")?.as_str().map(String::from))
            .unwrap_or_else(|| base_url(&self.url));

        Ok(ResolvedServerConfig {
            id: self.id.clone(),
            url: self.url.clone(),
            client_id: self.client_id.clone(),
            resource: self.resource.clone().unwrap_or_else(|| self.url.clone()),
            authorization_endpoint,
            token_endpoint,
            authorization_server_url,
            server_name: self.server_name.clone(),
        })
    }
}

fn base_url(url: &str) -> String {
    Url::parse(url)
        .map(|parsed| format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or("")))
        .unwrap_or_else(|_| url.to_string())
}

fn try_fetch_discovery(discovery_url: &str) -> Option<Value> {
    let response = ureq::get(discovery_url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .ok()?;

    if response.status() != 200 {
        return None;
    }

    response.into_json().ok()
}

fn fetch_discovery(url: &str) -> Result<Value, Error> {
    let base = base_url(url);

    let base_discovery_url = format!("{base}/.well-known/oauth-authorization-server");
    log!("Fetching OAuth discovery from {base_discovery_url}");
    if let Some(document) = try_fetch_discovery(&base_discovery_url) {
        return Ok(document);
    }

    let path_discovery_url = format!("{url}/.well-known/oauth-authorization-server");
    if path_discovery_url != base_discovery_url {
        log!("Trying fallback discovery at {path_discovery_url}");
        if let Some(document) = try_fetch_discovery(&path_discovery_url) {
            return Ok(document);
        }
    }

    Err(Error::new(&format!(
        "OAuth discovery failed; tried {base_discovery_url} — set authorization_endpoint and token_endpoint in config to skip discovery",
    )))
}

fn config_path() -> Result<PathBuf, Error> {
    let home = std::env::var("HOME")
        .map_err(|error| Error::from_error_default(Box::new(error)))?;
    Ok(PathBuf::from(home).join(".config").join("mcp-reauth").join("config.toml"))
}

pub fn load() -> Result<Config, Error> {
    let path = config_path()?;
    let content = std::fs::read_to_string(&path)
        .map_err(|error| Error::new(&format!("could not read {}: {error}", path.display())))?;
    let config: Config = toml::from_str(&content)
        .map_err(|error| Error::new(&format!("could not parse {}: {error}", path.display())))?;
    if config.servers.is_empty() {
        return Err(Error::new("no servers configured in config.toml"));
    }
    Ok(config)
}

impl Config {
    pub fn find_server(&self, id: &str) -> Result<&ServerConfig, Error> {
        self.servers
            .iter()
            .find(|server| server.id == id)
            .ok_or_else(|| Error::new(&format!("no server with id '{id}' found in config")))
    }

    pub fn resolve_servers(&self, server_id: Option<&str>) -> Result<Vec<&ServerConfig>, Error> {
        match server_id {
            Some(id) => Ok(vec![self.find_server(id)?]),
            None => Ok(self.servers.iter().collect()),
        }
    }
}

#[cfg(test)]
#[path = "config_test.rs"]
mod test;

