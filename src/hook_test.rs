use super::*;

#[test]
fn command_is_ours_matches_nonce() {
    let command = format!("MCP_REAUTH_NONCE={HOOK_NONCE} MCP_REAUTH_HOOK=1 /usr/bin/mcp-reauth");
    assert!(command_is_ours(&command));
}

#[test]
fn command_is_ours_rejects_different_nonce() {
    let command = "MCP_REAUTH_NONCE=00000000-0000-0000-0000-000000000000 MCP_REAUTH_HOOK=1 /usr/bin/mcp-reauth";
    assert!(!command_is_ours(command));
}

#[test]
fn command_is_ours_rejects_unrelated_command() {
    assert!(!command_is_ours("echo hello"));
}

#[test]
fn remove_our_hooks_removes_matching_entries() {
    let nonce_command = format!("MCP_REAUTH_NONCE={HOOK_NONCE} /usr/bin/mcp-reauth");
    let mut entries = vec![
        serde_json::json!({"hooks": [{"command": "unrelated"}]}),
        serde_json::json!({"hooks": [{"command": nonce_command}]}),
        serde_json::json!({"hooks": [{"command": "also unrelated"}]}),
    ];
    let removed = remove_our_hooks(&mut entries);
    assert_eq!(removed, 1);
    assert_eq!(entries.len(), 2);
}

#[test]
fn remove_our_hooks_leaves_unrelated_entries() {
    let mut entries = vec![
        serde_json::json!({"hooks": [{"command": "unrelated"}]}),
    ];
    let removed = remove_our_hooks(&mut entries);
    assert_eq!(removed, 0);
    assert_eq!(entries.len(), 1);
}

#[test]
fn hook_already_installed_finds_exact_command() {
    let command = "MCP_REAUTH_HOOK=1 /usr/bin/mcp-reauth";
    let settings = serde_json::json!({
        "hooks": {
            "PreToolUse": [
                {"hooks": [{"command": command}]},
            ]
        }
    });
    assert!(hook_already_installed(&settings, "PreToolUse", command));
}

#[test]
fn hook_already_installed_returns_false_for_missing_command() {
    let settings = serde_json::json!({
        "hooks": {
            "PreToolUse": [
                {"hooks": [{"command": "something else"}]},
            ]
        }
    });
    assert!(!hook_already_installed(&settings, "PreToolUse", "not-installed"));
}

#[test]
fn hook_already_installed_returns_false_for_empty_settings() {
    let settings = serde_json::json!({});
    assert!(!hook_already_installed(&settings, "PreToolUse", "anything"));
}

#[test]
fn add_hook_creates_structure_from_empty_settings() {
    let mut settings = serde_json::json!({});
    add_hook(&mut settings, "PreToolUse", "mcp__test__", "test-command").unwrap();

    let entries = settings.pointer("/hooks/PreToolUse").unwrap().as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["matcher"], "mcp__test__");
    assert_eq!(entries[0]["hooks"][0]["command"], "test-command");
}

#[test]
fn add_hook_appends_to_existing_entries() {
    let mut settings = serde_json::json!({
        "hooks": {
            "PreToolUse": [
                {"hooks": [{"command": "existing"}], "matcher": "existing"},
            ]
        }
    });
    add_hook(&mut settings, "PreToolUse", "mcp__new__", "new-command").unwrap();

    let entries = settings.pointer("/hooks/PreToolUse").unwrap().as_array().unwrap();
    assert_eq!(entries.len(), 2);
}
