# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
# Build
cargo build
cargo build --release

# Run (refresh all servers)
cargo run
./target/release/mcp-reauth

# Run (refresh specific server)
cargo run -- --server <id>

# Install Claude Code hooks
./target/release/mcp-reauth hook install

# Uninstall Claude Code hooks
./target/release/mcp-reauth hook uninstall

# Invalidate token(s) (for testing)
./target/release/mcp-reauth invalidate
./target/release/mcp-reauth invalidate --server <id>
```

## Architecture

This is a single-binary Rust tool that refreshes OAuth tokens Claude Code uses to authenticate with MCP servers. It runs as a Claude Code `SessionStart` hook (on session open) and `PreToolUse` hook (before each MCP tool call). Server configuration is read from `~/.config/mcp-reauth/config.toml`.

**The problem it solves:** MCP server tokens have short lifetimes (e.g. 8 hours) with no refresh token. They're stored as JSON in the macOS Keychain under the service name `Claude Code-credentials`. This tool re-runs the OAuth PKCE flow when a token is close to expiry (within 1 hour).

**Token storage format:** The keychain entry holds a JSON object with a top-level `mcpOAuth` map. Each MCP server has an entry keyed by `"<servername>|<uuid>"`. The tool discovers each server's entry by scanning for the one whose `serverUrl` matches the config's `resource` (or `url` if `resource` is not set).

**Module responsibilities:**

- `config.rs` — loads `~/.config/mcp-reauth/config.toml`; defines `ServerConfig` (optional fields), `ResolvedServerConfig` (all fields populated), and `Config` structs. `ServerConfig::resolve()` fetches OAuth discovery metadata (RFC 8414) to fill in missing endpoints.
- `token.rs` — `refresh_token()` checks expiry and runs the OAuth PKCE flow if needed. On the fast path, reads the per-server cache (~2ms) instead of the keychain (~16ms `security` subprocess). Falls back to the keychain when the cache is missing or stale. Calls `resolve()` only when authentication is actually needed (slow path).
- `hook.rs` — `install_hook()` discovers server names (from cache, config, or keychain) and writes hook entries into `~/.claude/settings.json`; uses a static nonce UUID to identify its hooks for stale detection and uninstall
- `launchd.rs` — `respawn()` re-launches the binary outside the Claude Code sandbox via `launchctl bootstrap`; uses a named pipe (FIFO) to block until the respawned process completes; passes `--server` arg through when targeting a single server
- `cache.rs` — per-server reads/writes under `/tmp/mcp-reauth/<server-id>/cache` (two lines: server name, expiry timestamp)
- `oauth.rs` — full OAuth 2.0 PKCE flow: spins up an ephemeral `tiny_http` server on a random port for the redirect callback, opens the browser, waits for the authorization code, exchanges it for a token. All OAuth parameters taken from `ResolvedServerConfig`.
- `keychain.rs` — reads/writes the keychain entry via the macOS `security` CLI
- `logging.rs` — `log!` macro writing to stderr with timestamp and trace ID; `truncate_logs()` truncates log files to 1000 lines
- `error.rs` — unified `Error` type with optional chained sub-error and forced backtrace capture

**Logs:** `/tmp/mcp-reauth/hook/{stdout,stderr}.log`
