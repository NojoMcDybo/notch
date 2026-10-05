//! Tastenkuerzel des Sprachassistenten (Tastatur). Einstellungen › App: eigenes Kuerzel aufnehmen; leer =
//! automatisch das erste freie aus AUTO (Strg+Alt+Leertaste belegt z. B. die Claude-App).
//! Die Controller-Kombinationen stehen in gamepad.rs (PadCfg).
//!
//! Waehrend das Einstellungsfenster ein neues Kuerzel aufnimmt, ist das alte abgemeldet (shortcut_pause),
//! sonst wuerde Windows den Tastendruck abfangen, bevor das Fenster ihn sieht.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

const AUTO: [&str; 3] = ["ctrl+alt+Space", "ctrl+shift+alt+Space", "ctrl+alt+KeyN"];

/// angemeldetes Kuerzel (auch waehrend der Pause gemerkt)
static CURRENT: Mutex<String> = Mutex::new(String::new());

/// "ctrl+alt+Space" -> "Strg+Alt+Leertaste"
pub fn label(acc: &str) -> String {
    acc.split('+')
        .filter(|p| !p.is_empty())
        .map(|p| match p.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "commandorcontrol" => "Strg".to_string(),
            "alt" | "option" => "Alt".to_string(),
            "shift" => "Umschalt".to_string(),
            "super" | "meta" | "win" | "cmd" => "Win".to_string(),
            "space" => "Leertaste".to_string(),
            "arrowup" => "↑".to_string(),
            "arrowdown" => "↓".to_string(),
            "arrowleft" => "←".to_string(),
            "arrowright" => "→".to_string(),
            "enter" => "Eingabe".to_string(),
            "backspace" => "Rücktaste".to_string(),
            "escape" => "Esc".to_string(),
            _ => p.strip_prefix("Key").or_else(|| p.strip_prefix("Digit")).unwrap_or(p).to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Ein Kuerzel braucht Strg, Alt oder Win (Umschalt allein zaehlt nicht) — ausser F13 bis F24, die nichts anderes belegen
pub fn valid(acc: &str) -> bool {
    let parts: Vec<String> = acc.split('+').map(|p| p.to_ascii_lowercase()).collect();
    let Some(key) = parts.last() else { return false };
    let mods = ["ctrl", "control", "alt", "super", "meta", "win"];
    let has_mod = parts[..parts.len() - 1].iter().any(|p| mods.contains(&p.as_str()));
    let free_f = key.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| (13..=24).contains(&n));
    !mods.contains(&key.as_str()) && key != "shift" && (has_mod || free_f) && acc.len() <= 60
}

fn announce(app: &AppHandle, acc: &str) {
    let l = if acc.is_empty() { String::new() } else { label(acc) };
    *crate::VOICE_KEY.lock().unwrap() = l.clone();
    let _ = app.emit("voice-key", l);
}

fn register(app: &AppHandle, acc: &str) -> Result<(), String> {
    app.global_shortcut().register(acc).map_err(|e| e.to_string())?;
    *CURRENT.lock().unwrap() = acc.to_string();
    announce(app, acc);
    Ok(())
}

fn unregister(app: &AppHandle) -> String {
    let cur = std::mem::take(&mut *CURRENT.lock().unwrap());
    if !cur.is_empty() {
        let _ = app.global_shortcut().unregister(cur.as_str());
    }
    cur
}

fn auto(app: &AppHandle) -> Result<String, String> {
    for k in AUTO {
        match register(app, k) {
            Ok(()) => return Ok(label(k)),
            Err(e) => eprintln!("[notch] Kuerzel {} belegt: {e}", label(k)),
        }
    }
    announce(app, "");
    Err("Kein freies Kürzel gefunden – bitte selbst eins festlegen.".into())
}

/// Beim Start: eigenes Kuerzel aus den Einstellungen, sonst (oder wenn belegt) automatisch
pub fn init(app: &AppHandle) {
    let want = crate::settings_value("/shortcuts/voiceKey").and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    if !want.is_empty() && valid(&want) {
        match register(app, &want) {
            Ok(()) => return,
            Err(e) => eprintln!("[notch] eigenes Kuerzel {} belegt: {e}", label(&want)),
        }
    }
    let _ = auto(app);
}

/// Neues Kuerzel setzen ("" = automatisch). Belegt -> das alte bleibt, Fehlermeldung mit Namen.
#[tauri::command]
pub fn shortcut_set(app: AppHandle, key: String) -> Result<String, String> {
    let old = unregister(&app);
    if key.is_empty() {
        return auto(&app);
    }
    if !valid(&key) {
        if !old.is_empty() {
            let _ = register(&app, &old);
        }
        return Err("Bitte mit Strg, Alt oder Win kombinieren (allein gehen nur F13 bis F24).".into());
    }
    match register(&app, &key) {
        Ok(()) => Ok(label(&key)),
        Err(_) => {
            if !old.is_empty() {
                let _ = register(&app, &old);
            }
            Err(format!("{} ist schon von einem anderen Programm belegt.", label(&key)))
        }
    }
}

/// Aufnehmen im Einstellungsfenster: Kuerzel kurz abmelden (pause) bzw. wieder anmelden
#[tauri::command]
pub fn shortcut_pause(app: AppHandle, pause: bool) {
    let cur = CURRENT.lock().unwrap().clone();
    if cur.is_empty() {
        return;
    }
    if pause {
        let _ = app.global_shortcut().unregister(cur.as_str());
    } else if !app.global_shortcut().is_registered(cur.as_str()) {
        let _ = app.global_shortcut().register(cur.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namen_und_regeln() {
        assert_eq!(label("ctrl+alt+Space"), "Strg+Alt+Leertaste");
        assert_eq!(label("ctrl+shift+KeyN"), "Strg+Umschalt+N");
        assert_eq!(label("alt+Digit5"), "Alt+5");
        assert_eq!(label("F14"), "F14");
        assert!(valid("ctrl+alt+Space"));
        assert!(valid("super+KeyJ"));
        assert!(valid("F13"), "F13 bis F24 duerfen allein");
        assert!(!valid("F5"), "F5 allein wuerde Programme stoeren");
        assert!(!valid("shift+KeyA"), "Umschalt allein ist Grossschreibung");
        assert!(!valid("KeyA"));
        assert!(!valid("ctrl+alt"), "nur Zusatztasten");
    }
}
