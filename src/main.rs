use std::process::ExitCode;

use clap::{Arg, Command};
use mcp_reauth::log;

fn main() -> ExitCode {
    let matches = Command::new("mcp-reauth")
        .about("Refresh MCP OAuth tokens used by Claude Code")
        .arg(
            Arg::new("server")
                .long("server")
                .global(true)
                .help("Target a specific server by config ID (default: all servers)"),
        )
        .subcommand(
            Command::new("hook")
                .about("Manage the Claude Code hooks")
                .subcommand_required(true)
                .subcommand(
                    Command::new("install")
                        .about("Install hooks into ~/.claude/settings.json"),
                )
                .subcommand(
                    Command::new("uninstall")
                        .about("Remove hooks from ~/.claude/settings.json"),
                ),
        )
        .subcommand(
            Command::new("invalidate")
                .about("Invalidate cached token(s) to force re-authentication"),
        )
        .get_matches();

    let server_id = matches.get_one::<String>("server").map(String::as_str);

    let result = match matches.subcommand() {
        Some(("hook", hook_matches)) => match hook_matches.subcommand() {
            Some(("install", _)) => match mcp_reauth::install_hook(server_id) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    log!("{error}");
                    ExitCode::FAILURE
                }
            },
            Some(("uninstall", _)) => match mcp_reauth::uninstall_hook(server_id) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    log!("{error}");
                    ExitCode::FAILURE
                }
            },
            _ => unreachable!(),
        },
        Some(("invalidate", _)) => match mcp_reauth::invalidate_token(server_id) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                log!("{error}");
                ExitCode::FAILURE
            }
        },
        _ => match mcp_reauth::refresh_token(server_id) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                log!("{error}");
                ExitCode::FAILURE
            }
        },
    };

    mcp_reauth::truncate_logs();
    result
}
