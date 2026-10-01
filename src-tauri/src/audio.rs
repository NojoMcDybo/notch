//! Lautstaerke ueber Core Audio (ohne das Windows-Lautstaerke-Popup auszuloesen).
//!
//! Gesteuert wird das Ausgabegeraet, auf dem der angezeigte Player gerade wirklich spielt —
//! nicht stur das Standardgeraet. Bei SteelSeries Sonar z. B. laeuft Spotify auf
//! "Sonar - Media", Spiele auf "Sonar - Gaming"; die Notch findet die Audio-Sitzung des
//! Players auf allen Ausgabegeraeten und regelt genau diesen Kanal. Spielt nichts,
//! gilt das Windows-Standardgeraet.

use serde::Serialize;
use windows::core::{Interface, GUID};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
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

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
    }
}

fn device_name(dev: &IMMDevice) -> String {
    unsafe {
        dev.OpenPropertyStore(STGM_READ)
            .and_then(|ps| ps.GetValue(&PKEY_Device_FriendlyName))
            .map(|v| v.to_string())
            .unwrap_or_default()
    }
}

/// Exe-Name des Players aus der Medienquelle: "SpotifyAB…!Spotify" -> "spotify.exe", "chrome.exe" bleibt.
fn player_exe() -> Option<String> {
    let src = crate::media::last()?.source;
    let s = src.rsplit('!').next().unwrap_or(&src).to_lowercase();
    Some(if s.ends_with(".exe") { s } else { format!("{s}.exe") })
}

/// Ausgabegeraet, auf dem der Player eine Audio-Sitzung hat (aktive Sitzungen zuerst).
fn player_device(en: &IMMDeviceEnumerator, exe: &str) -> Option<IMMDevice> {
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
