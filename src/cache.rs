use std::path::PathBuf;

const CACHE_DIR: &str = "/tmp/mcp-reauth";

pub struct MetadataCache {
    pub server_name: String,
    pub expires_at_ms: u64,
}

fn cache_path(server_id: &str) -> PathBuf {
    PathBuf::from(CACHE_DIR).join(server_id).join("cache")
}

pub fn read(server_id: &str) -> Option<MetadataCache> {
    let content = std::fs::read_to_string(cache_path(server_id)).ok()?;
    let mut lines = content.lines();
    let server_name = lines.next()?.trim().to_string();
    let expires_at_ms = lines.next()?.trim().parse().ok()?;
    if server_name.is_empty() {
        return None;
    }
    Some(MetadataCache { server_name, expires_at_ms })
}

pub fn write(server_id: &str, server_name: &str, expires_at_ms: u64) {
    let path = cache_path(server_id);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, format!("{server_name}\n{expires_at_ms}\n"));
}

pub fn remove(server_id: &str) {
    let _ = std::fs::remove_file(cache_path(server_id));
}
