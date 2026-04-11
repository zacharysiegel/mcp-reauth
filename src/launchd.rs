use std::io::Read;

use crate::error::Error;
use crate::log;

const LABEL: &str = "ro.zach.mcp-reauth";
const PLIST_PATH: &str = "/tmp/mcp-reauth/launchd.plist";
const FIFO_PATH: &str = "/tmp/mcp-reauth/launchd.pipe";
const LAUNCHD_STDOUT: &str = "/tmp/mcp-reauth/launchd.stdout";
const LAUNCHD_STDERR: &str = "/tmp/mcp-reauth/launchd.stderr";

fn current_uid() -> Result<String, Error> {
    let output = std::process::Command::new("id")
        .arg("-u")
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn create_fifo() -> Result<(), Error> {
    if let Some(parent) = std::path::Path::new(FIFO_PATH).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(FIFO_PATH);
    let status = std::process::Command::new("mkfifo")
        .args(["-m", "600", FIFO_PATH])
        .status()?;
    if !status.success() {
        return Err(Error::new("mkfifo failed"));
    }
    Ok(())
}

fn write_plist(binary_path: &str, server_arg: &str) -> Result<(), Error> {
    let trace_id = crate::logging::trace_id();
    let hook_type = std::env::var(crate::ENV_HOOK_TYPE).unwrap_or_default();
    let command = format!(
        "{}=1 {}={trace_id} {}={hook_type} {binary_path}{server_arg}; echo $? > {FIFO_PATH}",
        crate::ENV_LAUNCHD,
        crate::logging::ENV_TRACE_ID,
        crate::ENV_HOOK_TYPE,
    );
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>/bin/sh</string>
        <string>-c</string>
        <string>{command}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{LAUNCHD_STDOUT}</string>
    <key>StandardErrorPath</key>
    <string>{LAUNCHD_STDERR}</string>
</dict>
</plist>
"#,
    );
    std::fs::write(PLIST_PATH, plist)?;
    Ok(())
}

fn bootstrap(uid: &str) -> Result<(), Error> {
    let output = std::process::Command::new("launchctl")
        .args(["bootstrap", &format!("gui/{uid}"), PLIST_PATH])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::new(&format!(
            "launchctl bootstrap failed (exit {}): {stderr}",
            output.status.code().unwrap_or(-1),
        )));
    }
    Ok(())
}

fn bootout(uid: &str) -> Result<(), Error> {
    let output = std::process::Command::new("launchctl")
        .args(["bootout", &format!("gui/{uid}/{LABEL}")])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::new(&format!(
            "launchctl bootout failed (exit {}): {stderr}",
            output.status.code().unwrap_or(-1),
        )));
    }
    Ok(())
}

fn read_exit_code_from_fifo() -> Result<i32, Error> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .open(FIFO_PATH)?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            Ok::<String, std::io::Error>(contents)
        })();
        let _ = sender.send(result);
    });

    let contents = receiver
        .recv_timeout(std::time::Duration::from_secs(120))
        .map_err(|_| Error::new("timed out waiting for launchd job to complete"))?
        .map_err(|error| Error::from_error_default(Box::new(error)))?;

    contents
        .trim()
        .parse::<i32>()
        .map_err(|error| Error::from_error_default(Box::new(error)))
}

fn cleanup() {
    let _ = std::fs::remove_file(PLIST_PATH);
    let _ = std::fs::remove_file(FIFO_PATH);
}

fn log_launchd_output() {
    if let Ok(stdout) = std::fs::read_to_string(LAUNCHD_STDOUT) {
        if !stdout.trim().is_empty() {
            log!("launchd stdout: {stdout}");
        }
    }
    if let Ok(stderr) = std::fs::read_to_string(LAUNCHD_STDERR) {
        if !stderr.trim().is_empty() {
            log!("launchd stderr: {stderr}");
        }
    }
}

pub fn respawn() -> Result<(), Error> {
    let binary_path = std::env::current_exe()
        .map_err(|error| Error::from_error_default(Box::new(error)))?;
    let binary_path = binary_path.to_string_lossy();
    let uid = current_uid()?;

    let server_arg = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|window| window[0] == "--server")
        .map(|window| format!(" --server {}", window[1]))
        .unwrap_or_default();

    log!("Respawning outside sandbox via launchd...");

    create_fifo()?;
    write_plist(&binary_path, &server_arg)?;

    let result = (|| {
        let _ = bootout(&uid);
        bootstrap(&uid)?;
        let exit_code = read_exit_code_from_fifo()?;
        log_launchd_output();
        let bootout_result = bootout(&uid);
        if exit_code != 0 {
            return Err(Error::new(&format!(
                "respawned process exited with code {exit_code}",
            )));
        }
        bootout_result
    })();

    cleanup();
    result
}
