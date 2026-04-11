use serde_json::Value;
use std::process::Command;

use crate::error::Error;

const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

fn keychain_account() -> String {
    std::env::var("USER").unwrap_or_else(|_| whoami::username())
}

pub fn read() -> Result<Value, Error> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s", KEYCHAIN_SERVICE,
            "-a", &keychain_account(),
            "-w",
        ])
        .output()?;

    if !output.status.success() {
        return Err(Error::new("keychain entry not found"));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| Error::from_error_default(Box::new(error)))?;

    serde_json::from_str(stdout.trim()).map_err(Into::into)
}

pub fn write(data: &Value) -> Result<(), Error> {
    let json_str = serde_json::to_string(data)?;

    let account = keychain_account();
    let _ = Command::new("security")
        .args([
            "delete-generic-password",
            "-s", KEYCHAIN_SERVICE,
            "-a", &account,
        ])
        .output();

    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-s", KEYCHAIN_SERVICE,
            "-a", &account,
            "-w", &json_str,
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::new(&format!("security add failed: {stderr}")));
    }

    Ok(())
}
