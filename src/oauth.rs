use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;
use tiny_http::{Request, Response, Server};
use url::Url;

use crate::config::ResolvedServerConfig;
use crate::error::Error;
use crate::log;

fn generate_pkce() -> (String, String) {
    let mut rng = rand::thread_rng();
    let random_bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    let verifier = URL_SAFE_NO_PAD.encode(&random_bytes);

    let challenge_hash = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(challenge_hash);

    (verifier, challenge)
}

fn generate_state() -> String {
    let mut rng = rand::thread_rng();
    let random_bytes: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
    URL_SAFE_NO_PAD.encode(&random_bytes)
}

fn receive_authorization_code(server: &Server, expected_state: &str) -> Result<(String, Request), Error> {
    let request = server
        .recv_timeout(Duration::from_secs(120))
        .map_err(|error| Error::from_error_default(Box::new(error)))?
        .ok_or_else(|| Error::new("timed out waiting for OAuth callback"))?;

    let request_url = format!("http://localhost{}", request.url());
    let parsed = Url::parse(&request_url)?;
    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

    let code = params
        .get("code")
        .ok_or_else(|| {
            let error_description = params
                .get("error")
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            Error::new(&format!("OAuth error: {error_description}"))
        })?
        .clone();

    let callback_state = params.get("state").map(String::as_str).unwrap_or("");
    if callback_state != expected_state {
        let _ = request.respond(Response::from_string("State mismatch").with_status_code(400));
        return Err(Error::new("state mismatch in OAuth callback"));
    }

    Ok((code, request))
}

fn respond_success(request: Request, server_id: &str, expires_in_hours: u64, trigger: &str) {
    let html = format!(
        "<html><body>\
         <h2>Authentication successful.</h2>\
         <p>Server: <strong>{server_id}</strong></p>\
         <p>Initiated by <strong>mcp-reauth</strong> ({trigger}).</p>\
         <p>Token expires in {expires_in_hours} hours.</p>\
         <p>Close Claude Code and restart with <code>claude --continue</code> to load the new token.</p>\
         <p>You can close this tab.</p>\
         </body></html>",
    );
    let _ = request.respond(
        Response::from_string(html)
            .with_header(tiny_http::Header::from_bytes("Content-Type", "text/html").unwrap()),
    );
}

fn exchange_authorization_code(
    config: &ResolvedServerConfig,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<Value, Error> {
    let token_body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", code)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("client_id", &config.client_id)
        .append_pair("code_verifier", code_verifier)
        .finish();

    let response: Value = ureq::post(&config.token_endpoint)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&token_body)?
        .into_json()?;

    Ok(response)
}

fn trigger_description() -> &'static str {
    match std::env::var(crate::ENV_HOOK_TYPE).ok().as_deref() {
        Some("SessionStart") => "SessionStart hook",
        Some("PreToolUse") => "PreToolUse hook",
        Some(other) => Box::leak(format!("{other} hook").into_boxed_str()),
        None if std::env::var(crate::ENV_HOOK).is_ok() => "hook",
        None => "manual",
    }
}

pub fn authenticate(config: &ResolvedServerConfig) -> Result<Value, Error> {
    let (verifier, challenge) = generate_pkce();
    let state = generate_state();

    let http_server = Server::http("127.0.0.1:0")
        .map_err(|error| Error::new(&format!("HTTP server: {error}")))?;
    let port = http_server.server_addr().to_ip().unwrap().port();
    let redirect_uri = format!("http://localhost:{port}/callback");

    let mut params = url::form_urlencoded::Serializer::new(String::new());
    params.append_pair("response_type", "code");
    params.append_pair("client_id", &config.client_id);
    params.append_pair("code_challenge", &challenge);
    params.append_pair("code_challenge_method", "S256");
    params.append_pair("redirect_uri", &redirect_uri);
    params.append_pair("state", &state);
    if !config.resource.is_empty() {
        params.append_pair("resource", &config.resource);
    }
    let auth_url = format!(
        "{}?{}",
        config.authorization_endpoint,
        params.finish(),
    );

    log!("[{}] Opening browser for authentication...", config.id);
    open::that(&auth_url)
        .map_err(|error| Error::from_error_default(Box::new(error)))?;

    let (code, request) = receive_authorization_code(&http_server, &state)?;

    log!("[{}] Exchanging authorization code for token...", config.id);
    let token_response = exchange_authorization_code(config, &code, &redirect_uri, &verifier)?;

    let expires_in_hours = token_response
        .get("expires_in")
        .and_then(|value| value.as_u64())
        .unwrap_or(28800) / 3600;

    respond_success(request, &config.id, expires_in_hours, trigger_description());

    Ok(token_response)
}
