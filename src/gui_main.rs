//! Resonate Studio: Native Linux Bluetooth Headphone GUI for AirPods and Bowers & Wilkins.
//!
//! Provides the primary desktop graphical interface for inspecting battery telemetry,
//! switching Active Noise Cancellation listening modes, tuning hardware equalizers,
//! managing multipoint connections, and supervising the background daemon.

slint::include_modules!();

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
    ConnectDevice(String),
    DisconnectDevice,
    ConnectMultipoint(String),
    RefreshDevices,
    StartDaemon,
    StopDaemon,
    RestartDaemon,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let main_window = MainWindow::new()?;

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
                    }

                    // Initial fetch of paired devices
                    if let Ok(devs) = client.list_devices().await {
                        let dev_entries: Vec<DeviceEntry> = devs
                            .into_iter()
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

                    // Subscribe to StateChanged signals
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
                                    add_log(&weak_ui, &logs_store_bg, &log_msg, false);
                                }
                            }
                        }
                    }

                    // If stream closed, daemon likely disconnected
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
            let client_opt = client_cmd_clone.lock().await.clone();

            match cmd {
                GuiCommand::SetAnc(mode) => {
                    if let Some(c) = client_opt {
                        let res = c.set_anc(&mode).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set ANC mode: {}", mode),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::SetAutoPause(val) => {
                    if let Some(c) = client_opt {
                        let res = c.set_auto_pause(val).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Auto-Pause: {}", val),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::SetConversationAwareness(val) => {
                    if let Some(c) = client_opt {
                        let res = c.set_conversation_awareness(val).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Conversation Awareness: {}", val),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::SetOneBudAnc(val) => {
                    if let Some(c) = client_opt {
                        let res = c.set_one_bud_anc(val).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Single-Bud ANC: {}", val),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::SetWearSensor(val) => {
                    if let Some(c) = client_opt {
                        let res = c.set_wear_sensor(val).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Wear Sensor: {}", val),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::SetEqualizer(bass, treble) => {
                    if let Some(c) = client_opt {
                        let args = format!("{:+0.1} {:+0.1}", bass, treble);
                        let res = c.set_equalizer(&args).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Equalizer: {}", args),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::ApplyPreset(preset) => {
                    if let Some(c) = client_opt {
                        let args = match preset.as_str() {
                            "flat" => "0.0 0.0",
                            "bass_boost" => "+4.0 0.0",
                            "vocal" => "+1.0 +3.0",
                            "treble_boost" => "0.0 +4.0",
                            other => other,
                        };
                        let res = c.set_equalizer(args).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Applied Preset: {}", preset),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::SetMicrophoneMode(mode) => {
                    if let Some(c) = client_opt {
                        let res = c.set_microphone_mode(&mode).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Mic Mode: {}", mode),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::SetChimeVolume(vol) => {
                    if let Some(c) = client_opt {
                        let res = c.set_chime_volume(vol).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Chime Volume: {}%", vol),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::SetStandby(val) => {
                    if let Some(c) = client_opt {
                        let res = c.set_standby(&val).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Auto-Standby: {}", val),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::SetActionButton(action) => {
                    if let Some(c) = client_opt {
                        let res = c.set_action_button(&action).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Set Action Button: {}", action),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::ConnectDevice(addr) => {
                    if let Some(c) = client_opt {
                        let res = c.connect_device(&addr).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Connect Device: {}", addr),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::DisconnectDevice => {
                    if let Some(c) = client_opt {
                        let res = c.disconnect_device().await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            "Disconnected device session",
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::ConnectMultipoint(target) => {
                    if let Some(c) = client_opt {
                        let res = c.connect_multipoint(&target).await;
                        log_action(
                            &weak_ui_cmd,
                            &logs_store_cmd,
                            &format!("Connect Multipoint: {}", target),
                            res.is_ok(),
                        );
                    }
                }
                GuiCommand::RefreshDevices => {
                    if let Some(c) = client_opt {
                        if let Ok(devs) = c.list_devices().await {
                            let dev_entries: Vec<DeviceEntry> = devs
                                .into_iter()
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
            let _ = tx.blocking_send(GuiCommand::SetAnc(mode.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_auto_pause(move |val| {
            let _ = tx.blocking_send(GuiCommand::SetAutoPause(val));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_conv_aware(move |val| {
            let _ = tx.blocking_send(GuiCommand::SetConversationAwareness(val));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_one_bud_anc(move |val| {
            let _ = tx.blocking_send(GuiCommand::SetOneBudAnc(val));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_ear_detection(move |val| {
            let _ = tx.blocking_send(GuiCommand::SetWearSensor(val));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_equalizer(move |bass, treble| {
            let _ = tx.blocking_send(GuiCommand::SetEqualizer(bass, treble));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_apply_preset(move |preset| {
            let _ = tx.blocking_send(GuiCommand::ApplyPreset(preset.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_mic_mode(move |mode| {
            let _ = tx.blocking_send(GuiCommand::SetMicrophoneMode(mode.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_chime_volume(move |vol| {
            let _ = tx.blocking_send(GuiCommand::SetChimeVolume(vol as u8));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_standby(move |val| {
            let _ = tx.blocking_send(GuiCommand::SetStandby(val.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_set_action_button(move |action| {
            let _ = tx.blocking_send(GuiCommand::SetActionButton(action.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_connect_device(move |addr| {
            let _ = tx.blocking_send(GuiCommand::ConnectDevice(addr.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_disconnect_device(move |_| {
            let _ = tx.blocking_send(GuiCommand::DisconnectDevice);
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_connect_multipoint(move |addr| {
            let _ = tx.blocking_send(GuiCommand::ConnectMultipoint(addr.to_string()));
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_refresh_devices(move || {
            let _ = tx.blocking_send(GuiCommand::RefreshDevices);
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_start_daemon(move || {
            let _ = tx.blocking_send(GuiCommand::StartDaemon);
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_stop_daemon(move || {
            let _ = tx.blocking_send(GuiCommand::StopDaemon);
        });
    }

    {
        let tx = cmd_tx.clone();
        main_window.on_restart_daemon(move || {
            let _ = tx.blocking_send(GuiCommand::RestartDaemon);
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
        ui.set_wear_sensitivity(status_clone.wear_sensitivity.unwrap_or(1));

        ui.set_auto_pause(status_clone.auto_pause);
        ui.set_conv_aware(status_clone.conversation_awareness);
        ui.set_one_bud_anc(status_clone.one_bud_anc);
        ui.set_ear_detection(status_clone.ear_detection);
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
