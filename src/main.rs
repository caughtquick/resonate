use std::io::Write;
use std::sync::Arc;
use clap::{CommandFactory, Parser};
use clap_complete::{generate, Shell};
use futures_lite::stream::StreamExt;
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
                } else if act == "install" {
                    let exe_path = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("/usr/local/bin/resonate"));
                    let service_content = format!(
r#"[Unit]
Description=Resonate Bluetooth Headphone Daemon
After=bluetooth.target sound.target

[Service]
Type=simple
ExecStart={} daemon
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
"#, exe_path.display());
                    let user_systemd_dir = resonate::settings::dirs_config_dir().join("systemd").join("user");
                    let _ = std::fs::create_dir_all(&user_systemd_dir);
                    let service_file = user_systemd_dir.join("resonate.service");
                    match std::fs::write(&service_file, service_content) {
                        Ok(_) => {
                            println!("Systemd user service installed to: {}", service_file.display());
                            println!("To enable and start Resonate automatically on login, run:");
                            println!("  systemctl --user daemon-reload");
                            println!("  systemctl --user enable --now resonate.service");
                        }
                        Err(e) => eprintln!("Failed to install systemd service: {}", e),
                    }
                    return Ok(());
                } else if act == "uninstall" {
                    let service_file = resonate::settings::dirs_config_dir().join("systemd").join("user").join("resonate.service");
                    if service_file.exists() {
                        let _ = std::fs::remove_file(&service_file);
                        println!("Removed {}", service_file.display());
                        println!("Run: systemctl --user daemon-reload");
                    } else {
                        println!("No service file found at {}", service_file.display());
                    }
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

            device_mgr.set_dbus_connection(_conn.clone()).await;

            println!("Resonate D-Bus services registered: org.headbridge & org.resonate on /org/headbridge");
            println!("Monitoring BlueZ device connections...");

            // Spawn auto-connect and monitoring loop
            tokio::spawn(device_mgr.clone().run_monitoring_loop());

            // Keep daemon alive
            std::future::pending::<()>().await;
        }

        Commands::Watch { format } => {
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
                    eprintln!("Failed to connect to Resonate D-Bus interface: {}", e);
                    std::process::exit(1);
                }
            };

            let initial_status: String = match proxy.call("GetStatus", &()).await {
                Ok(s) => s,
                Err(_) => r#"{"connected":false}"#.to_string(),
            };
            print_formatted_status(&initial_status, &format);

            let mut signal_stream = match proxy.receive_signal("StateChanged").await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to subscribe to StateChanged signal: {}", e);
                    std::process::exit(1);
                }
            };

            while let Some(msg) = signal_stream.next().await {
                if let Ok((status_json,)) = msg.body().deserialize::<(String,)>() {
                    print_formatted_status(&status_json, &format);
                }
            }
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
                    let target_mode = match mode.as_deref() {
                        Some("toggle") | None => {
                            let status_res: Result<String, _> = proxy.call("GetStatus", &()).await;
                            let current = match status_res {
                                Ok(s) => {
                                    let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
                                    v["noiseMode"].as_str().unwrap_or("off").to_string()
                                }
                                Err(_) => "off".to_string(),
                            };
                            match current.as_str() {
                                "anc" => "ambient".to_string(),
                                "ambient" => "off".to_string(),
                                _ => "anc".to_string(),
                            }
                        }
                        Some(m) => m.to_string(),
                    };

                    let res: Result<(bool, String), _> = proxy.call("SetAnc", &(target_mode,)).await;
                    match res {
                        Ok((_, msg)) => println!("{}", msg),
                        Err(e) => eprintln!("Failed to set ANC mode: {}", e),
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

        Commands::Mic { mode } => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let target = mode.unwrap_or_else(|| "auto".to_string());
                    let res: Result<bool, _> = proxy.call("SetMicrophoneMode", &(target.clone(),)).await;
                    if let Ok(true) = res {
                        println!("Microphone routing set to: {}", target);
                    } else {
                        eprintln!("Failed to set microphone mode.");
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::OneBudAnc { action } => {
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
                    let res: Result<bool, _> = proxy.call("SetOneBudAnc", &(enable,)).await;
                    if let Ok(true) = res {
                        println!("Single-bud ANC set to: {}", enable);
                    } else {
                        eprintln!("Failed to set single-bud ANC.");
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::Chime { volume } => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let vol = volume.unwrap_or(80);
                    let res: Result<bool, _> = proxy.call("SetChimeVolume", &(vol,)).await;
                    if let Ok(true) = res {
                        println!("Chime tone volume set to: {}", vol);
                    } else {
                        eprintln!("Failed to set chime volume.");
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::AmbientLevel { level } => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let lvl = level.unwrap_or(10);
                    let res: Result<bool, _> = proxy.call("SetAmbientLevel", &(lvl,)).await;
                    if let Ok(true) = res {
                        println!("Ambient sound level set to: {}", lvl);
                    } else {
                        eprintln!("Failed to set ambient level.");
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::Eq { args } => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let arg_str = args.join(" ");
                    let res: Result<(bool, String), _> = proxy.call("SetEqualizer", &(arg_str,)).await;
                    match res {
                        Ok((_, msg)) => println!("{}", msg),
                        Err(e) => eprintln!("Equalizer call failed: {}", e),
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::Standby { val } => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let target = val.unwrap_or_else(|| "15".to_string());
                    let res: Result<bool, _> = proxy.call("SetStandby", &(target.clone(),)).await;
                    if let Ok(true) = res {
                        println!("Auto-standby set to: {} minutes", target);
                    } else {
                        eprintln!("Failed to set standby timer.");
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::Button { action } => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let act = action.unwrap_or_else(|| "anc".to_string());
                    let res: Result<(bool, String), _> = proxy.call("SetActionButton", &(act,)).await;
                    match res {
                        Ok((_, msg)) => println!("{}", msg),
                        Err(e) => eprintln!("Action button call failed: {}", e),
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::Touch { action } => {
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
                    let res: Result<bool, _> = proxy.call("SetTouchSensor", &(enable,)).await;
                    if let Ok(true) = res {
                        println!("Touch sensor set to: {}", enable);
                    } else {
                        eprintln!("Failed to set touch sensor.");
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::Dsee { action } => {
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
                    let res: Result<bool, _> = proxy.call("SetDsee", &(enable,)).await;
                    if let Ok(true) = res {
                        println!("DSEE audio enhancement set to: {}", enable);
                    } else {
                        eprintln!("Failed to set DSEE.");
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }

        Commands::Optimize { action } => {
            if let Ok(conn) = zbus::Connection::session().await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &conn,
                    "org.headbridge",
                    "/org/headbridge",
                    "org.headbridge.Manager",
                ).await {
                    let act = action.unwrap_or_else(|| "start".to_string());
                    let res: Result<(bool, String), _> = proxy.call("Optimize", &(act,)).await;
                    match res {
                        Ok((_, msg)) => println!("{}", msg),
                        Err(e) => eprintln!("Optimize call failed: {}", e),
                    }
                    return Ok(());
                }
            }
            eprintln!("Resonate daemon is not running.");
        }
    }

    Ok(())
}

fn format_waybar(status_json: &str) -> String {
    let val: serde_json::Value = serde_json::from_str(status_json).unwrap_or_default();
    let connected = val["connected"].as_bool().unwrap_or(false);

    if !connected {
        return serde_json::json!({
            "text": "🎧 Disconnected",
            "tooltip": "No supported headphones connected",
            "class": "disconnected",
            "percentage": 0
        }).to_string();
    }

    let dev_name = val["deviceName"].as_str().unwrap_or("Headphones");
    let vendor = val["vendor"].as_str().unwrap_or("");
    let batt = val["batteryPercent"].as_i64().unwrap_or(-1);
    let left = val["batteryLeft"].as_i64();
    let right = val["batteryRight"].as_i64();
    let case = val["batteryCase"].as_i64();
    let noise_mode = val["noiseMode"].as_str().unwrap_or("off");
    let is_charging = val["isCharging"].as_bool().unwrap_or(false);
    let sink = val["audioSink"].as_str().unwrap_or("");

    let mode_tag = match noise_mode {
        "anc" => "ANC",
        "ambient" => "Ambient",
        "adaptive" => "Adaptive",
        _ => "Off",
    };

    let text = if batt >= 0 {
        format!("🎧 {}% ({}){}", batt, mode_tag, if is_charging { " ⚡" } else { "" })
    } else {
        format!("🎧 ({})", mode_tag)
    };

    let mut tooltip = format!("{}\nVendor: {}\nMode: {}", dev_name, vendor, noise_mode);
    if let (Some(l), Some(r)) = (left, right) {
        tooltip.push_str(&format!("\nLeft: {}% | Right: {}%", l, r));
    }
    if let Some(b) = case {
        tooltip.push_str(&format!("\nCase: {}%", b));
    }
    if !sink.is_empty() {
        tooltip.push_str(&format!("\nSink: {}", sink));
    }

    serde_json::json!({
        "text": text,
        "tooltip": tooltip,
        "class": format!("connected {}", noise_mode),
        "percentage": batt.max(0)
    }).to_string()
}

fn print_formatted_status(status_json: &str, format: &str) {
    match format.to_lowercase().as_str() {
        "raw" | "json" => {
            println!("{}", status_json.trim());
        }
        "compact" => {
            let val: serde_json::Value = serde_json::from_str(status_json).unwrap_or_default();
            if val["connected"].as_bool().unwrap_or(false) {
                let batt = val["batteryPercent"].as_i64().unwrap_or(-1);
                let mode = val["noiseMode"].as_str().unwrap_or("off");
                println!("🎧 {}% • {}", batt, mode.to_uppercase());
            } else {
                println!("🎧 Disconnected");
            }
        }
        _ => {
            // Default: Waybar JSON
            println!("{}", format_waybar(status_json));
        }
    }
    let _ = std::io::stdout().flush();
}
