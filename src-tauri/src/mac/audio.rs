//! Lautstaerke auf dem Mac: Systemlautstaerke des aktuellen Ausgabegeraets per AppleScript
//! (`output volume`, ohne Erlaubnisabfrage). Einen Pegelmesser fuer den Equalizer gibt es auf dem Mac nicht —
//! Systemton mitzuhoeren braucht dort die Bildschirmaufnahme-Freigabe.

use serde::Serialize;
use std::process::Command;
use tauri::AppHandle;

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

pub fn spawn_meter(_app: AppHandle) {}

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
