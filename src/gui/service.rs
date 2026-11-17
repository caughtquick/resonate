//! Systemd user service management for the `resonate.service` background daemon.

use std::process::Command;

/// Queries systemd to check if `resonate.service` is actively running.
pub fn is_service_active() -> (bool, String) {
    match Command::new("systemctl")
        .args(["--user", "is-active", "resonate.service"])
        .output()
    {
        Ok(out) => {
            let status = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let is_active = status == "active";
            let display_str = match status.as_str() {
                "active" => "Active (Running)".to_string(),
                "inactive" => "Inactive (Stopped)".to_string(),
                "failed" => "Failed".to_string(),
                "activating" => "Starting...".to_string(),
                "deactivating" => "Stopping...".to_string(),
                other => {
                    if other.is_empty() {
                        "Not Running".to_string()
                    } else {
                        format!("Status: {}", other)
                    }
                }
            };
            (is_active, display_str)
        }
        Err(e) => (false, format!("Error: {}", e)),
    }
}

/// Starts the `resonate.service` user unit.
pub fn start_service() -> Result<(), String> {
    let status = Command::new("systemctl")
        .args(["--user", "start", "resonate.service"])
        .status()
        .map_err(|e| format!("Failed to spawn systemctl: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("systemctl start exited with code {:?}", status.code()))
    }
}

/// Stops the `resonate.service` user unit.
pub fn stop_service() -> Result<(), String> {
    let status = Command::new("systemctl")
        .args(["--user", "stop", "resonate.service"])
        .status()
        .map_err(|e| format!("Failed to spawn systemctl: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("systemctl stop exited with code {:?}", status.code()))
    }
}

/// Restarts the `resonate.service` user unit.
pub fn restart_service() -> Result<(), String> {
    let status = Command::new("systemctl")
        .args(["--user", "restart", "resonate.service"])
        .status()
        .map_err(|e| format!("Failed to spawn systemctl: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("systemctl restart exited with code {:?}", status.code()))
    }
}
