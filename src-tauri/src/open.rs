//! Alles, was "beim Klick etwas oeffnen" heisst, laeuft hier durch.
//!
//!   reveal:<pfad>       -> Explorer mit markierter Datei
//!   https://… / x://…   -> Standardprogramm fuer die URL
//!   …\App.exe           -> laeuft sie schon: nach vorn holen, sonst starten
//!   anderer Pfad        -> mit Standardprogramm oeffnen

use std::io::Write;
use std::net::TcpStream;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::win;

pub fn open_target(app: &AppHandle, target: &str) -> Result<(), String> {
    let t = target.trim();
    if let Some(p) = t.strip_prefix("reveal:") {
        win::reveal(p);
        return Ok(());
    }
    if t.contains("://") || t.starts_with("mailto:") {
        return app.opener().open_url(t, None::<&str>).map_err(|e| e.to_string());
    }
    if t.to_lowercase().ends_with(".exe") {
        if win::focus_exe(t) {
            return Ok(());
        }
        let mut c = Command::new(t);
        if let Some(dir) = Path::new(t).parent() {
            c.current_dir(dir);
        }
        return c.spawn().map(|_| ()).map_err(|e| e.to_string());
    }
    // Ist die Datei schon offen (Fenstertitel = Dateiname, z. B. in Folio oder Word), das Fenster nach vorn holen.
    // Direkt hier, weil nur wir gerade die letzte Eingabe hatten und Windows uns den Fokuswechsel erlaubt.
    if let Some(name) = Path::new(t).file_name().map(|n| n.to_string_lossy().to_string()) {
        let hit = win::focus_title(&name);
        if cfg!(debug_assertions) {
            eprintln!("[notch] open {t}: Fenster mit Titel '{name}' nach vorn -> {hit}");
        }
        if hit {
            return Ok(());
        }
    }
    app.opener().open_path(t, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open(app: AppHandle, target: String) -> Result<(), String> {
    open_target(&app, &target)
}

/// JSON per HTTP-POST an eine lokale App schicken (Rueckmeldung eines Aktionsknopfs).
/// Nur localhost — die Notch telefoniert nicht nach draussen.
pub fn post_local(url: &str, body: String) {
    let url = url.to_string();
    std::thread::spawn(move || {
        let Some(rest) = url.strip_prefix("http://") else { return };
        let (hostport, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let host = hostport.split(':').next().unwrap_or("");
        if host != "127.0.0.1" && host != "localhost" {
            return;
        }
        let addr = hostport.replace("localhost", "127.0.0.1");
        let addr = if addr.contains(':') { addr } else { format!("{addr}:80") };
        let Ok(sock) = addr.parse() else { return };
        if let Ok(mut s) = TcpStream::connect_timeout(&sock, Duration::from_secs(1)) {
            let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
            let req = format!(
                "POST {path} HTTP/1.1\r\nHost: {hostport}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(req.as_bytes());
        }
    });
}
