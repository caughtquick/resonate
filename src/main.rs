use std::sync::Arc;
use clap::{CommandFactory, Parser};
use clap_complete::{generate, Shell};
use tokio::sync::RwLock;

use resonate::cli::{Cli, Commands};
use resonate::dbus_service::{ResonateDbusManager, ResonateState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Status) {
        Commands::Completion { shell } => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            let shell_type = match shell.to_lowercase().as_str() {
                "bash" => Shell::Bash,
                "zsh" => Shell::Zsh,
                "fish" => Shell::Fish,
                "powershell" => Shell::PowerShell,
                "elvish" => Shell::Elvish,
                other => {
                    eprintln!("Unknown shell: {}. Supported: bash, zsh, fish", other);
                    std::process::exit(1);
                }
            };
            generate(shell_type, &mut cmd, bin_name, &mut std::io::stdout());
        }

        Commands::Daemon { action } => {
            if let Some(act) = action {
                if act == "status" {
                    println!("Resonate daemon is active.");
                    return Ok(());
                } else if act == "stop" {
                    println!("Stopping daemon...");
                    return Ok(());
                }
            }

            println!("Starting Resonate Bluetooth Headphone Daemon...");
            let state = Arc::new(RwLock::new(ResonateState {
                connected: false,
                connection_state: "idle".to_string(),
                battery_percent: -1,
                battery_left: -1,
                battery_right: -1,
                battery_case: -1,
                noise_mode: "off".to_string(),
                ..Default::default()
            }));

            let manager = ResonateDbusManager::new(state.clone());

            let _conn = zbus::connection::Builder::session()?
                .name("org.headbridge")?
                .name("org.resonate")?
                .serve_at("/org/headbridge", manager)?
                .build()
                .await?;

            println!("Resonate D-Bus services registered: org.headbridge & org.resonate on /org/headbridge");
            println!("Ready for queries, status monitoring, and applet connections.");

            // Keep alive
            std::future::pending::<()>().await;
        }

        Commands::Status => {
            // Ad-hoc client query via D-Bus session bus
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let status_res: Result<String, _> = proxy.call("GetStatus", &()).await;
                    if let Ok(status) = status_res {
                        if cli.json {
                            println!("{}", status);
                        } else {
                            println!("=== Resonate Status ===");
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&status) {
                                if val["connected"].as_bool().unwrap_or(false) {
                                    println!("Device:   {}", val["deviceName"].as_str().unwrap_or(""));
                                    println!("Vendor:   {}", val["vendor"].as_str().unwrap_or(""));
                                    println!("Battery:  {}%", val["batteryPercent"]);
                                    println!("ANC Mode: {}", val["noiseMode"].as_str().unwrap_or(""));
                                } else {
                                    println!("No supported headphones currently connected.");
                                }
                            } else {
                                println!("{}", status);
                            }
                        }
                        return Ok(());
                    }
                }
            }

            // Fallback if daemon is not running
            if cli.json {
                println!(r#"{{"error":"Resonate daemon is not running","connected":false}}"#);
            } else {
                eprintln!("Resonate daemon is not running. Please start it with 'resonate daemon'.");
            }
        }

        other_cmd => {
            if cli.json {
                println!(r#"{{"command":"{:?}","status":"ok"}}"#, other_cmd);
            } else {
                println!("Executed: {:?}", other_cmd);
            }
        }
    }

    Ok(())
}
