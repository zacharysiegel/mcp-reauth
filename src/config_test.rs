use super::*;

fn server_config(id: &str, url: &str, resource: Option<&str>) -> ServerConfig {
    ServerConfig {
        id: id.to_string(),
        url: url.to_string(),
        client_id: "test-client".to_string(),
        resource: resource.map(String::from),
        authorization_endpoint: None,
        token_endpoint: None,
        authorization_server_url: None,
        server_name: None,
    }
}

#[test]
fn resource_url_returns_resource_when_set() {
    let config = server_config("s", "https://example.com/mcp", Some("https://example.com/api"));
    assert_eq!(config.resource_url(), "https://example.com/api");
}

#[test]
fn resource_url_falls_back_to_url() {
    let config = server_config("s", "https://example.com/mcp", None);
    assert_eq!(config.resource_url(), "https://example.com/mcp");
}

#[test]
fn base_url_extracts_scheme_and_host() {
    assert_eq!(base_url("https://example.com/mcp/oauth"), "https://example.com");
    assert_eq!(base_url("http://localhost:8080/path"), "http://localhost");
}

#[test]
fn base_url_returns_input_on_invalid_url() {
    assert_eq!(base_url("not-a-url"), "not-a-url");
}

#[test]
fn needs_discovery_when_endpoints_missing() {
    let config = server_config("s", "https://example.com", None);
    assert!(config.needs_discovery());
}

#[test]
fn needs_discovery_when_one_endpoint_missing() {
    let mut config = server_config("s", "https://example.com", None);
    config.authorization_endpoint = Some("https://example.com/authorize".to_string());
    assert!(config.needs_discovery());
}

#[test]
fn no_discovery_when_both_endpoints_present() {
    let mut config = server_config("s", "https://example.com", None);
    config.authorization_endpoint = Some("https://example.com/authorize".to_string());
    config.token_endpoint = Some("https://example.com/token".to_string());
    assert!(!config.needs_discovery());
}

#[test]
fn resolve_skips_discovery_when_endpoints_present() {
    let mut config = server_config("s", "https://example.com/mcp", None);
    config.authorization_endpoint = Some("https://example.com/authorize".to_string());
    config.token_endpoint = Some("https://example.com/token".to_string());

    let resolved = config.resolve().unwrap();
    assert_eq!(resolved.authorization_endpoint, "https://example.com/authorize");
    assert_eq!(resolved.token_endpoint, "https://example.com/token");
    assert_eq!(resolved.resource, "https://example.com/mcp");
    assert_eq!(resolved.authorization_server_url, "https://example.com");
}

#[test]
fn find_server_returns_matching_server() {
    let config = Config {
        servers: vec![
            server_config("a", "https://a.com", None),
            server_config("b", "https://b.com", None),
        ],
    };
    assert_eq!(config.find_server("b").unwrap().url, "https://b.com");
}

#[test]
fn find_server_returns_error_for_unknown_id() {
    let config = Config {
        servers: vec![server_config("a", "https://a.com", None)],
    };
    assert!(config.find_server("z").is_err());
}

#[test]
fn resolve_servers_returns_all_when_no_filter() {
    let config = Config {
        servers: vec![
            server_config("a", "https://a.com", None),
            server_config("b", "https://b.com", None),
        ],
    };
    assert_eq!(config.resolve_servers(None).unwrap().len(), 2);
}

#[test]
fn resolve_servers_filters_by_id() {
    let config = Config {
        servers: vec![
            server_config("a", "https://a.com", None),
            server_config("b", "https://b.com", None),
        ],
    };
    let servers = config.resolve_servers(Some("a")).unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].id, "a");
}
