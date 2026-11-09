//! Lautstaerke auf dem Mac: Systemlautstaerke des aktuellen Ausgabegeraets per AppleScript
//! (`output volume`, ohne Erlaubnisabfrage). Den Pegel fuer den Equalizer liest ein Core-Audio-Tap (mac/loopback.rs).

use serde::Serialize;
use std::process::Command;
use tauri::{AppHandle, Emitter};

#[derive(Serialize)]
pub struct Volume {
    level: f32,
    muted: bool,
    device: String,
    /// auf dem Mac immer das Systemgeraet
    follows_player: bool,
}

fn osascript(script: &str) -> Result<String, String> {
    let o = Command::new("osascript").arg("-e").arg(script).output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// "output volume:50, input volume:75, alert volume:100, output muted:false"
fn parse(s: &str) -> Option<(f32, bool)> {
    let field = |k: &str| s.split(", ").find_map(|p| p.strip_prefix(k)).map(str::trim);
    // ohne regelbares Ausgabegeraet (z. B. HDMI) meldet macOS "missing value"
    let level = field("output volume:")?.parse::<f32>().ok()? / 100.0;
    Some((level, field("output muted:") == Some("true")))
}

fn read() -> Result<Volume, String> {
    let (level, muted) = parse(&osascript("get volume settings")?).ok_or("Dieses Ausgabegerät lässt sich nicht regeln")?;
    Ok(Volume { level, muted, device: "Mac".into(), follows_player: false })
}

/// Pegel der Musik ("level", ~30x pro Sekunde) aus dem Systemton — wie unter Windows nur, solange etwas spielt
/// und kein Vollbild vorne ist. Ohne macOS 14.6 bleibt die Notch bei ihrer ruhigen Animation.
pub fn spawn_meter(app: AppHandle) {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};
    if !crate::mac::loopback::supported() {
        return;
    }
    std::thread::spawn(move || {
        let mut cap: Option<crate::mac::loopback::Loopback> = None;
        let mut tried: Option<Instant> = None;
        let mut buf = Vec::new();
        let mut silent = 0u32;
        let mut checked = Instant::now();
        loop {
            let playing = crate::media::last().is_some_and(|m| m.playing);
            if !playing || crate::FULLSCREEN.load(Ordering::Relaxed) {
                cap = None;
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            // Ausgang gewechselt (Kopfhoerer, AirPlay): neu oeffnen; Fehlschlaege hoechstens alle 5 s versuchen
            if checked.elapsed() > Duration::from_secs(3) {
                checked = Instant::now();
                let name = crate::mac::loopback::output_name();
                if cap.as_ref().is_some_and(|c| Some(&c.device) != name.as_ref()) {
                    cap = None;
                }
            }
            if cap.is_none() && tried.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
                tried = Some(Instant::now());
                cap = crate::mac::loopback::Loopback::open().map_err(|e| eprintln!("[notch] Pegel: {e}")).ok();
            }
            buf.clear();
            if let Some(c) = &cap {
                let _ = c.read(&mut buf);
            }
            let peak = buf.iter().fold(0.0f32, |m, v| m.max(v.abs())).min(1.0);
            // Stille nicht dauernd schicken
            silent = if peak < 0.001 { silent + 1 } else { 0 };
            if silent < 3 {
                let _ = app.emit("level", (peak * 1000.0).round() / 1000.0);
            }
            std::thread::sleep(Duration::from_millis(33));
        }
    });
}

#[tauri::command]
pub async fn volume_get() -> Result<Volume, String> {
    read()
}

#[tauri::command]
pub async fn volume_set(level: f32) -> Result<Volume, String> {
    let pct = (level.clamp(0.0, 1.0) * 100.0).round() as u32;
    let unmute = if pct > 0 { " without output muted" } else { "" };
    osascript(&format!("set volume output volume {pct}{unmute}"))?;
    read()
}

#[tauri::command]
pub async fn volume_mute() -> Result<Volume, String> {
    osascript("set volume output muted (not (output muted of (get volume settings)))")?;
    read()
}

#[cfg(test)]
mod tests {
    #[test]
    fn lautstaerke_lesen() {
        assert_eq!(super::parse("output volume:50, input volume:75, alert volume:100, output muted:false"), Some((0.5, false)));
        assert_eq!(super::parse("output volume:0, input volume:75, alert volume:100, output muted:true"), Some((0.0, true)));
        assert_eq!(super::parse("output volume:missing value, input volume:75, alert volume:100, output muted:missing value"), None);
    }
}
