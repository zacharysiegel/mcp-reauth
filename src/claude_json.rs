use std::path::PathBuf;

use serde_json::Value;

use crate::error::Error;

const FILENAME: &str = ".claude.json";

fn config_path() -> Result<PathBuf, Error> {
    let home = std::env::var("HOME")
        .map_err(|error| Error::from_error_default(Box::new(error)))?;
    Ok(PathBuf::from(home).join(FILENAME))
}

fn read() -> Result<Value, Error> {
    let path = config_path()?;
    if !path.exists() {
        return Err(Error::new(&format!(
            "~/{FILENAME} not found; ensure Claude Code is installed and configured",
        )));
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

fn write(data: &Value) -> Result<(), Error> {
    let path = config_path()?;
    std::fs::write(&path, serde_json::to_string_pretty(data)? + "\n")?;
    Ok(())
}

fn set_bearer_header(data: &mut Value, server_name: &str, token: &str) -> Result<(), Error> {
    let server_entry = data
        .get_mut("mcpServers")
        .and_then(|servers| servers.get_mut(server_name))
        .and_then(|entry| entry.as_object_mut())
        .ok_or_else(|| Error::new(&format!(
            "[{server_name}] not found under mcpServers in ~/{FILENAME}; add it to Claude Code first",
        )))?;

    let headers = server_entry
        .entry("headers")
        .or_insert_with(|| serde_json::json!({}));

    headers["Authorization"] = Value::String(format!("Bearer {token}"));

    Ok(())
}

pub fn update_server_token(server_name: &str, token: &str) -> Result<(), Error> {
    let mut data = read()?;
    set_bearer_header(&mut data, server_name, token)?;
    write(&data)
}
