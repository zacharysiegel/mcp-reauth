use super::*;

fn keychain_data(server_name: &str, server_url: &str, access_token: &str, expires_at: u64) -> Value {
    serde_json::json!({
        "mcpOAuth": {
            format!("{server_name}|abc-123"): {
                "serverName": server_name,
                "serverUrl": server_url,
                "accessToken": access_token,
                "expiresAt": expires_at,
            }
        }
    })
}

#[test]
fn server_name_finds_by_server_url() {
    let data = keychain_data("my-mcp", "https://example.com/mcp", "tok", 0);
    assert_eq!(
        server_name(&data, "https://example.com/mcp"),
        Some("my-mcp".to_string()),
    );
}

#[test]
fn server_name_returns_none_for_unknown_url() {
    let data = keychain_data("my-mcp", "https://example.com/mcp", "tok", 0);
    assert_eq!(server_name(&data, "https://other.com"), None);
}

#[test]
fn server_name_returns_none_for_empty_keychain() {
    let data = serde_json::json!({"mcpOAuth": {}});
    assert_eq!(server_name(&data, "https://example.com"), None);
}

#[test]
fn server_name_returns_none_for_missing_mcp_oauth() {
    let data = serde_json::json!({});
    assert_eq!(server_name(&data, "https://example.com"), None);
}

#[test]
fn find_server_key_returns_full_key() {
    let data = keychain_data("my-mcp", "https://example.com/mcp", "tok", 0);
    assert_eq!(
        find_server_key(&data, "https://example.com/mcp"),
        Some("my-mcp|abc-123".to_string()),
    );
}

#[test]
fn find_server_key_returns_none_for_unknown_url() {
    let data = keychain_data("my-mcp", "https://example.com/mcp", "tok", 0);
    assert_eq!(find_server_key(&data, "https://other.com"), None);
}

#[test]
fn decode_jwt_exp_extracts_exp_from_valid_jwt() {
    // Build a JWT with exp=1700000000
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(r#"{"alg":"RS256","typ":"JWT"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(r#"{"sub":"user","exp":1700000000}"#);
    let token = format!("{header}.{payload}.signature");

    assert_eq!(decode_jwt_exp(&token), Some(1700000000));
}

#[test]
fn decode_jwt_exp_returns_none_for_non_jwt() {
    assert_eq!(decode_jwt_exp("not-a-jwt"), None);
}

#[test]
fn decode_jwt_exp_returns_none_for_jwt_without_exp() {
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(r#"{"alg":"RS256"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(r#"{"sub":"user"}"#);
    let token = format!("{header}.{payload}.signature");

    assert_eq!(decode_jwt_exp(&token), None);
}

#[test]
fn decode_jwt_exp_returns_none_for_empty_string() {
    assert_eq!(decode_jwt_exp(""), None);
}
