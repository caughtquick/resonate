//! Audio sink monitoring and smart auto-pause / auto-resume media player coordination.
//!
//! Integrates with Linux desktop audio stacks using:
//! - `pactl` to query default PulseAudio / PipeWire audio sink names via [`AudioMonitor`].
//! - MPRIS D-Bus interface (`org.mpris.MediaPlayer2.*`) to introspect playback status
//!   and pause / resume playing media streams via [`MprisController`].
//! - [`AutoPauseCoordinator`] to detect earbud removal, pause active media, and resume only
//!   the paused players when earbuds are re-inserted within a configurable time window (default 10 minutes).

use std::time::{Duration, Instant};
use tokio::process::Command;
use zbus::Connection;
use zbus::fdo::DBusProxy;

use crate::protocols::aap::InEarStatus;

/// Audio sink monitor interacting with PulseAudio or PipeWire.
pub struct AudioMonitor;

impl AudioMonitor {
    /// Queries `pactl info` for the currently active default audio sink name.
    ///
    /// Returns `Some(sink_name)` if pactl succeeds and reports a default sink, or `None`.
    pub async fn get_default_sink() -> Option<String> {
        let output = Command::new("pactl").arg("info").output().await.ok()?;

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

/// D-Bus MPRIS media player controller for smart playback pausing and resuming.
pub struct MprisController;

impl MprisController {
    /// Queries the session D-Bus for all active MPRIS media players (`org.mpris.MediaPlayer2.*`).
    pub async fn get_mpris_players(conn: &Connection) -> Vec<String> {
        if let Ok(dbus) = DBusProxy::new(conn).await
            && let Ok(names) = dbus.list_names().await
        {
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
        Vec::new()
    }

    /// Gets the playback status of a given player (`"Playing"`, `"Paused"`, or `"Stopped"`).
    pub async fn get_playback_status(conn: &Connection, player_bus_name: &str) -> Option<String> {
        let proxy = zbus::Proxy::new(
            conn,
            player_bus_name,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        )
        .await
        .ok()?;

        let status_res: Result<String, _> = proxy.get_property("PlaybackStatus").await;
        status_res.ok()
    }

    /// Pauses a specific player via its MPRIS interface.
    pub async fn pause_player(conn: &Connection, player_bus_name: &str) -> bool {
        if let Ok(proxy) = zbus::Proxy::new(
            conn,
            player_bus_name,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        )
        .await
        {
            let res: Result<(), _> = proxy.call("Pause", &()).await;
            res.is_ok()
        } else {
            false
        }
    }

    /// Resumes (Play) a specific player via its MPRIS interface.
    pub async fn play_player(conn: &Connection, player_bus_name: &str) -> bool {
        if let Ok(proxy) = zbus::Proxy::new(
            conn,
            player_bus_name,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        )
        .await
        {
            let res: Result<(), _> = proxy.call("Play", &()).await;
            res.is_ok()
        } else {
            false
        }
    }

    /// Pauses all currently playing media players and returns the list of bus names paused.
    pub async fn pause_all_playing(conn: &Connection) -> Vec<String> {
        let players = Self::get_mpris_players(conn).await;
        let mut paused = Vec::new();

        for player in players {
            if let Some(status) = Self::get_playback_status(conn, &player).await
                && status.eq_ignore_ascii_case("Playing")
                && Self::pause_player(conn, &player).await
            {
                paused.push(player);
            }
        }

        // Fallback to playerctl if no MPRIS player returned "Playing" directly
        if paused.is_empty() {
            let _ = Command::new("playerctl").arg("pause").output().await;
        }

        paused
    }

    /// Resumes the players that were previously paused by smart auto-pause.
    pub async fn resume_players(conn: &Connection, player_bus_names: &[String]) -> usize {
        if player_bus_names.is_empty() {
            // Fallback to playerctl play
            let res = Command::new("playerctl").arg("play").output().await;
            return if res.is_ok() { 1 } else { 0 };
        }

        let mut resumed = 0;
        for player in player_bus_names {
            if let Ok(proxy) = zbus::Proxy::new(
                conn,
                player.as_str(),
                "/org/mpris/MediaPlayer2",
                "org.mpris.MediaPlayer2.Player",
            )
            .await
            {
                let _: Result<(), _> = proxy.call("Play", &()).await;
                if let Some(status) = Self::get_playback_status(conn, player).await {
                    if !status.eq_ignore_ascii_case("Playing")
                        && Self::play_player(conn, player).await
                    {
                        resumed += 1;
                    }
                } else if Self::play_player(conn, player).await {
                    resumed += 1;
                }
            }
        }
        resumed
    }
}

/// Action decision returned by the state machine evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPauseAction {
    /// No action required.
    None,
    /// Pause active media playback.
    Pause,
    /// Resume media playback on previously paused players.
    Resume,
}

/// State coordinator for smart auto-pause and auto-resume transitions.
pub struct AutoPauseCoordinator {
    /// Whether auto-pause is enabled by user preference.
    pub enabled: bool,
    /// Previous count of earbuds worn in ear.
    pub prev_in_ear_count: Option<usize>,
    /// Bus names of players paused by this coordinator.
    pub paused_players: Vec<String>,
    /// Timestamp when playback was paused.
    pub paused_at: Option<Instant>,
    /// Maximum elapsed duration allowed for auto-resuming playback (10 minutes).
    pub max_resume_delay: Duration,
}

impl AutoPauseCoordinator {
    /// Creates a new `AutoPauseCoordinator`.
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            prev_in_ear_count: None,
            paused_players: Vec::new(),
            paused_at: None,
            max_resume_delay: Duration::from_secs(600), // 10 minutes
        }
    }

    /// Sets whether auto-pause is enabled.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.paused_players.clear();
            self.paused_at = None;
        }
    }

    /// Resets the internal state tracking.
    pub fn reset(&mut self) {
        self.prev_in_ear_count = None;
        self.paused_players.clear();
        self.paused_at = None;
    }

    /// Returns a slice of the bus names currently paused by the coordinator.
    pub fn get_paused_players(&self) -> &[String] {
        &self.paused_players
    }

    /// Pure transition evaluator without I/O, ideal for unit testing and deterministic logic.
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
                let can_resume = self
                    .paused_at
                    .is_some_and(|t| t.elapsed() <= self.max_resume_delay);
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

    /// Handles an in-ear status telemetry update from earbuds.
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
                let count_resumed =
                    MprisController::resume_players(conn, &self.paused_players).await;
                println!(
                    "Smart Auto-Pause: Earbud re-inserted. Resumed {} player(s)",
                    count_resumed
                );
            }
            AutoPauseAction::None => {}
        }
    }

    /// Handles wear sensor toggle/status update (for single wear sensors, over-ear headphones, B&W).
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
                let count_resumed =
                    MprisController::resume_players(conn, &self.paused_players).await;
                println!(
                    "Smart Auto-Pause: Headphone put back on. Resumed {} player(s)",
                    count_resumed
                );
            }
            AutoPauseAction::None => {}
        }
    }

    /// Handles Bluetooth disconnection while audio was active.
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
        if let Ok(conn) = Connection::session().await {
            let players = MprisController::get_mpris_players(&conn).await;
            println!("Discovered active MPRIS media players: {:?}", players);
        }
    }

    #[test]
    fn test_auto_pause_dual_earbuds_removal_and_resume() {
        let mut coord = AutoPauseCoordinator::new(true);

        // Baseline: both earbuds in ear
        let a1 = coord.evaluate_ear_transition(2, false);
        assert_eq!(a1, AutoPauseAction::None);

        // One earbud removed: transition 2 -> 1
        let a2 = coord.evaluate_ear_transition(1, false);
        assert_eq!(a2, AutoPauseAction::Pause);
        // Simulate that a media player was paused
        coord
            .paused_players
            .push("org.mpris.MediaPlayer2.spotify".to_string());
        coord.paused_at = Some(Instant::now());

        // Earbud returned: transition 1 -> 2
        let a3 = coord.evaluate_ear_transition(2, false);
        assert_eq!(a3, AutoPauseAction::Resume);
        assert!(coord.paused_players.is_empty());
        assert!(coord.paused_at.is_none());
    }

    #[test]
    fn test_auto_pause_single_earbud_removal_and_resume() {
        let mut coord = AutoPauseCoordinator::new(true);

        // Baseline: 1 bud worn
        let a1 = coord.evaluate_ear_transition(1, false);
        assert_eq!(a1, AutoPauseAction::None);

        // Removed the only worn bud: 1 -> 0
        let a2 = coord.evaluate_ear_transition(0, false);
        assert_eq!(a2, AutoPauseAction::Pause);
        coord
            .paused_players
            .push("org.mpris.MediaPlayer2.vlc".to_string());
        coord.paused_at = Some(Instant::now());

        // Put it back in: 0 -> 1
        let a3 = coord.evaluate_ear_transition(1, false);
        assert_eq!(a3, AutoPauseAction::Resume);
    }

    #[test]
    fn test_auto_pause_both_in_case_clears_resume() {
        let mut coord = AutoPauseCoordinator::new(true);

        // Baseline: 2 buds in ear
        coord.evaluate_ear_transition(2, false);

        // Earbud removed
        coord.evaluate_ear_transition(1, false);
        coord
            .paused_players
            .push("org.mpris.MediaPlayer2.spotify".to_string());
        coord.paused_at = Some(Instant::now());

        // Both buds placed in case (user docked them)
        let a = coord.evaluate_ear_transition(0, true);
        assert_eq!(a, AutoPauseAction::Pause);
        // Paused state should now be reset so resuming is aborted
        assert!(coord.paused_players.is_empty());
        assert!(coord.paused_at.is_none());

        // Taking them out of the case later should not auto-resume old session
        let a_next = coord.evaluate_ear_transition(2, false);
        assert_eq!(a_next, AutoPauseAction::None);
    }

    #[test]
    fn test_auto_pause_disabled_does_nothing() {
        let mut coord = AutoPauseCoordinator::new(false);

        coord.evaluate_ear_transition(2, false);
        let a = coord.evaluate_ear_transition(1, false);
        assert_eq!(a, AutoPauseAction::None);
    }

    #[test]
    fn test_auto_pause_timeout_expires() {
        let mut coord = AutoPauseCoordinator::new(true);
        coord.max_resume_delay = Duration::from_millis(50);

        coord.evaluate_ear_transition(2, false);
        coord.evaluate_ear_transition(1, false);
        coord
            .paused_players
            .push("org.mpris.MediaPlayer2.spotify".to_string());
        coord.paused_at = Some(Instant::now() - Duration::from_millis(100));

        // Re-insertion after timeout expired
        let a = coord.evaluate_ear_transition(2, false);
        assert_eq!(a, AutoPauseAction::None);
        assert!(coord.paused_players.is_empty());
    }

    #[test]
    fn test_in_ear_status_integration_helpers() {
        let status = InEarStatus {
            primary: EarStatus::InEar,
            secondary: EarStatus::OutOfEar,
        };
        assert_eq!(status.in_ear_count(), 1);
        assert!(!status.are_both_in_case());

        let in_case = InEarStatus {
            primary: EarStatus::InCase,
            secondary: EarStatus::InCase,
        };
        assert_eq!(in_case.in_ear_count(), 0);
        assert!(in_case.are_both_in_case());
    }
}
