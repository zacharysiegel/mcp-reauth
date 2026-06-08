# mcp-reauth

Refreshes OAuth tokens for MCP servers used by [Claude Code](https://docs.anthropic.com/en/docs/claude-code). Many MCP servers issue short-lived tokens with no refresh token, requiring periodic re-authentication. This tool automates that via Claude Code hooks.

When a token is expired or expiring within an hour, it runs an OAuth 2.0 PKCE flow via the browser. If you have an active SSO session, the browser re-auth completes instantly.

## Configuration

Create `~/.config/mcp-reauth/config.toml`:

```toml
[[servers]]
id = "my-server"
url = "https://mcp.example.com/mcp"
client_id = "your-client-id"
```

OAuth endpoints are auto-discovered via [RFC 8414](https://www.rfc-editor.org/rfc/rfc8414) metadata (`/.well-known/oauth-authorization-server`). If your server doesn't support discovery, specify the endpoints explicitly:

```toml
[[servers]]
id = "my-server"
url = "https://mcp.example.com/mcp"
client_id = "your-client-id"
authorization_endpoint = "https://auth.example.com/authorize"
token_endpoint = "https://auth.example.com/token"
```

| Field                      | Required | Description                                                                                          |
|:---------------------------|:---------|:-----------------------------------------------------------------------------------------------------|
| `id`                       | yes      | Identifier for this server (used in CLI args and cache paths)                                        |
| `url`                      | yes      | MCP server URL (also used as `resource` and for keychain lookup if not overridden)                   |
| `client_id`                | yes      | OAuth client ID                                                                                      |
| `authorization_endpoint`   | no       | OAuth authorization URL (auto-discovered if omitted)                                                 |
| `token_endpoint`           | no       | OAuth token exchange URL (auto-discovered if omitted)                                                |
| `resource`                 | no       | Resource/audience identifier sent in the auth request; defaults to `url`                             |
| `authorization_server_url` | no       | Stored in keychain entry's `discoveryState`; defaults to discovery `issuer` or base URL              |
| `server_name`              | no       | Claude Code MCP server name; auto-discovered from keychain if omitted                                |

Multiple `[[servers]]` blocks can be defined. All commands operate on all servers by default; use `--server <id>` to target one.

## Setup

```sh
cargo install --path .
mcp-reauth hook install
```

`hook install` writes two kinds of hooks into `~/.claude/settings.json`:
- A `PreToolUse` hook per server (matcher: `mcp__<server_name>__`) that refreshes the token before each MCP tool call
- A single `SessionStart` hook that refreshes all tokens when a Claude Code session starts

To remove the hooks:

```sh
mcp-reauth hook uninstall
```

## Usage

```sh
# Refresh all servers (default action, also what hooks run)
mcp-reauth

# Refresh a specific server
mcp-reauth --server my-server

# Invalidate tokens to force re-authentication
mcp-reauth invalidate
mcp-reauth invalidate --server my-server
```

## How it works

- Tokens are stored in the macOS Keychain under `Claude Code-credentials` (shared by Claude Code for all MCP servers)
- A per-server cache at `/tmp/mcp-reauth/<server-id>/cache` stores the server name and token expiry for fast checks (~2ms file read vs ~16ms keychain subprocess)
- When running as a Claude Code hook, the binary detects the sandbox (which blocks port binding for the OAuth callback) and re-launches itself outside the sandbox via `launchctl bootstrap`
- Logs go to `/tmp/mcp-reauth/hook/stderr.log` and `/tmp/mcp-reauth/hook/stdout.log`

## Limitations

If the token is already expired when a session starts, the `SessionStart` hook will refresh it (the browser will open), but Claude Code will have already marked the MCP server as needing re-authentication before the hook completes. To pick up the refreshed token, close and restart the session with `claude --continue`.
