//! Now Playing ueber die Windows-Media-Session (GSMTC).
//! Das ist dieselbe Quelle, aus der Windows sein Lautstaerke-Overlay fuettert:
//! Spotify, Browser-Tabs, VLC, Apple Music usw. laufen automatisch mit.

use std::sync::Mutex;
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
    pub source: String,
    pub playing: bool,
    /// Sekunden
    pub position: f64,
    /// Sekunden, 0 = unbekannt (z. B. Livestream)
    pub duration: f64,
    /// wechselt, wenn ein anderes Stueck laeuft -> Cover neu laden
    pub key: String,
}

static LAST: Mutex<Option<MediaState>> = Mutex::new(None);
static COVER: Mutex<Option<String>> = Mutex::new(None);

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

fn read(m: &Manager) -> windows::core::Result<Option<(MediaState, Props)>> {
    let sess: Session = match m.GetCurrentSession() {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let props = sess.TryGetMediaPropertiesAsync()?.join()?;
    let title = props.Title()?.to_string();
    if title.is_empty() {
        return Ok(None);
    }
    let artist = props.Artist()?.to_string();
    let album = props.AlbumTitle().map(|s| s.to_string()).unwrap_or_default();
    let source = sess.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default();
    let playing = sess.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;

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
        MediaState { title, artist, album, source, playing, position: pos, duration: dur, key },
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
            std::thread::sleep(Duration::from_millis(1000));
        }
    });
}

#[tauri::command]
pub async fn media_control(action: String) -> Result<(), String> {
    let run = || -> windows::core::Result<()> {
        let s = Manager::RequestAsync()?.join()?.GetCurrentSession()?;
        match action.as_str() {
            "toggle" => {
                s.TryTogglePlayPauseAsync()?.join()?;
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
    run().map_err(|e| e.to_string())
}
