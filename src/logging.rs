use std::path::Path;
use std::sync::OnceLock;

const LOG_MAX_LINES: usize = 1000;
pub const STDERR_LOG: &str = "/tmp/mcp-reauth/hook/stderr.log";
pub const STDOUT_LOG: &str = "/tmp/mcp-reauth/hook/stdout.log";

static TRACE_ID: OnceLock<String> = OnceLock::new();

pub const ENV_TRACE_ID: &str = "MCP_REAUTH_TRACE_ID";

pub fn trace_id() -> &'static str {
    TRACE_ID.get_or_init(|| {
        std::env::var(ENV_TRACE_ID).unwrap_or_else(|_| uuid::Uuid::new_v4().to_string())
    })
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        eprintln!("[{}] [{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), $crate::logging::trace_id(), format!($($arg)*))
    };
}

fn truncate_file(path: &Path) {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return,
    };

    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= LOG_MAX_LINES {
        return;
    }

    let truncated = lines[lines.len() - LOG_MAX_LINES..].join("\n");
    let _ = std::fs::write(path, truncated + "\n");
}

pub fn truncate_logs() {
    truncate_file(Path::new(STDERR_LOG));
    truncate_file(Path::new(STDOUT_LOG));
}
