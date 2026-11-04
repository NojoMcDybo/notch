//! Now Playing auf dem Mac: Spotify und Musik (Apple Music) per AppleScript.
//! macOS hat keine oeffentliche Schnittstelle wie Windows' Media-Session (MediaRemote ist seit macOS 15.4 fuer
//! fremde Apps gesperrt), deshalb fragt die Notch die beiden Player direkt — nur, wenn sie laufen, damit
//! AppleScript sie nicht startet. Beim ersten Mal fragt macOS einmal pro Player nach der Erlaubnis.
//! shortcut: Browser-Tabs (YouTube usw.) erscheinen auf dem Mac nicht; dafuer braeuchte es eine Browser-Erweiterung.

use std::process::Command;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
pub struct MediaState {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// "Spotify" oder "Music"
    pub source: String,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub can_seek: bool,
    pub can_next: bool,
    pub can_prev: bool,
    pub sessions: usize,
    pub key: String,
}

const PLAYERS: [&str; 2] = ["Spotify", "Music"];

static LAST: Mutex<Option<MediaState>> = Mutex::new(None);
static COVER: Mutex<Option<String>> = Mutex::new(None);
static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
static PREFERRED: Mutex<String> = Mutex::new(String::new());
static SHOWN: Mutex<String> = Mutex::new(String::new());

fn wake() {
    let (m, c) = &WAKE;
    *m.lock().unwrap() = true;
    c.notify_all();
}

pub fn last() -> Option<MediaState> {
    LAST.lock().unwrap().clone()
}
pub fn last_cover() -> Option<String> {
    COVER.lock().unwrap().clone()
}

fn osascript(script: &str) -> Option<String> {
    let o = Command::new("osascript").arg("-e").arg(script).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim_end_matches('\n').to_string())
}

fn running(player: &str) -> bool {
    Command::new("pgrep").args(["-xq", player]).status().is_ok_and(|s| s.success())
}

/// Eine Zeile: Titel, Kuenstler, Album, Status, Position, Dauer (Tab-getrennt)
fn query(player: &str) -> Option<MediaState> {
    let script = format!(
        "tell application \"{player}\"\n\
         set t to current track\n\
         return (name of t) & tab & (artist of t) & tab & (album of t) & tab & (player state as text) & tab & (player position as text) & tab & (duration of t as text)\n\
         end tell"
    );
    parse(player, &osascript(&script)?)
}

/// Zahlen kommen im Format der Systemsprache ("12,5"); Spotify meldet die Dauer in ms, Musik in s.
fn parse(player: &str, line: &str) -> Option<MediaState> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() < 6 || f[0].is_empty() {
        return None;
    }
    let num = |s: &str| s.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0);
    let mut dur = num(f[5]);
    if player == "Spotify" {
        dur /= 1000.0;
    }
    let pos = num(f[4]).clamp(0.0, dur.max(0.0));
    Some(MediaState {
        title: f[0].into(),
        artist: f[1].into(),
        album: f[2].into(),
        source: player.into(),
        playing: f[3] == "playing",
        position: (pos * 10.0).round() / 10.0,
        duration: dur,
        can_seek: dur > 0.5,
        can_next: true,
        can_prev: true,
        sessions: 0,
        key: format!("{player}|{}|{}", f[0], f[1]),
    })
}

fn data_url(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let ct = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
    Some(format!("data:{ct};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Cover als data:-URL (das Frontend liest daraus Farben, fremde Bild-URLs sperrt der Canvas)
fn cover(player: &str) -> Option<String> {
    if player == "Spotify" {
        let url = osascript("tell application \"Spotify\" to return artwork url of current track")?;
        if !url.starts_with("https://") {
            return None;
        }
        let mut res = ureq::get(&url).call().ok()?;
        let bytes = res.body_mut().with_config().limit(5 * 1024 * 1024).read_to_vec().ok()?;
        return data_url(&bytes);
    }
    // Musik: Bilddaten des Covers in eine temporaere Datei schreiben lassen
    let file = std::env::temp_dir().join("notch-cover.bin");
    let _ = std::fs::remove_file(&file);
    let script = format!(
        "tell application \"Music\"\n\
         set d to raw data of artwork 1 of current track\n\
         end tell\n\
         set f to open for access (POSIX file \"{}\") with write permission\n\
         set eof f to 0\n\
         write d to f\n\
         close access f",
        file.display()
    );
    osascript(&script)?;
    let bytes = std::fs::read(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    data_url(&bytes)
}

/// Welcher Player? 1. der gewaehlte (solange kein anderer spielt, waehrend er schweigt), 2. einer, der spielt, 3. irgendeiner.
fn pick(all: &[MediaState]) -> Option<MediaState> {
    let pref = PREFERRED.lock().unwrap().clone();
    if !pref.is_empty() {
        if let Some(s) = all.iter().find(|s| s.source == pref) {
            if s.playing || !all.iter().any(|o| o.source != pref && o.playing) {
                return Some(s.clone());
            }
        }
        PREFERRED.lock().unwrap().clear();
    }
    all.iter().find(|s| s.playing).or(all.first()).cloned()
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut cover_key = String::new();
        loop {
            let all: Vec<MediaState> = PLAYERS.iter().filter(|p| running(p)).filter_map(|p| query(p)).collect();
            let state = pick(&all).map(|mut s| {
                s.sessions = all.len();
                s
            });
            *SHOWN.lock().unwrap() = state.as_ref().map(|s| s.source.clone()).unwrap_or_default();

            match &state {
                Some(s) if s.key != cover_key => {
                    if let Some(c) = cover(&s.source) {
                        cover_key = s.key.clone();
                        *COVER.lock().unwrap() = Some(c.clone());
                        let _ = app.emit("media-cover", serde_json::json!({ "key": s.key, "src": c }));
                    }
                }
                None if !cover_key.is_empty() => {
                    cover_key.clear();
                    *COVER.lock().unwrap() = None;
                }
                _ => {}
            }

            let changed = {
                let mut last = LAST.lock().unwrap();
                let changed = *last != state;
                *last = state.clone();
                changed
            };
            if changed {
                let _ = app.emit("media", state);
            }

            let (m, c) = &WAKE;
            let guard = m.lock().unwrap();
            let (mut g, _) = c.wait_timeout_while(guard, Duration::from_millis(1000), |woken| !*woken).unwrap();
            *g = false;
        }
    });
}

fn shown() -> Result<String, String> {
    let s = SHOWN.lock().unwrap().clone();
    if s.is_empty() { Err("Kein Player aktiv".into()) } else { Ok(s) }
}

fn wake_soon() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(180));
        wake();
    });
}

#[tauri::command]
pub async fn media_next_source() -> Result<(), String> {
    let srcs: Vec<&str> = PLAYERS.iter().copied().filter(|p| running(p)).collect();
    if srcs.is_empty() {
        return Ok(());
    }
    let shown = SHOWN.lock().unwrap().clone();
    let i = srcs.iter().position(|s| *s == shown).map(|i| (i + 1) % srcs.len()).unwrap_or(0);
    *PREFERRED.lock().unwrap() = srcs[i].into();
    wake();
    Ok(())
}

#[tauri::command]
pub async fn media_control(action: String) -> Result<(), String> {
    let cmd = match action.as_str() {
        "toggle" => "playpause",
        "play" => "play",
        "pause" => "pause",
        "next" => "next track",
        "prev" => "previous track",
        _ => return Ok(()),
    };
    let player = shown()?;
    let r = osascript(&format!("tell application \"{player}\" to {cmd}")).map(|_| ()).ok_or_else(|| "Player antwortet nicht".to_string());
    wake_soon();
    r
}

#[tauri::command]
pub async fn media_seek(seconds: f64) -> Result<bool, String> {
    let player = shown()?;
    // Punkt als Dezimaltrenner versteht AppleScript unabhaengig von der Systemsprache
    let ok = osascript(&format!("tell application \"{player}\" to set player position to {:.1}", seconds.max(0.0))).is_some();
    wake_soon();
    Ok(ok)
}

#[tauri::command]
pub fn media_focus() {
    if let Some(m) = last() {
        let _ = Command::new("open").args(["-a", &m.source]).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn zeile_mit_komma_und_ms() {
        let s = parse("Spotify", "Song\tBand\tAlbum\tplaying\t12,5\t200000").unwrap();
        assert_eq!((s.position, s.duration, s.playing), (12.5, 200.0, true));
        assert_eq!(s.key, "Spotify|Song|Band");
        let m = parse("Music", "Song\tBand\tAlbum\tpaused\t300.0\t180.5").unwrap();
        assert_eq!((m.position, m.duration, m.playing), (180.5, 180.5, false), "Position nie hinter dem Ende");
        assert!(parse("Music", "\t\t\t\t\t").is_none());
    }
}
