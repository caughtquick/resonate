use std::sync::Arc;
use clap::{CommandFactory, Parser};
use clap_complete::{generate, Shell};
use tokio::sync::RwLock;

use resonate::cli::{Cli, Commands};
use resonate::dbus_service::{ResonateDbusManager, ResonateState};
use resonate::inventory::BluetoothInventory;
use resonate::manager::DeviceManager;

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

            let device_mgr = Arc::new(DeviceManager::new(state.clone(), tokio::runtime::Handle::current()));
            let dbus_mgr = ResonateDbusManager::new(device_mgr.clone());

            let _conn = zbus::connection::Builder::session()?
                .name("org.headbridge")?
                .name("org.resonate")?
                .serve_at("/org/headbridge", dbus_mgr)?
                .build()
                .await?;

            println!("Resonate D-Bus services registered: org.headbridge & org.resonate on /org/headbridge");
            println!("Monitoring BlueZ device connections...");

            // Spawn auto-connect and monitoring loop
            tokio::spawn(device_mgr.clone().run_monitoring_loop());

            // Keep daemon alive
            std::future::pending::<()>().await;
        }

        Commands::Devices => {
            // First check if daemon is running and has device list
            let mut output_json: Option<String> = None;
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let res: Result<String, _> = proxy.call("ListDevices", &()).await;
                    if let Ok(json_str) = res {
                        if json_str != "[]" {
                            output_json = Some(json_str);
                        }
                    }
                }
            }

            // Direct BlueZ discovery fallback if daemon not active or returned empty
            let devices = if let Some(j) = output_json {
                serde_json::from_str::<Vec<resonate::inventory::PairedDevice>>(&j).unwrap_or_default()
            } else {
                BluetoothInventory::fetch_paired_devices().await.unwrap_or_default()
            };

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&devices).unwrap_or_else(|_| "[]".to_string()));
            } else {
                println!("=== Paired Bluetooth Headphones ===");
                let supported: Vec<_> = devices.iter().filter(|d| d.is_supported()).collect();
                if supported.is_empty() {
                    println!("No supported AirPods, Bowers & Wilkins, or Sony headphones found.");
                    println!("Ensure Bluetooth is turned on and your device is paired.");
                } else {
                    for d in supported {
                        let conn_tag = if d.connected {
                            "[Connected]"
                        } else {
                            "[Not connected]"
                        };
                        println!(
                            " - {} ({}) - {} {}",
                            d.name, d.address, d.vendor, conn_tag
                        );
                    }
                }
            }
        }

        Commands::Connect { target } => {
            let conn = match zbus::Connection::session().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to connect to D-Bus session: {}", e);
                    std::process::exit(1);
                }
            };

            let proxy = match zbus::Proxy::new(
                &conn,
                "org.headbridge",
                "/org/headbridge",
                "org.headbridge.Manager",
            ).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Resonate daemon is not running. Please start it with 'resonate daemon'. Error: {}", e);
                    std::process::exit(1);
                }
            };

            let target_str = match target {
                Some(t) => t,
                None => {
                    // Default to first supported device
                    let devs = BluetoothInventory::fetch_paired_devices().await.unwrap_or_default();
                    if let Some(d) = devs.into_iter().find(|d| d.is_supported()) {
                        d.address
                    } else {
                        eprintln!("No supported paired devices found to connect to.");
                        std::process::exit(1);
                    }
                }
            };

            println!("Connecting to {}...", target_str);
            let res: Result<(bool, String), _> = proxy.call("Connect", &(target_str,)).await;
            match res {
                Ok((success, msg)) => {
                    if cli.json {
                        println!(r#"{{"success":{},"message":"{}"}}"#, success, msg);
                    } else {
                        println!("{}", msg);
                    }
                }
                Err(e) => {
                    eprintln!("Connect call failed: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Disconnect => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let res: Result<bool, _> = proxy.call("Disconnect", &()).await;
                    if let Ok(true) = res {
                        if cli.json {
                            println!("{}", r#"{"disconnected":true}"#);
                        } else {
                            println!("Disconnected active session.");
                        }
                        return Ok(());
                    }
                }
            }
            eprintln!("Resonate daemon is not running or failed to disconnect.");
        }

        Commands::Status => {
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
                                    println!("Device:      {}", val["deviceName"].as_str().unwrap_or(""));
                                    println!("Vendor:      {}", val["vendor"].as_str().unwrap_or(""));
                                    if let Some(pct) = val["batteryPercent"].as_i64() {
                                        println!("Battery:     {}%", pct);
                                    }
                                    if let Some(left) = val["batteryLeft"].as_i64() {
                                        println!("  Left Bud:  {}%", left);
                                    }
                                    if let Some(right) = val["batteryRight"].as_i64() {
                                        println!("  Right Bud: {}%", right);
                                    }
                                    if let Some(case) = val["batteryCase"].as_i64() {
                                        println!("  Case:      {}%", case);
                                    }
                                    println!("ANC Mode:    {}", val["noiseMode"].as_str().unwrap_or(""));
                                    if let Some(ear) = val["earDetection"].as_bool() {
                                        println!("In-Ear:      {}", if ear { "Detected (wearing)" } else { "Not worn" });
                                    }
                                    if let Some(conv) = val["conversationAwareness"].as_bool() {
                                        println!("Conv Aware:  {}", if conv { "Enabled" } else { "Disabled" });
                                    }
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

            if cli.json {
                println!(r#"{{"error":"Resonate daemon is not running","connected":false}}"#);
            } else {
                eprintln!("Resonate daemon is not running. Please start it with 'resonate daemon'.");
            }
        }

        Commands::Battery => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let status_res: Result<String, _> = proxy.call("GetStatus", &()).await;
                    if let Ok(status) = status_res {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&status) {
                            if cli.json {
                                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                    "percent": val["batteryPercent"],
                                    "left": val["batteryLeft"],
                                    "right": val["batteryRight"],
                                    "case": val["batteryCase"],
                                    "charging": val["isCharging"]
                                })).unwrap());
                            } else {
                                println!("=== Battery Telemetry ===");
                                println!("Overall:  {}%", val["batteryPercent"]);
                                if let Some(l) = val["batteryLeft"].as_i64() {
                                    println!("Left:     {}%", l);
                                }
                                if let Some(r) = val["batteryRight"].as_i64() {
                                    println!("Right:    {}%", r);
                                }
                                if let Some(c) = val["batteryCase"].as_i64() {
                                    println!("Case:     {}%", c);
                                }
                                println!("Charging: {}", val["isCharging"]);
                            }
                            return Ok(());
                        }
                    }
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::Anc { mode } => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    if let Some(m) = mode {
                        let res: Result<(bool, String), _> = proxy.call("SetAnc", &(m,)).await;
                        match res {
                            Ok((_, msg)) => println!("{}", msg),
                            Err(e) => eprintln!("Failed to set ANC mode: {}", e),
                        }
                    } else {
                        let status_res: Result<String, _> = proxy.call("GetStatus", &()).await;
                        if let Ok(status) = status_res {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&status) {
                                println!("Current ANC Mode: {}", val["noiseMode"].as_str().unwrap_or("off"));
                            }
                        }
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::ConvAware { action } => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let enable = match action.as_deref() {
                        Some("on") | Some("enable") | Some("true") | Some("1") => true,
                        Some("off") | Some("disable") | Some("false") | Some("0") => false,
                        _ => true,
                    };
                    let res: Result<bool, _> = proxy.call("SetConversationAwareness", &(enable,)).await;
                    if let Ok(true) = res {
                        println!("Conversation Awareness set to: {}", enable);
                    } else {
                        eprintln!("Failed to set Conversation Awareness.");
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::WearSensor { action } => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let enable = match action.as_deref() {
                        Some("on") | Some("enable") | Some("true") | Some("1") => true,
                        Some("off") | Some("disable") | Some("false") | Some("0") => false,
                        _ => true,
                    };
                    let res: Result<bool, _> = proxy.call("SetWearSensor", &(enable,)).await;
                    if let Ok(true) = res {
                        println!("Wear / in-ear detection set to: {}", enable);
                    } else {
                        eprintln!("Failed to set wear sensor.");
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
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
