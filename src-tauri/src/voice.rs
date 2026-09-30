//! Sprachassistent ueber die OpenAI Realtime API.
//!
//! Der echte API-Schluessel verlaesst Rust nie: hier wird nur ein kurzlebiger Schluessel
//! (client secret) geholt, mit dem das Frontend per WebRTC direkt mit OpenAI spricht.
//! Schluessel liegt in der Umgebungsvariable OPENAI_API_KEY oder in
//! %APPDATA%\de.nojo.notch\openai-key.txt (eine Zeile, beginnt mit "sk-").

use std::path::PathBuf;
use std::process::Command;

use tauri::{AppHandle, Manager};

pub const MODEL: &str = "gpt-realtime-2.1";
const SECRETS_URL: &str = "https://api.openai.com/v1/realtime/client_secrets";

fn key_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("openai-key.txt"))
}

fn api_key(app: &AppHandle) -> Option<String> {
    if let Ok(k) = std::env::var("OPENAI_API_KEY") {
        if k.trim().starts_with("sk-") {
            return Some(k.trim().into());
        }
    }
    let text = std::fs::read_to_string(key_file(app)?).ok()?;
    text.lines().map(str::trim).find(|l| l.starts_with("sk-")).map(String::from)
}

/// Ist ein Schluessel hinterlegt?
#[tauri::command]
pub fn voice_ready(app: AppHandle) -> bool {
    api_key(&app).is_some()
}

/// Schluesseldatei anlegen (falls noetig) und im Editor oeffnen — eintragen macht Nojo selbst.
#[tauri::command]
pub fn voice_setup(app: AppHandle) -> Result<(), String> {
    let f = key_file(&app).ok_or("kein App-Ordner")?;
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if !f.exists() {
        std::fs::write(
            &f,
            "# OpenAI-API-Schluessel in eine eigene Zeile darunter einfuegen (beginnt mit sk-), speichern, fertig.\n# Die Datei bleibt nur auf diesem PC. Nicht in Git einchecken.\n",
        )
        .map_err(|e| e.to_string())?;
    }
    Command::new("notepad.exe").arg(&f).spawn().map(|_| ()).map_err(|e| e.to_string())
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
