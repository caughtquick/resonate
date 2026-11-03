use std::time::{Duration, Instant};
use tokio::process::Command;
use zbus::Connection;
use zbus::fdo::DBusProxy;

use crate::protocols::aap::InEarStatus;

pub struct AudioMonitor;

impl AudioMonitor {
    pub async fn get_default_sink() -> Option<String> {
        let output = Command::new("pactl")
            .arg("info")
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(sink) = trimmed.strip_prefix("Default Sink: ") {
                return Some(sink.trim().to_string());
            }
        }

        None
    }
}

pub struct MprisController;

impl MprisController {
    /// Queries the session D-Bus for all active MPRIS media players (org.mpris.MediaPlayer2.*)
    pub async fn get_mpris_players(conn: &Connection) -> Vec<String> {
        if let Ok(dbus) = DBusProxy::new(conn).await {
            if let Ok(names) = dbus.list_names().await {
                return names
                    .into_iter()
                    .filter_map(|name| {
                        let s = name.as_str();
                        if s.starts_with("org.mpris.MediaPlayer2.") {
                            Some(s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
            }
        }
        Vec::new()
    }

    /// Gets the playback status of a given player ("Playing", "Paused", "Stopped")
    pub async fn get_playback_status(conn: &Connection, player_bus_name: &str) -> Option<String> {
        let proxy = zbus::Proxy::new(
            conn,
            player_bus_name,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ).await.ok()?;

        proxy.get_property::<String>("PlaybackStatus").await.ok()
    }

    /// Pauses a specific player via MPRIS
    pub async fn pause_player(conn: &Connection, player_bus_name: &str) -> bool {
        if let Ok(proxy) = zbus::Proxy::new(
            conn,
            player_bus_name,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ).await {
            let res: Result<(), _> = proxy.call("Pause", &()).await;
            return res.is_ok();
        }
        false
    }

    /// Resumes (Play) a specific player via MPRIS
    pub async fn play_player(conn: &Connection, player_bus_name: &str) -> bool {
        if let Ok(proxy) = zbus::Proxy::new(
            conn,
            player_bus_name,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ).await {
            let res: Result<(), _> = proxy.call("Play", &()).await;
            return res.is_ok();
        }
        false
    }

    /// Pauses all currently playing media players and returns the list of bus names paused
    pub async fn pause_all_playing(conn: &Connection) -> Vec<String> {
        let mut paused = Vec::new();
        let players = Self::get_mpris_players(conn).await;
        for player in players {
            if let Some(status) = Self::get_playback_status(conn, &player).await {
                if status.eq_ignore_ascii_case("Playing") {
                    if Self::pause_player(conn, &player).await {
                        paused.push(player);
                    }
                }
            }
        }

        // Fallback: if playerctl is installed and playing, use it if no players were found above
        if paused.is_empty() {
            if let Ok(status) = Command::new("playerctl").arg("status").output().await {
                if String::from_utf8_lossy(&status.stdout).trim().eq_ignore_ascii_case("Playing") {
                    let _ = Command::new("playerctl").arg("pause").status().await;
                    paused.push("playerctl".to_string());
                }
            }
        }

        paused
    }

    /// Resumes the players that were previously paused by smart auto-pause
    pub async fn resume_players(conn: &Connection, players: &[String]) -> usize {
        let mut resumed = 0;
        for player in players {
            if player == "playerctl" {
                if let Ok(status) = Command::new("playerctl").arg("play").status().await {
                    if status.success() {
                        resumed += 1;
                    }
                }
            } else {
                // If the player is already playing, don't re-trigger
                if let Some(status) = Self::get_playback_status(conn, player).await {
                    if !status.eq_ignore_ascii_case("Playing") {
                        if Self::play_player(conn, player).await {
                            resumed += 1;
                        }
                    }
                } else if Self::play_player(conn, player).await {
                    resumed += 1;
                }
            }
        }
        resumed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPauseAction {
    None,
    Pause,
    Resume,
}

pub struct AutoPauseCoordinator {
    pub enabled: bool,
    pub prev_in_ear_count: Option<usize>,
    pub paused_players: Vec<String>,
    pub paused_at: Option<Instant>,
    pub max_resume_delay: Duration,
}

impl AutoPauseCoordinator {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            prev_in_ear_count: None,
            paused_players: Vec::new(),
            paused_at: None,
            max_resume_delay: Duration::from_secs(600), // 10 minutes
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.paused_players.clear();
            self.paused_at = None;
        }
    }

    pub fn reset(&mut self) {
        self.prev_in_ear_count = None;
        self.paused_players.clear();
        self.paused_at = None;
    }

    pub fn get_paused_players(&self) -> &[String] {
        &self.paused_players
    }

    /// Pure transition evaluator without I/O, ideal for unit testing and deterministic logic
    pub fn evaluate_ear_transition(
        &mut self,
        current_count: usize,
        both_in_case: bool,
    ) -> AutoPauseAction {
        if !self.enabled {
            self.prev_in_ear_count = Some(current_count);
            return AutoPauseAction::None;
        }

        let prev = match self.prev_in_ear_count {
            Some(c) => c,
            None => {
                // Initial baseline established on connect
                self.prev_in_ear_count = Some(current_count);
                return AutoPauseAction::None;
            }
        };

        let mut action = AutoPauseAction::None;

        if current_count < prev {
            // Earbud removed!
            action = AutoPauseAction::Pause;
        } else if current_count > prev {
            // Earbud inserted / returned!
            if !self.paused_players.is_empty() {
                let can_resume = self.paused_at.map_or(false, |t| t.elapsed() <= self.max_resume_delay);
                if can_resume {
                    action = AutoPauseAction::Resume;
                }
                self.paused_players.clear();
                self.paused_at = None;
            }
        }

        if both_in_case {
            self.paused_players.clear();
            self.paused_at = None;
        }

        self.prev_in_ear_count = Some(current_count);
        action
    }

    /// Handles an in-ear status telemetry update from earbuds
    pub async fn on_ear_status_update(&mut self, in_ear: InEarStatus, conn: &Connection) {
        let current_count = in_ear.in_ear_count();
        let both_in_case = in_ear.are_both_in_case();

        let action = self.evaluate_ear_transition(current_count, both_in_case);

        match action {
            AutoPauseAction::Pause => {
                let paused = MprisController::pause_all_playing(conn).await;
                if !paused.is_empty() {
                    println!(
                        "Smart Auto-Pause: Earbud removal detected. Paused media on: {:?}",
                        paused
                    );
                    self.paused_players = paused;
                    self.paused_at = Some(Instant::now());
                }
            }
            AutoPauseAction::Resume => {
                let count_resumed = MprisController::resume_players(conn, &self.paused_players).await;
                println!(
                    "Smart Auto-Pause: Earbud re-inserted. Resumed {} player(s)",
                    count_resumed
                );
            }
            AutoPauseAction::None => {}
        }
    }

    /// Handles wear sensor toggle/status update (for single wear sensors, over-ear headphones, B&W)
    pub async fn on_wear_sensor_update(&mut self, is_worn: bool, conn: &Connection) {
        let current_count = if is_worn { 1 } else { 0 };
        let action = self.evaluate_ear_transition(current_count, false);

        match action {
            AutoPauseAction::Pause => {
                let paused = MprisController::pause_all_playing(conn).await;
                if !paused.is_empty() {
                    println!(
                        "Smart Auto-Pause: Headphone removed. Paused media on: {:?}",
                        paused
                    );
                    self.paused_players = paused;
                    self.paused_at = Some(Instant::now());
                }
            }
            AutoPauseAction::Resume => {
                let count_resumed = MprisController::resume_players(conn, &self.paused_players).await;
                println!(
                    "Smart Auto-Pause: Headphone put back on. Resumed {} player(s)",
                    count_resumed
                );
            }
            AutoPauseAction::None => {}
        }
    }

    /// Handles Bluetooth disconnection while audio was active
    pub async fn on_disconnect(&mut self, conn: &Connection) {
        if self.enabled {
            let _ = MprisController::pause_all_playing(conn).await;
        }
        self.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::aap::EarStatus;

    #[tokio::test]
    async fn test_get_default_sink() {
        if let Some(sink) = AudioMonitor::get_default_sink().await {
            println!("Detected default audio sink: {}", sink);
            assert!(!sink.is_empty());
        }
    }

    #[tokio::test]
    async fn test_zbus_fdo_list_names() {
        if let Ok(conn) = zbus::Connection::session().await {
            let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
            let names = dbus.list_names().await.unwrap();
            println!("DBus names count: {}", names.len());
            assert!(!names.is_empty());
        }
    }

    #[test]
    fn test_auto_pause_dual_earbuds_removal_and_resume() {
        let mut coordinator = AutoPauseCoordinator::new(true);

        // Initial connect with both earbuds in-ear: baseline established, no action
        let action0 = coordinator.evaluate_ear_transition(2, false);
        assert_eq!(action0, AutoPauseAction::None);
        assert_eq!(coordinator.prev_in_ear_count, Some(2));

        // User removes one earbud (2 -> 1)
        let action1 = coordinator.evaluate_ear_transition(1, false);
        assert_eq!(action1, AutoPauseAction::Pause);
        assert_eq!(coordinator.prev_in_ear_count, Some(1));

        // Simulate that a media player was paused
        coordinator.paused_players = vec!["org.mpris.MediaPlayer2.spotify".to_string()];
        coordinator.paused_at = Some(Instant::now());

        // User puts the earbud back in (1 -> 2)
        let action2 = coordinator.evaluate_ear_transition(2, false);
        assert_eq!(action2, AutoPauseAction::Resume);
        assert!(coordinator.paused_players.is_empty());
    }

    #[test]
    fn test_auto_pause_single_earbud_removal_and_resume() {
        let mut coordinator = AutoPauseCoordinator::new(true);

        // User starts with only one earbud in ear (1 in-ear, 1 in case)
        let action0 = coordinator.evaluate_ear_transition(1, false);
        assert_eq!(action0, AutoPauseAction::None);

        // User removes the single earbud (1 -> 0)
        let action1 = coordinator.evaluate_ear_transition(0, false);
        assert_eq!(action1, AutoPauseAction::Pause);

        // Simulate paused media
        coordinator.paused_players = vec!["org.mpris.MediaPlayer2.spotify".to_string()];
        coordinator.paused_at = Some(Instant::now());

        // User puts the earbud back in (0 -> 1)
        let action2 = coordinator.evaluate_ear_transition(1, false);
        assert_eq!(action2, AutoPauseAction::Resume);
    }

    #[test]
    fn test_auto_pause_both_in_case_clears_resume() {
        let mut coordinator = AutoPauseCoordinator::new(true);

        // Both in-ear initially
        coordinator.evaluate_ear_transition(2, false);

        // User takes them out and puts in case
        let action1 = coordinator.evaluate_ear_transition(0, true);
        assert_eq!(action1, AutoPauseAction::Pause);

        coordinator.paused_players = vec!["org.mpris.MediaPlayer2.spotify".to_string()];
        coordinator.paused_at = Some(Instant::now());

        // Notification confirms both are in case
        let action2 = coordinator.evaluate_ear_transition(0, true);
        assert_eq!(action2, AutoPauseAction::None);
        assert!(coordinator.paused_players.is_empty());
    }

    #[test]
    fn test_auto_pause_timeout_expires() {
        let mut coordinator = AutoPauseCoordinator::new(true);
        coordinator.evaluate_ear_transition(2, false);

        // Removed
        coordinator.evaluate_ear_transition(1, false);
        coordinator.paused_players = vec!["org.mpris.MediaPlayer2.spotify".to_string()];
        // Simulate paused 15 minutes ago (timeout is 10 minutes)
        coordinator.paused_at = Some(Instant::now() - Duration::from_secs(900));

        // Re-inserted after timeout
        let action = coordinator.evaluate_ear_transition(2, false);
        assert_eq!(action, AutoPauseAction::None); // Did not resume because timeout expired!
        assert!(coordinator.paused_players.is_empty());
    }

    #[test]
    fn test_auto_pause_disabled_does_nothing() {
        let mut coordinator = AutoPauseCoordinator::new(false);

        coordinator.evaluate_ear_transition(2, false);
        let action = coordinator.evaluate_ear_transition(1, false);
        assert_eq!(action, AutoPauseAction::None);
        assert!(coordinator.paused_players.is_empty());
    }

    #[test]
    fn test_in_ear_status_integration_helpers() {
        let both_in = InEarStatus {
            primary: EarStatus::InEar,
            secondary: EarStatus::InEar,
        };
        assert_eq!(both_in.in_ear_count(), 2);
        assert!(both_in.is_worn());
        assert!(!both_in.are_both_in_case());

        let one_out = InEarStatus {
            primary: EarStatus::OutOfEar,
            secondary: EarStatus::InEar,
        };
        assert_eq!(one_out.in_ear_count(), 1);
        assert!(one_out.is_worn());

        let both_case = InEarStatus {
            primary: EarStatus::InCase,
            secondary: EarStatus::InCase,
        };
        assert_eq!(both_case.in_ear_count(), 0);
        assert!(!both_case.is_worn());
        assert!(both_case.are_both_in_case());
    }
}
