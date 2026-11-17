//! Resonate Studio: Native Linux Bluetooth Headphone GUI for AirPods and Bowers & Wilkins.
//!
//! Provides the primary desktop graphical interface for inspecting battery telemetry,
//! switching Active Noise Cancellation listening modes, tuning hardware equalizers,
//! managing multipoint connections, and supervising the background daemon.

use resonate::gui::{DeviceEntry, LogItem, MainWindow};

use futures_lite::stream::StreamExt;
use resonate::gui::dbus::{parse_eq_gains, ResonateDbusClient, ResonateStatusDto};
use resonate::gui::service;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

/// Generates a local timestamp string (`HH:MM:SS`) for diagnostics and event logs.
fn current_time_str() -> String {
    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();
    let hours = (secs / 3600) % 24;
    let minutes = (secs / 60) % 60;
    let seconds = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

/// Commands sent from the Slint UI thread to the background async Tokio runtime.
#[derive(Debug)]
enum GuiCommand {
    SetAnc(String),
    SetAutoPause(bool),
    SetConversationAwareness(bool),
    SetOneBudAnc(bool),
    SetWearSensor(bool),
    SetEqualizer(f32, f32),
    ApplyPreset(String),
    SetMicrophoneMode(String),
    SetChimeVolume(u8),
    SetStandby(String),
    SetActionButton(String),
    SetWearSensitivity(String),
    ConnectDevice(String),
    DisconnectDevice,
    ConnectMultipoint(String),
    DisconnectMultipoint(String),
    RefreshDevices,
    StartDaemon,
    StopDaemon,
    RestartDaemon,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let main_window = MainWindow::new()?;

    // Initialize and watch user theme from ~/.config/resonate-studio/config.toml
    let studio_config = resonate::gui::theme::ensure_config_file();
    resonate::gui::theme::apply_theme_to_ui(&main_window, &studio_config);
    resonate::gui::theme::start_theme_watcher(main_window.as_weak());

    // Thread-safe log store
    let logs_store: Arc<Mutex<Vec<LogItem>>> = Arc::new(Mutex::new(vec![LogItem {
        time: current_time_str().into(),
        message: "Resonate Studio initialized. Connecting to D-Bus session bus...".into(),
        is_error: false,
    }]));

    // Initialize UI logs
    {
        let initial_logs = logs_store.lock().unwrap().clone();
        let model = Rc::new(VecModel::from(initial_logs));
        main_window.set_logs(ModelRc::from(model));
    }

    // Communication channel between UI callbacks and Tokio async worker
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<GuiCommand>(64);

    let client_holder: Arc<tokio::sync::Mutex<Option<ResonateDbusClient>>> =
        Arc::new(tokio::sync::Mutex::new(None));
    let client_clone = client_holder.clone();

    let weak_ui = main_window.as_weak();
    let logs_store_bg = logs_store.clone();

    // Spawn background task for D-Bus connection & event streaming
    tokio::spawn(async move {
        loop {
            // Attempt connection to org.headbridge.Manager
            match ResonateDbusClient::connect().await {
                Ok(client) => {
                    *client_clone.lock().await = Some(client.clone());

                    add_log(
                        &weak_ui,
                        &logs_store_bg,
                        "Connected to Resonate Daemon (org.headbridge.Manager)",
                        false,
                    );

                    let _ = weak_ui.upgrade_in_event_loop(|ui| {
                        ui.set_daemon_running(true);
                        ui.set_daemon_status("Active (Running)".into());
                    });

                    // Initial fetch of device status
                    if let Ok(status) = client.get_status().await {
                        update_ui_status(&weak_ui, &status);
                        if status.vendor == "Bowers & Wilkins" {
                            refresh_multipoint_slots(&weak_ui, &client).await;
                        }
                    }

                    // Initial fetch of paired devices (supported headphones only)
                    if let Ok(devs) = client.list_devices().await {
                        let dev_entries: Vec<DeviceEntry> = devs
                            .into_iter()
                            .filter(|d| d.is_supported())
                            .map(|d| {
                                let vendor = d.vendor_display().into();
                                DeviceEntry {
                                    name: d.name.into(),
                                    address: d.address.into(),
                                    vendor,
                                    connected: d.connected,
                                    is_multipoint: false,
                                }
                            })
                            .collect();

                        let _ = weak_ui.upgrade_in_event_loop(move |ui| {
                            let model = Rc::new(VecModel::from(dev_entries));
                            ui.set_devices(ModelRc::from(model));
                        });
                    }

                    // Spawn a periodic polling task to ensure telemetry stays 100% in sync
                    let client_poll = client.clone();
                    let weak_poll = weak_ui.clone();
                    let poll_handle = tokio::spawn(async move {
                        loop {
                            tokio::time::sleep(Duration::from_millis(1500)).await;
                            if let Ok(status) = client_poll.get_status().await {
                                update_ui_status(&weak_poll, &status);
                            } else {
                                break;
                            }
                        }
                    });

                    // Subscribe to StateChanged signals
                    let mut last_telemetry_sig = String::new();
                    if let Ok(mut stream) = client.subscribe_state_changed().await {
                        while let Some(msg) = stream.next().await {
                            if let Ok((json_str,)) = msg.body().deserialize::<(String,)>() {
                                if let Ok(status) = serde_json::from_str::<ResonateStatusDto>(&json_str) {
                                    update_ui_status(&weak_ui, &status);

                                    let dev_name = if status.device_name.is_empty() {
                                        "Device".to_string()
                                    } else {
                                        status.device_name.clone()
                                    };
                                    let anc_mode = status.noise_mode.clone();
                                    let bat = status.battery_percent.unwrap_or(-1);

                                    let log_msg = format!(
                                        "Telemetry: {} [ANC: {}, Battery: {}%]",
                                        dev_name,
                                        anc_mode,
                                        if bat >= 0 { bat.to_string() } else { "--".to_string() }
                                    );
                                    // Deduplicate telemetry logs so identical periodic telemetry does not spam the stream
                                    if log_msg != last_telemetry_sig {
                                        last_telemetry_sig = log_msg.clone();
                                        add_log(&weak_ui, &logs_store_bg, &log_msg, false);
                                    }
                                }
                            }
                        }
                    }

                    poll_handle.abort();
                    *client_clone.lock().await = None;
                }
                Err(e) => {
                    let (is_act, status_str) = service::is_service_active();
                    let _ = weak_ui.upgrade_in_event_loop(move |ui| {
                        ui.set_daemon_running(is_act);
                        ui.set_daemon_status(status_str.into());
                        ui.set_connected(false);
                        ui.set_device_name("Disconnected".into());
                    });

                    add_log(
                        &weak_ui,
                        &logs_store_bg,
                        &format!("D-Bus disconnected: {}. Retrying...", e),
                        true,
                    );
                }
            }

            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });

    // Spawn command processor handling UI clicks
    let client_cmd_clone = client_holder.clone();
    let weak_ui_cmd = main_window.as_weak();
    let logs_store_cmd = logs_store.clone();

    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            // Reconnect client on-demand if not already connected
            let mut client_res = client_cmd_clone.lock().await.clone();
            if client_res.is_none() {
                if let Ok(c) = ResonateDbusClient::connect().await {
                    *client_cmd_clone.lock().await = Some(c.clone());
                    client_res = Some(c);
                }
            }

            match cmd {
                GuiCommand::SetAnc(mode) => {
                    let weak = weak_ui_cmd.clone();
                    let m_clone = mode.clone();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_noise_mode(m_clone.into());
                    });

                    if let Some(c) = client_res.as_ref() {
                        let res = c.set_anc(&mode).await;
                        let ok = res.as_ref().map(|(b, _)| *b).unwrap_or(false);
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set ANC mode: {}", mode),
                            ok,
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::SetAutoPause(val) => {
                    let weak = weak_ui_cmd.clone();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_auto_pause(val);
                    });

                    if let Some(c) = client_res.as_ref() {
                        let res = c.set_auto_pause(val).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Auto-Pause: {}", if val { "Enabled" } else { "Disabled" }),
                            res.unwrap_or(false),
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::SetConversationAwareness(val) => {
                    let weak = weak_ui_cmd.clone();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_conv_aware(val);
                    });

                    if let Some(c) = client_res.as_ref() {
                        let res = c.set_conversation_awareness(val).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Conversation Awareness: {}", if val { "Enabled" } else { "Disabled" }),
                            res.unwrap_or(false),
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::SetOneBudAnc(val) => {
                    let weak = weak_ui_cmd.clone();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_one_bud_anc(val);
                    });

                    if let Some(c) = client_res.as_ref() {
                        let res = c.set_one_bud_anc(val).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Single-Bud ANC: {}", if val { "Enabled" } else { "Disabled" }),
                            res.unwrap_or(false),
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::SetWearSensor(val) => {
                    let weak = weak_ui_cmd.clone();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_ear_detection(val);
                    });

                    if let Some(c) = client_res.as_ref() {
                        let res = c.set_wear_sensor(val).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Wear Sensor: {}", if val { "Enabled" } else { "Disabled" }),
                            res.unwrap_or(false),
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::SetEqualizer(bass, treble) => {
                    let weak = weak_ui_cmd.clone();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_bass_db(bass);
                        ui.set_treble_db(treble);
                    });

                    if let Some(c) = client_res.as_ref() {
                        let args = format!("{:+0.1} {:+0.1}", bass, treble);
                        let res = c.set_equalizer(&args).await;
                        let ok = res.as_ref().map(|(b, _)| *b).unwrap_or(false);
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Equalizer: {}", args),
                            ok,
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::ApplyPreset(preset) => {
                    if let Some(c) = client_res.as_ref() {
                        let args = match preset.as_str() {
                            "flat" => "0.0 0.0",
                            "bass_boost" => "+4.0 0.0",
                            "vocal" => "+1.0 +3.0",
                            "treble_boost" => "0.0 +4.0",
                            other => other,
                        };
                        let res = c.set_equalizer(args).await;
                        let ok = res.as_ref().map(|(b, _)| *b).unwrap_or(false);
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Applied Preset: {}", preset),
                            ok,
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::SetMicrophoneMode(mode) => {
                    let weak = weak_ui_cmd.clone();
                    let m_clone = mode.clone();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_mic_mode(m_clone.into());
                    });

                    if let Some(c) = client_res.as_ref() {
                        let res = c.set_microphone_mode(&mode).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Mic Mode: {}", mode),
                            res.unwrap_or(false),
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::SetChimeVolume(vol) => {
                    let weak = weak_ui_cmd.clone();
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_chime_volume(vol as i32);
                    });

                    if let Some(c) = client_res.as_ref() {
                        let res = c.set_chime_volume(vol).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Chime Volume: {}%", vol),
                            res.unwrap_or(false),
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::SetStandby(val) => {
                    let weak = weak_ui_cmd.clone();
                    let v_num: i32 = val.parse().unwrap_or(15);
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_standby_minutes(v_num);
                    });

                    if let Some(c) = client_res.as_ref() {
                        let res = c.set_standby(&val).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Auto-Standby: {}", val),
                            res.unwrap_or(false),
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::SetActionButton(action) => {
                    if let Some(c) = client_res.as_ref() {
                        let res = c.set_action_button(&action).await;
                        let ok = res.as_ref().map(|(b, _)| *b).unwrap_or(false);
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Action Button: {}", action),
                            ok,
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::SetWearSensitivity(sens) => {
                    if let Some(c) = client_res.as_ref() {
                        let res = c.set_wear_sensor_string(&sens).await;
                        let ok = res.as_ref().map(|(b, _)| *b).unwrap_or(false);
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Wear Sensitivity: {}", sens),
                            ok,
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                    }
                }
                GuiCommand::ConnectDevice(addr) => {
                    if let Some(c) = client_res.as_ref() {
                        let res = c.connect_device(&addr).await;
                        let ok = res.as_ref().map(|(b, _)| *b).unwrap_or(false);
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Connect Device: {}", addr),
                            ok,
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                            if status.vendor == "Bowers & Wilkins" {
                                refresh_multipoint_slots(&weak_ui_cmd, c).await;
                            }
                        }
                        if let Ok(devs) = c.list_devices().await {
                            let dev_entries: Vec<DeviceEntry> = devs
                                .into_iter()
                                .filter(|d| d.is_supported())
                                .map(|d| {
                                    let vendor = d.vendor_display().into();
                                    DeviceEntry {
                                        name: d.name.into(),
                                        address: d.address.into(),
                                        vendor,
                                        connected: d.connected,
                                        is_multipoint: false,
                                    }
                                })
                                .collect();

                            let _ = weak_ui_cmd.upgrade_in_event_loop(move |ui| {
                                let model = Rc::new(VecModel::from(dev_entries));
                                ui.set_devices(ModelRc::from(model));
                            });
                        }
                    }
                }
                GuiCommand::DisconnectDevice => {
                    if let Some(c) = client_res.as_ref() {
                        let res = c.disconnect_device().await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            "Disconnected device session",
                            res.unwrap_or(false),
                        );
                        if let Ok(status) = c.get_status().await {
                            update_ui_status(&weak_ui_cmd, &status);
                        }
                        if let Ok(devs) = c.list_devices().await {
                            let dev_entries: Vec<DeviceEntry> = devs
                                .into_iter()
                                .filter(|d| d.is_supported())
                                .map(|d| {
                                    let vendor = d.vendor_display().into();
                                    DeviceEntry {
                                        name: d.name.into(),
                                        address: d.address.into(),
                                        vendor,
                                        connected: d.connected,
                                        is_multipoint: false,
                                    }
                                })
                                .collect();

                            let _ = weak_ui_cmd.upgrade_in_event_loop(move |ui| {
                                let model = Rc::new(VecModel::from(dev_entries));
                                ui.set_devices(ModelRc::from(model));
                            });
                        }
                    }
                }
                GuiCommand::ConnectMultipoint(target) => {
                    if let Some(c) = client_res.as_ref() {
                        let res = c.connect_multipoint(&target).await;
                        let ok = res.as_ref().map(|(b, _)| *b).unwrap_or(false);
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Connect Multipoint: {}", target),
                            ok,
                        );
                        refresh_multipoint_slots(&weak_ui_cmd, c).await;
                    }
                }
                GuiCommand::DisconnectMultipoint(target) => {
                    if let Some(c) = client_res.as_ref() {
                        let res = c.disconnect_multipoint(&target).await;
                        let ok = res.as_ref().map(|(b, _)| *b).unwrap_or(false);
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Disconnect Multipoint: {}", target),
                            ok,
                        );
                        refresh_multipoint_slots(&weak_ui_cmd, c).await;
                    }
                }
                GuiCommand::RefreshDevices => {
                    if let Some(c) = client_res.as_ref() {
                        if let Ok(devs) = c.list_devices().await {
                            let dev_entries: Vec<DeviceEntry> = devs
                                .into_iter()
                                .filter(|d| d.is_supported())
                                .map(|d| {
                                    let vendor = d.vendor_display().into();
                                    DeviceEntry {
                                        name: d.name.into(),
                                        address: d.address.into(),
                                        vendor,
                                        connected: d.connected,
                                        is_multipoint: false,
                                    }
                                })
                                .collect();

                            let _ = weak_ui_cmd.upgrade_in_event_loop(move |ui| {
                                let model = Rc::new(VecModel::from(dev_entries));
                                ui.set_devices(ModelRc::from(model));
                            });
                        }
                        refresh_multipoint_slots(&weak_ui_cmd, c).await;
                    }
                }
                GuiCommand::StartDaemon => {
                    let res = service::start_service();
                    log_action(
                        &weak_ui_cmd,
                        &logs_store_cmd,
                        "systemctl --user start resonate.service",
                        res.is_ok(),
                    );
                }
                GuiCommand::StopDaemon => {
                    let res = service::stop_service();
                    log_action(
                        &weak_ui_cmd,
                        &logs_store_cmd,
                        "systemctl --user stop resonate.service",
                        res.is_ok(),
                    );
                }
                GuiCommand::RestartDaemon => {
                    let res = service::restart_service();
                    log_action(
                        &weak_ui_cmd,
                        &logs_store_cmd,
                        "systemctl --user restart resonate.service",
                        res.is_ok(),
                    );
                }
            }
        }
    });

    // Wire Slint callbacks to cmd_tx
    {
        let tx = cmd_tx.clone();
        main_window.on_set_anc(move |mode| {
            let _ = tx.try_send(GuiCommand::SetAnc(mode.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_auto_pause(move |val| {
            let _ = tx.try_send(GuiCommand::SetAutoPause(val));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_conv_aware(move |val| {
            let _ = tx.try_send(GuiCommand::SetConversationAwareness(val));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_one_bud_anc(move |val| {
            let _ = tx.try_send(GuiCommand::SetOneBudAnc(val));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_ear_detection(move |val| {
            let _ = tx.try_send(GuiCommand::SetWearSensor(val));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_equalizer(move |bass, treble| {
            let _ = tx.try_send(GuiCommand::SetEqualizer(bass, treble));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_apply_preset(move |preset| {
            let _ = tx.try_send(GuiCommand::ApplyPreset(preset.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_mic_mode(move |mode| {
            let _ = tx.try_send(GuiCommand::SetMicrophoneMode(mode.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_chime_volume(move |vol| {
            let _ = tx.try_send(GuiCommand::SetChimeVolume(vol as u8));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_standby(move |val| {
            let _ = tx.try_send(GuiCommand::SetStandby(val.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_action_button(move |action| {
            let _ = tx.try_send(GuiCommand::SetActionButton(action.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_wear_sensitivity(move |sens| {
            let _ = tx.try_send(GuiCommand::SetWearSensitivity(sens.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_connect_device(move |addr| {
            let _ = tx.try_send(GuiCommand::ConnectDevice(addr.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_disconnect_device(move |_| {
            let _ = tx.try_send(GuiCommand::DisconnectDevice);
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_connect_multipoint(move |addr| {
            let _ = tx.try_send(GuiCommand::ConnectMultipoint(addr.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_disconnect_multipoint(move |addr| {
            let _ = tx.try_send(GuiCommand::DisconnectMultipoint(addr.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_refresh_devices(move || {
            let _ = tx.try_send(GuiCommand::RefreshDevices);
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_start_daemon(move || {
            let _ = tx.try_send(GuiCommand::StartDaemon);
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_stop_daemon(move || {
            let _ = tx.try_send(GuiCommand::StopDaemon);
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_restart_daemon(move || {
            let _ = tx.try_send(GuiCommand::RestartDaemon);
        });
    }

    {
        let logs_store_clear = logs_store.clone();
        let weak_ui_clear = main_window.as_weak();
        main_window.on_clear_logs(move || {
            logs_store_clear.lock().unwrap().clear();
            let _ = weak_ui_clear.upgrade_in_event_loop(|ui| {
                let model = Rc::new(VecModel::<LogItem>::default());
                ui.set_logs(ModelRc::from(model));
            });
        });
    }

    // Run the Slint GUI event loop
    main_window.run()?;
    Ok(())
}

/// Updates all Slint UI properties using a received `ResonateStatusDto`.
fn update_ui_status(weak: &slint::Weak<MainWindow>, s: &ResonateStatusDto) {
    let status_clone = s.clone();
    let (bass, treble) = parse_eq_gains(&status_clone.equalizer_preset);

    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.set_connected(status_clone.connected);
        ui.set_connection_state(status_clone.connection_state.into());
        ui.set_device_name(status_clone.device_name.into());
        ui.set_vendor(status_clone.vendor.into());
        ui.set_audio_sink(status_clone.audio_sink.into());

        ui.set_battery_percent(status_clone.battery_percent.unwrap_or(-1));
        ui.set_battery_left(status_clone.battery_left.unwrap_or(-1));
        ui.set_battery_right(status_clone.battery_right.unwrap_or(-1));
        ui.set_battery_case(status_clone.battery_case.unwrap_or(-1));
        ui.set_is_charging(status_clone.is_charging);

        ui.set_in_ear_primary(status_clone.in_ear_primary.into());
        ui.set_in_ear_secondary(status_clone.in_ear_secondary.into());

        ui.set_noise_mode(status_clone.noise_mode.into());

        ui.set_bass_db(bass);
        ui.set_treble_db(treble);

        ui.set_mic_mode(status_clone.mic_mode.into());
        ui.set_chime_volume(status_clone.chime_volume);
        ui.set_standby_minutes(status_clone.standby_minutes);
        ui.set_wear_sensitivity(
            status_clone
                .wear_sensitivity
                .unwrap_or(if status_clone.ear_detection { 1 } else { -1 }),
        );
        ui.set_action_button(status_clone.action_button.into());

        ui.set_auto_pause(status_clone.auto_pause);
        ui.set_conv_aware(status_clone.conversation_awareness);
        ui.set_one_bud_anc(status_clone.one_bud_anc);
        ui.set_ear_detection(status_clone.ear_detection);
    });
}

/// Refreshes the Bowers & Wilkins multipoint slots from the D-Bus service.
async fn refresh_multipoint_slots(weak: &slint::Weak<MainWindow>, client: &ResonateDbusClient) {
    if let Ok((true, json_str)) = client.get_multipoint().await {
        if let Ok(slots) =
            serde_json::from_str::<Vec<resonate::gui::dbus::BwMultipointDeviceDto>>(&json_str)
        {
            let entries: Vec<DeviceEntry> = slots
                .into_iter()
                .map(|s| {
                    let name = if s.name.is_empty() {
                        format!("Slot #{}", s.id)
                    } else {
                        s.name
                    };
                    DeviceEntry {
                        name: name.into(),
                        address: s.address.into(),
                        vendor: "Bowers & Wilkins".into(),
                        connected: s.connected,
                        is_multipoint: true,
                    }
                })
                .collect();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let model = Rc::new(VecModel::from(entries));
                ui.set_multipoint_slots(ModelRc::from(model));
            });
            return;
        }
    }
    let _ = weak.upgrade_in_event_loop(|ui| {
        let model = Rc::new(VecModel::<DeviceEntry>::default());
        ui.set_multipoint_slots(ModelRc::from(model));
    });
}

/// Adds a log message to the log store and refreshes the Slint UI logs.
fn add_log(
    weak: &slint::Weak<MainWindow>,
    logs_store: &Arc<Mutex<Vec<LogItem>>>,
    msg: &str,
    is_error: bool,
) {
    let mut guard = logs_store.lock().unwrap();
    guard.push(LogItem {
        time: current_time_str().into(),
        message: msg.into(),
        is_error,
    });
    // Keep max 100 log lines to preserve memory
    if guard.len() > 100 {
        guard.remove(0);
    }
    let snapshot = guard.clone();
    drop(guard);

    let _ = weak.upgrade_in_event_loop(move |ui| {
        let model = Rc::new(VecModel::from(snapshot));
        ui.set_logs(ModelRc::from(model));
    });
}

/// Appends an action outcome log.
fn log_action(
    weak: &slint::Weak<MainWindow>,
    logs_store: &Arc<Mutex<Vec<LogItem>>>,
    msg: &str,
    success: bool,
) {
    let msg_str = format!("{}: {}", if success { "OK" } else { "FAILED" }, msg);
    add_log(weak, logs_store, &msg_str, !success);
}
