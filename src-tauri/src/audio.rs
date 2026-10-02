//! Lautstaerke ueber Core Audio (ohne das Windows-Lautstaerke-Popup auszuloesen).
//!
//! Gesteuert wird das Ausgabegeraet, auf dem der angezeigte Player gerade wirklich spielt —
//! nicht stur das Standardgeraet. Bei SteelSeries Sonar z. B. laeuft Spotify auf
//! "Sonar - Media", Spiele auf "Sonar - Gaming"; die Notch findet die Audio-Sitzung des
//! Players auf allen Ausgabegeraeten und regelt genau diesen Kanal. Spielt nichts,
//! gilt das Windows-Standardgeraet.

use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use windows::core::{Interface, GUID};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    eConsole, eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

#[derive(Serialize)]
pub struct Volume {
    level: f32,
    muted: bool,
    /// Name des Ausgabegeraets, z. B. "SteelSeries Sonar - Media (SteelSeries Sonar Virtual Audio Device)"
    device: String,
    /// true = Geraet des Players gefunden, false = Windows-Standardgeraet
    follows_player: bool,
}

pub(crate) fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
    }
}

pub(crate) fn device_name(dev: &IMMDevice) -> String {
    unsafe {
        dev.OpenPropertyStore(STGM_READ)
            .and_then(|ps| ps.GetValue(&PKEY_Device_FriendlyName))
            .map(|v| v.to_string())
            .unwrap_or_default()
    }
}

/// Exe-Name des Players aus der Medienquelle: "SpotifyAB…!Spotify" -> "spotify.exe", "chrome.exe" bleibt.
pub(crate) fn player_exe() -> Option<String> {
    let src = crate::media::last()?.source;
    let s = src.rsplit('!').next().unwrap_or(&src).to_lowercase();
    Some(if s.ends_with(".exe") { s } else { format!("{s}.exe") })
}

/// Ausgabegeraet, auf dem der Player eine Audio-Sitzung hat (aktive Sitzungen zuerst).
pub(crate) fn player_device(en: &IMMDeviceEnumerator, exe: &str) -> Option<IMMDevice> {
    unsafe {
        let devices = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).ok()?;
        let mut fallback = None;
        for i in 0..devices.GetCount().ok()? {
            let Ok(dev) = devices.Item(i) else { continue };
            let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(list) = mgr.GetSessionEnumerator() else { continue };
            for j in 0..list.GetCount().unwrap_or(0) {
                let Ok(ctl) = list.GetSession(j) else { continue };
                let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
                let Ok(pid) = ctl2.GetProcessId() else { continue };
                let name = crate::win::process_path(pid)
                    .map(|p| p.rsplit('\\').next().unwrap_or("").to_lowercase())
                    .unwrap_or_default();
                if name != exe {
                    continue;
                }
                if ctl.GetState().map(|s| s == AudioSessionStateActive).unwrap_or(false) {
                    return Some(dev);
                }
                if fallback.is_none() {
                    fallback = Some(dev.clone());
                }
            }
        }
        fallback
    }
}

/// Geraet waehlen: das des Players, sonst Standard.
fn target() -> windows::core::Result<(IAudioEndpointVolume, String, bool)> {
    let en = enumerator()?;
    let (dev, follows) = match player_exe().and_then(|exe| player_device(&en, &exe)) {
        Some(d) => (d, true),
        None => (unsafe { en.GetDefaultAudioEndpoint(eRender, eConsole)? }, false),
    };
    let vol = unsafe { dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)? };
    Ok((vol, device_name(&dev), follows))
}

fn read(e: &IAudioEndpointVolume, device: String, follows_player: bool) -> windows::core::Result<Volume> {
    unsafe {
        Ok(Volume {
            level: e.GetMasterVolumeLevelScalar()?,
            muted: e.GetMute()?.as_bool(),
            device,
            follows_player,
        })
    }
}

// ---------- Pegel fuer den Equalizer ----------

/// Peak-Messer aller Audio-Sitzungen des Players (Spotify hat oft mehrere).
fn player_meters(en: &IMMDeviceEnumerator, exe: &str) -> Vec<IAudioMeterInformation> {
    let mut out = Vec::new();
    unsafe {
        let Ok(devices) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
        for i in 0..devices.GetCount().unwrap_or(0) {
            let Ok(dev) = devices.Item(i) else { continue };
            let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(list) = mgr.GetSessionEnumerator() else { continue };
            for j in 0..list.GetCount().unwrap_or(0) {
                let Ok(ctl) = list.GetSession(j) else { continue };
                let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
                let Ok(pid) = ctl2.GetProcessId() else { continue };
                let name = crate::win::process_path(pid)
                    .map(|p| p.rsplit('\\').next().unwrap_or("").to_lowercase())
                    .unwrap_or_default();
                if name == exe {
                    if let Ok(m) = ctl.cast::<IAudioMeterInformation>() {
                        out.push(m);
                    }
                }
            }
        }
    }
    out
}

/// Misst ~30x pro Sekunde den Pegel des Players und schickt ihn als "level" ans Frontend —
/// nur solange Musik laeuft und die Notch sichtbar ist. Findet sich keine Sitzung des Players,
/// wird der Standardausgang gemessen.
pub fn spawn_meter(app: AppHandle) {
    std::thread::spawn(move || {
        let Ok(en) = enumerator() else { return };
        let mut meters: Vec<IAudioMeterInformation> = Vec::new();
        let mut tick = 0u32;
        let mut silent = 0u32;
        loop {
            let playing = crate::media::last().map(|m| m.playing).unwrap_or(false);
            if !playing || crate::FULLSCREEN.load(std::sync::atomic::Ordering::Relaxed) {
                meters.clear();
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            // Sitzungen alle ~2 s neu suchen (Player gewechselt, Geraet gewechselt)
            if meters.is_empty() || tick % 60 == 0 {
                meters = player_meters(&en, &player_exe().unwrap_or_default());
                if meters.is_empty() {
                    if let Ok(dev) = unsafe { en.GetDefaultAudioEndpoint(eRender, eConsole) } {
                        if let Ok(m) = unsafe { dev.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) } {
                            meters.push(m);
                        }
                    }
                }
            }
            tick = tick.wrapping_add(1);
            let peak = meters
                .iter()
                .filter_map(|m| unsafe { m.GetPeakValue().ok() })
                .fold(0.0f32, f32::max);
            // Stille nicht dauernd schicken
            if peak < 0.001 {
                silent += 1;
            } else {
                silent = 0;
            }
            if silent < 3 {
                let _ = app.emit("level", (peak * 1000.0).round() / 1000.0);
            }
            std::thread::sleep(Duration::from_millis(33));
        }
    });
}

#[tauri::command]
pub async fn volume_get() -> Result<Volume, String> {
    let run = || -> windows::core::Result<Volume> {
        let (e, d, f) = target()?;
        read(&e, d, f)
    };
    run().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn volume_set(level: f32) -> Result<Volume, String> {
    let run = || -> windows::core::Result<Volume> {
        let (e, d, f) = target()?;
        unsafe {
            e.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), &GUID::zeroed())?;
            if level > 0.0 {
                e.SetMute(false, &GUID::zeroed())?;
            }
        }
        read(&e, d, f)
    };
    run().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn volume_mute() -> Result<Volume, String> {
    let run = || -> windows::core::Result<Volume> {
        let (e, d, f) = target()?;
        unsafe {
            let m = e.GetMute()?.as_bool();
            e.SetMute(!m, &GUID::zeroed())?;
        }
        read(&e, d, f)
    };
    run().map_err(|e| e.to_string())
}
