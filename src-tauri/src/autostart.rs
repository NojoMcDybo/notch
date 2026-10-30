//! Mit Windows starten: Eintrag „Notch“ unter HKCU\…\CurrentVersion\Run (nur dieses Benutzerkonto, ohne
//! Adminrechte, wie „Beim Anmelden starten“ anderer Programme). Zeigt auf die gerade laufende notch.exe.

use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    KEY_SET_VALUE, REG_SZ,
};

const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const NAME: PCWSTR = w!("Notch");

fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut k = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN, Some(0), access, &mut k) }.is_ok().then_some(k)
}

#[tauri::command]
pub fn autostart_get() -> bool {
    let Some(k) = open(KEY_READ) else { return false };
    let ok = unsafe { RegQueryValueExW(k, NAME, None, None, None, None) }.is_ok();
    unsafe {
        let _ = RegCloseKey(k);
    }
    ok
}

#[tauri::command]
pub fn autostart_set(on: bool) -> Result<(), String> {
    let k = open(KEY_SET_VALUE).ok_or("Autostart-Eintrag nicht erreichbar")?;
    let r = if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // REG_SZ als UTF-16 mit abschliessender Null, Pfad in Anfuehrungszeichen (Leerzeichen)
        let value: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(Some(0)).collect();
        let bytes = unsafe { std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2) };
        unsafe { RegSetValueExW(k, NAME, Some(0), REG_SZ, Some(bytes)) }.ok().map_err(|e| e.to_string())
    } else {
        let r = unsafe { RegDeleteValueW(k, NAME) };
        // schon weg ist auch gut
        if r.is_ok() || r == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND { Ok(()) } else { Err(format!("Windows-Fehler {}", r.0)) }
    };
    unsafe {
        let _ = RegCloseKey(k);
    }
    r
}
