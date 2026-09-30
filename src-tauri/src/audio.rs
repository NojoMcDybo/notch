//! Systemlautstaerke ueber Core Audio (ohne das Windows-Lautstaerke-Popup auszuloesen).

use serde::Serialize;
use windows::core::GUID;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

fn endpoint() -> windows::core::Result<IAudioEndpointVolume> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
        dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
    }
}

#[derive(Serialize)]
pub struct Volume {
    level: f32,
    muted: bool,
}

fn get() -> windows::core::Result<Volume> {
    let e = endpoint()?;
    unsafe {
        Ok(Volume { level: e.GetMasterVolumeLevelScalar()?, muted: e.GetMute()?.as_bool() })
    }
}

#[tauri::command]
pub async fn volume_get() -> Result<Volume, String> {
    get().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn volume_set(level: f32) -> Result<Volume, String> {
    let run = || -> windows::core::Result<Volume> {
        let e = endpoint()?;
        unsafe {
            e.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), &GUID::zeroed())?;
            if level > 0.0 {
                e.SetMute(false, &GUID::zeroed())?;
            }
        }
        get()
    };
    run().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn volume_mute() -> Result<Volume, String> {
    let run = || -> windows::core::Result<Volume> {
        let e = endpoint()?;
        unsafe {
            let m = e.GetMute()?.as_bool();
            e.SetMute(!m, &GUID::zeroed())?;
        }
        get()
    };
    run().map_err(|e| e.to_string())
}
