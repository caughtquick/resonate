use tokio::process::Command;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_default_sink() {
        if let Some(sink) = AudioMonitor::get_default_sink().await {
            println!("Detected default audio sink: {}", sink);
            assert!(!sink.is_empty());
        }
    }
}
