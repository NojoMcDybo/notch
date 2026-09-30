//! Now Playing ueber die Windows-Media-Session (GSMTC).
//! Das ist dieselbe Quelle, aus der Windows sein Lautstaerke-Overlay fuettert:
//! Spotify, Browser-Tabs, VLC, Apple Music usw. laufen automatisch mit.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Props,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
pub struct MediaState {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// AppUserModelId der Quelle, z. B. "Spotify.exe" oder "SpotifyAB.SpotifyMusic_...!Spotify"
    pub source: String,
    pub playing: bool,
    /// Sekunden
    pub position: f64,
    /// Sekunden, 0 = unbekannt (z. B. Livestream)
    pub duration: f64,
    pub can_seek: bool,
    pub can_next: bool,
    pub can_prev: bool,
    /// wie viele Player gerade eine Sitzung haben (>1 -> "Quelle wechseln" anbieten)
    pub sessions: usize,
    /// wechselt, wenn ein anderes Stueck laeuft -> Cover neu laden
    pub key: String,
}

static LAST: Mutex<Option<MediaState>> = Mutex::new(None);
static COVER: Mutex<Option<String>> = Mutex::new(None);
/// Weckt die Abfrage-Schleife sofort (nach Play/Skip/Spulen), statt bis zu 1 s zu warten.
static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

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

/// Jetzt in 100-ns-Ticks seit 1601 (Windows-DateTime).
fn ticks_now() -> i64 {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    (d.as_secs() as i64 + 11_644_473_600) * 10_000_000 + d.subsec_nanos() as i64 / 100
}

/// Vom Nutzer gewaehlte Quelle (per Klick auf "Quelle wechseln"); leer = automatisch.
static PREFERRED: Mutex<String> = Mutex::new(String::new());
/// Quelle, die gerade angezeigt wird — Steuerbefehle gehen genau an die.
static SHOWN: Mutex<String> = Mutex::new(String::new());

/// "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify" -> "Spotify", "chrome.exe" -> "Chrome"
pub fn app_name(source: &str) -> String {
    let s = source.rsplit('!').next().unwrap_or(source);
    let s = s.strip_suffix(".exe").or_else(|| s.strip_suffix(".EXE")).unwrap_or(s);
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn is_playing(s: &Session) -> bool {
    s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).map(|st| st == Status::Playing).unwrap_or(false)
}

fn all_sessions(m: &Manager) -> Vec<Session> {
    m.GetSessions().map(|v| v.into_iter().collect()).unwrap_or_default()
}

/// Welche Sitzung zeigen wir? 1. die gewaehlte, 2. eine die spielt, 3. die, die Windows fuer aktuell haelt.
fn pick(m: &Manager) -> Option<Session> {
    let all = all_sessions(m);
    let src = |s: &Session| s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
    let pref = PREFERRED.lock().unwrap().clone();
    if !pref.is_empty() {
        if let Some(s) = all.iter().find(|s| src(s) == pref) {
            // Wahl gilt, bis eine ANDERE Quelle zu spielen anfaengt, waehrend die gewaehlte schweigt
            if is_playing(s) || !all.iter().any(|o| src(o) != pref && is_playing(o)) {
                return Some(s.clone());
            }
        }
        PREFERRED.lock().unwrap().clear();
    }
    let cur = m.GetCurrentSession().ok();
    if let Some(c) = &cur {
        if is_playing(c) {
            return cur;
        }
    }
    all.iter().find(|s| is_playing(s)).cloned().or(cur)
}

fn read(m: &Manager) -> windows::core::Result<Option<(MediaState, Props)>> {
    let sess: Session = match pick(m) {
        Some(s) => s,
        None => return Ok(None),
    };
    let sessions = all_sessions(m).len();
    let props = sess.TryGetMediaPropertiesAsync()?.join()?;
    let source = sess.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default();
    let mut title = props.Title()?.to_string();
    if title.is_empty() {
        // Player ohne Titel (z. B. Spotify direkt nach dem Start): trotzdem zeigen, damit Play klickbar ist
        title = app_name(&source);
        if title.is_empty() {
            return Ok(None);
        }
    }
    let artist = props.Artist()?.to_string();
    let album = props.AlbumTitle().map(|s| s.to_string()).unwrap_or_default();
    let info = sess.GetPlaybackInfo()?;
    let playing = info.PlaybackStatus()? == Status::Playing;
    let (can_seek, can_next, can_prev) = match info.Controls() {
        Ok(c) => (
            c.IsPlaybackPositionEnabled().unwrap_or(false),
            c.IsNextEnabled().unwrap_or(true),
            c.IsPreviousEnabled().unwrap_or(true),
        ),
        Err(_) => (false, true, true),
    };

    let tl = sess.GetTimelineProperties()?;
    let start = tl.StartTime()?.Duration;
    let dur = (tl.EndTime()?.Duration - start) as f64 / 1e7;
    let mut pos = (tl.Position()?.Duration - start) as f64 / 1e7;
    // Viele Player melden die Position nur selten -> ab letztem Update hochrechnen.
    if playing {
        let upd = tl.LastUpdatedTime()?.UniversalTime;
        if upd > 0 {
            pos += (ticks_now() - upd) as f64 / 1e7;
        }
    }
    let (pos, dur) = if dur > 0.5 { (pos.clamp(0.0, dur), dur) } else { (0.0, 0.0) };
    let key = format!("{source}|{title}|{artist}");
    Ok(Some((
        MediaState {
            title,
            artist,
            album,
            source,
            playing,
            position: (pos * 10.0).round() / 10.0,
            duration: dur,
            can_seek,
            can_next,
            can_prev,
            sessions,
            key,
        },
        props,
    )))
}

fn cover(props: &Props) -> windows::core::Result<Option<String>> {
    let thumb = match props.Thumbnail() {
        Ok(t) => t,
        Err(_) => return Ok(None),
    };
    let stream = thumb.OpenReadAsync()?.join()?;
    let size = stream.Size()? as u32;
    if size == 0 {
        return Ok(None);
    }
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
    reader.LoadAsync(size)?.join()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf)?;
    let ct = stream.ContentType().map(|h| h.to_string()).unwrap_or_default();
    let ct = if ct.starts_with("image/") { ct } else { "image/jpeg".into() };
    Ok(Some(format!(
        "data:{ct};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&buf)
    )))
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut mgr: Option<Manager> = None;
        let mut cover_key = String::new();
        loop {
            if mgr.is_none() {
                mgr = Manager::RequestAsync().and_then(|op| op.join()).ok();
            }
            let now = mgr.as_ref().and_then(|m| read(m).ok().flatten());
            let state = now.as_ref().map(|(s, _)| s.clone());
            *SHOWN.lock().unwrap() = state.as_ref().map(|s| s.source.clone()).unwrap_or_default();

            // Cover nur bei Stueckwechsel holen (ist gross); schlaegt es fehl, naechste Runde nochmal.
            if let Some((s, props)) = &now {
                if s.key != cover_key {
                    if let Ok(Some(c)) = cover(props) {
                        cover_key = s.key.clone();
                        *COVER.lock().unwrap() = Some(c.clone());
                        let _ = app.emit("media-cover", serde_json::json!({ "key": s.key, "src": c }));
                    }
                }
            } else if !cover_key.is_empty() {
                cover_key.clear();
                *COVER.lock().unwrap() = None;
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
            let (mut g, _) = c
                .wait_timeout_while(guard, Duration::from_millis(1000), |woken| !*woken)
                .unwrap();
            *g = false;
        }
    });
}

/// Die Sitzung, die gerade in der Notch angezeigt wird.
fn session() -> windows::core::Result<Session> {
    let m = Manager::RequestAsync()?.join()?;
    let shown = SHOWN.lock().unwrap().clone();
    if !shown.is_empty() {
        for s in all_sessions(&m) {
            if s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default() == shown {
                return Ok(s);
            }
        }
    }
    m.GetCurrentSession()
}

/// Zur naechsten Medienquelle springen (Spotify -> Browser -> …).
#[tauri::command]
pub async fn media_next_source() -> Result<(), String> {
    let m = Manager::RequestAsync().and_then(|o| o.join()).map_err(|e| e.to_string())?;
    let srcs: Vec<String> = all_sessions(&m)
        .iter()
        .map(|s| s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default())
        .collect();
    if srcs.is_empty() {
        return Ok(());
    }
    let shown = SHOWN.lock().unwrap().clone();
    let i = srcs.iter().position(|s| *s == shown).map(|i| (i + 1) % srcs.len()).unwrap_or(0);
    *PREFERRED.lock().unwrap() = srcs[i].clone();
    wake();
    Ok(())
}

/// Nach einer Aktion kurz warten (der Player braucht einen Moment) und dann neu abfragen.
fn wake_soon() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(180));
        wake();
    });
}

#[tauri::command]
pub async fn media_control(action: String) -> Result<(), String> {
    let run = || -> windows::core::Result<()> {
        let s = session()?;
        match action.as_str() {
            "toggle" => {
                s.TryTogglePlayPauseAsync()?.join()?;
            }
            "play" => {
                s.TryPlayAsync()?.join()?;
            }
            "pause" => {
                s.TryPauseAsync()?.join()?;
            }
            "next" => {
                s.TrySkipNextAsync()?.join()?;
            }
            "prev" => {
                s.TrySkipPreviousAsync()?.join()?;
            }
            _ => {}
        }
        Ok(())
    };
    let r = run().map_err(|e| e.to_string());
    wake_soon();
    r
}

/// Spulen: Sekunden ab Anfang des Stuecks. Gibt false zurueck, wenn der Player es ablehnt.
#[tauri::command]
pub async fn media_seek(seconds: f64) -> Result<bool, String> {
    let run = || -> windows::core::Result<bool> {
        let s = session()?;
        let start = s.GetTimelineProperties()?.StartTime()?.Duration;
        let ticks = start + (seconds.max(0.0) * 1e7) as i64;
        s.TryChangePlaybackPositionAsync(ticks)?.join()
    };
    let r = run().map_err(|e| e.to_string());
    wake_soon();
    r
}

/// Die spielende App nach vorn holen (Klick auf Cover/Titel).
#[tauri::command]
pub fn media_focus() {
    if let Some(m) = last() {
        crate::win::activate_app(&m.source);
    }
}
