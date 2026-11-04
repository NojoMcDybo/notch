//! Beim Anmelden starten (macOS): LaunchAgent ~/Library/LaunchAgents/de.nojo.notch.plist, nur dieses
//! Benutzerkonto, ohne Adminrechte. Startet die App ueber `open`, damit sie wie per Doppelklick laeuft.

use std::path::PathBuf;

const LABEL: &str = "de.nojo.notch";

fn plist() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

/// …/Notch.app aus …/Notch.app/Contents/MacOS/notch
fn app_bundle() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(PathBuf::from)
        .ok_or("Notch läuft nicht als App (Entwicklungsstart)".into())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[tauri::command]
pub fn autostart_get() -> bool {
    plist().is_some_and(|p| p.exists())
}

#[tauri::command]
pub fn autostart_set(on: bool) -> Result<(), String> {
    let f = plist().ok_or("Benutzerordner nicht gefunden")?;
    if !on {
        // schon weg ist auch gut
        return match std::fs::remove_file(&f) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let app = app_bundle()?;
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>/usr/bin/open</string><string>{}</string></array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
        xml_escape(&app.to_string_lossy())
    );
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&f, xml).map_err(|e| e.to_string())
}
