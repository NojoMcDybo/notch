//! Sprachassistent ueber die OpenAI Realtime API.
//!
//! Der echte API-Schluessel verlaesst Rust nie: hier wird nur ein kurzlebiger Schluessel
//! (client secret) geholt, mit dem das Frontend per WebRTC direkt mit OpenAI spricht.
//! Schluessel: Umgebungsvariable OPENAI_API_KEY, sonst in den Einstellungen eingetragen und
//! mit Windows (DPAPI, nur dieses Benutzerkonto) verschluesselt in
//! %APPDATA%\de.nojo.notch\openai-key.bin. Die alte Klartextdatei openai-key.txt wird
//! beim ersten Lesen uebernommen und geloescht.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use windows::core::w;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

pub const MODEL: &str = "gpt-realtime-2.1";
const SECRETS_URL: &str = "https://api.openai.com/v1/realtime/client_secrets";
const MODELS_URL: &str = "https://api.openai.com/v1/models";

fn data_file(app: &AppHandle, name: &str) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(name))
}

fn dpapi(data: &[u8], protect: bool) -> Result<Vec<u8>, String> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        let res = if protect {
            CryptProtectData(&input, w!("Notch OpenAI"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        res.map_err(|e| e.to_string())?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        Ok(v)
    }
}

fn store(app: &AppHandle, key: &str) -> Result<(), String> {
    let f = data_file(app, "openai-key.bin").ok_or("kein App-Ordner")?;
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&f, dpapi(key.as_bytes(), true)?).map_err(|e| e.to_string())
}

fn stored_key(app: &AppHandle) -> Option<String> {
    // alte Klartextdatei einmalig uebernehmen
    if let Some(txt) = data_file(app, "openai-key.txt") {
        if let Ok(text) = std::fs::read_to_string(&txt) {
            if let Some(k) = text.lines().map(str::trim).find(|l| l.starts_with("sk-")) {
                if store(app, k).is_ok() {
                    let _ = std::fs::remove_file(&txt);
                }
                return Some(k.into());
            }
        }
    }
    let bytes = std::fs::read(data_file(app, "openai-key.bin")?).ok()?;
    String::from_utf8(dpapi(&bytes, false).ok()?).ok()
}

fn env_key() -> Option<String> {
    std::env::var("OPENAI_API_KEY").ok().map(|k| k.trim().to_string()).filter(|k| k.starts_with("sk-"))
}

fn api_key(app: &AppHandle) -> Option<String> {
    env_key().or_else(|| stored_key(app))
}

/// Ist ein Schluessel hinterlegt?
#[tauri::command]
pub fn voice_ready(app: AppHandle) -> bool {
    api_key(&app).is_some()
}

/// "Schluessel eintragen" in der Notch: Einstellungen auf dem Reiter App oeffnen.
#[tauri::command]
pub async fn voice_setup(app: AppHandle) -> Result<(), String> {
    // wie open_settings: Klick kam aus der Notch, Fokus nicht ans vorige Programm zurueckgeben
    crate::FOCUS_HOLD.store(crate::activities::now_ms() + 1500, std::sync::atomic::Ordering::Relaxed);
    crate::show_settings(&app, Some("app")).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct KeyStatus {
    /// "env" | "app" | "none"
    source: &'static str,
    /// z. B. "sk-…a1b2" — nie der ganze Schluessel
    hint: Option<String>,
}

fn hint(k: &str) -> String {
    let tail: String = k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("sk-…{tail}")
}

#[tauri::command]
pub fn voice_key_status(app: AppHandle) -> KeyStatus {
    if let Some(k) = env_key() {
        return KeyStatus { source: "env", hint: Some(hint(&k)) };
    }
    match stored_key(&app) {
        Some(k) => KeyStatus { source: "app", hint: Some(hint(&k)) },
        None => KeyStatus { source: "none", hint: None },
    }
}

/// Schluessel bei OpenAI pruefen und erst dann verschluesselt speichern.
#[tauri::command]
pub async fn voice_key_save(app: AppHandle, key: String) -> Result<KeyStatus, String> {
    let key = key.trim().to_string();
    if !key.starts_with("sk-") || key.len() < 20 || key.chars().any(char::is_whitespace) {
        return Err("Das sieht nicht nach einem OpenAI-Schlüssel aus (beginnt mit sk-).".into());
    }
    let k = key.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let resp = ureq::get(MODELS_URL)
            .config()
            .http_status_as_error(false)
            .build()
            .header("Authorization", &format!("Bearer {k}"))
            .call()
            .map_err(|e| format!("OpenAI nicht erreichbar: {e}"))?;
        match resp.status().as_u16() {
            200 => Ok(()),
            401 => Err("OpenAI lehnt den Schlüssel ab (ungültig oder widerrufen).".to_string()),
            s => Err(format!("OpenAI antwortet mit {s} – Schlüssel nicht gespeichert.")),
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    store(&app, &key)?;
    Ok(voice_key_status(app))
}

#[tauri::command]
pub fn voice_key_remove(app: AppHandle) -> KeyStatus {
    for name in ["openai-key.bin", "openai-key.txt"] {
        if let Some(f) = data_file(&app, name) {
            let _ = std::fs::remove_file(f);
        }
    }
    voice_key_status(app)
}

/// Kurzlebigen Schluessel fuer eine Sprachsitzung holen.
#[tauri::command]
pub async fn voice_token(app: AppHandle) -> Result<String, String> {
    let key = api_key(&app).ok_or("Kein OpenAI-Schlüssel hinterlegt")?;
    tauri::async_runtime::spawn_blocking(move || {
        let body = serde_json::json!({
            "session": {
                "type": "realtime",
                "model": MODEL,
                "audio": { "output": { "voice": "marin" } }
            }
        });
        let mut resp = ureq::post(SECRETS_URL)
            .config()
            .http_status_as_error(false)
            .build()
            .header("Authorization", &format!("Bearer {key}"))
            .send_json(&body)
            .map_err(|e| format!("OpenAI nicht erreichbar: {e}"))?;
        let status = resp.status();
        let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        if !status.is_success() {
            // Fehlermeldung von OpenAI durchreichen (z. B. falscher Schluessel, kein Guthaben)
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(String::from))
                .unwrap_or_else(|| text.chars().take(160).collect());
            return Err(format!("OpenAI {}: {msg}", status.as_u16()));
        }
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        v.get("value")
            .or_else(|| v.pointer("/client_secret/value"))
            .and_then(|x| x.as_str())
            .map(String::from)
            .ok_or_else(|| "Antwort ohne Schlüssel".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Mikrofon fuer die Notch ohne Rueckfrage erlauben. WebView2 wuerde sonst ein eigenes
/// Erlaubnis-Popup zeigen — in einem randlosen Overlay-Fenster waere das kaum bedienbar.
/// Andere Rechte (Kamera, Standort …) bleiben unangetastet.
pub fn allow_microphone(controller: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    };
    use webview2_com::PermissionRequestedEventHandler;
    unsafe {
        let Ok(core) = controller.CoreWebView2() else { return };
        let handler = PermissionRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                if kind == COREWEBVIEW2_PERMISSION_KIND_MICROPHONE {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
                }
            }
            Ok(())
        }));
        let mut token = Default::default();
        let _ = core.add_PermissionRequested(&handler, &mut token);
    }
}

#[cfg(test)]
mod tests {
    use super::{dpapi, hint};

    #[test]
    fn dpapi_hin_und_zurueck() {
        let enc = dpapi(b"sk-test-1234567890abcd", true).unwrap();
        assert!(!enc.windows(4).any(|w| w == b"sk-t"), "darf nicht im Klartext stehen");
        assert_eq!(dpapi(&enc, false).unwrap(), b"sk-test-1234567890abcd");
    }

    #[test]
    fn hinweis_zeigt_nur_die_letzten_vier() {
        assert_eq!(hint("sk-proj-geheim-a1b2"), "sk-…a1b2");
    }
}
