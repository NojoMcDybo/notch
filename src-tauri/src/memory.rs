//! Gedaechtnis des Sprachassistenten: Ablaeufe ("Zocken" = Playlist + Spiel + Discord-Kanal …) und gelernte
//! Discord-Sprachkanaele (Kanal -> Server), damit man nur noch den Kanal nennen muss.
//!
//! Ablage: `jarvis-memory.json` im App-Ordner. Ablaeufe legt der Assistent per Werkzeug an (merken/vergessen);
//! Discord-Kanaele lernt `discord_channel` von selbst bei jedem erfolgreichen Beitritt. Der Inhalt geht bei jedem
//! Gespraech als kurzer Absatz in die Anweisungen (main.ts), daher klein gehalten.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::voice::data_file;

const FILE: &str = "jarvis-memory.json";
const MAX_ROUTINES: usize = 40;
const MAX_CHANNELS: usize = 40;
const MAX_TEXT: usize = 600;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Routine {
    name: String,
    inhalt: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Channel {
    kanal: String,
    server: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Recent {
    /// "musik" | "programm"
    art: String,
    was: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Memory {
    #[serde(default)]
    ablaeufe: Vec<Routine>,
    #[serde(default)]
    discord: Vec<Channel>,
    /// zuletzt Gespieltes/Gestartetes, fuer Vorhersagen ("mach Musik an", "starte mein Spiel")
    #[serde(default)]
    zuletzt: Vec<Recent>,
}

static LOCK: Mutex<()> = Mutex::new(());

fn norm(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(s: &str, n: usize) -> String {
    s.trim().chars().take(n).collect()
}

fn load(app: &AppHandle) -> Memory {
    data_file(app, FILE).and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(app: &AppHandle, m: &Memory) -> Result<(), String> {
    let f = data_file(app, FILE).ok_or("kein App-Ordner")?;
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(f, serde_json::to_string_pretty(m).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

impl Memory {
    /// gleicher Name (ohne Gross/Klein, Satzzeichen) ersetzt; neueste vorn, aelteste fallen bei Ueberlauf raus
    fn set_routine(&mut self, name: &str, inhalt: &str) {
        let k = norm(name);
        self.ablaeufe.retain(|r| norm(&r.name) != k);
        self.ablaeufe.insert(0, Routine { name: clip(name, 60), inhalt: clip(inhalt, MAX_TEXT) });
        self.ablaeufe.truncate(MAX_ROUTINES);
    }

    fn remove_routine(&mut self, name: &str) -> bool {
        let k = norm(name);
        let n = self.ablaeufe.len();
        self.ablaeufe.retain(|r| norm(&r.name) != k);
        n != self.ablaeufe.len()
    }

    fn learn_channel(&mut self, kanal: &str, server: &str) {
        let k = norm(kanal);
        self.discord.retain(|c| norm(&c.kanal) != k);
        self.discord.insert(0, Channel { kanal: clip(kanal, 80), server: clip(server, 80) });
        self.discord.truncate(MAX_CHANNELS);
    }

    fn learn_recent(&mut self, art: &str, was: &str) {
        let k = norm(was);
        self.zuletzt.retain(|r| !(r.art == art && norm(&r.was) == k));
        self.zuletzt.insert(0, Recent { art: art.into(), was: clip(was, 80) });
        // je Art hoechstens 6
        let mut seen = std::collections::HashMap::<String, usize>::new();
        self.zuletzt.retain(|r| {
            let n = seen.entry(r.art.clone()).or_default();
            *n += 1;
            *n <= 6
        });
    }

    fn server_of(&self, kanal: &str) -> Option<String> {
        let k = norm(kanal);
        self.discord.iter().find(|c| norm(&c.kanal) == k).or_else(|| self.discord.iter().find(|c| norm(&c.kanal).contains(&k))).map(|c| c.server.clone())
    }
}

/// Erfolgreich Gespieltes/Gestartetes merken (Spotify, Programme) — Fehler hier sind egal
pub fn remember(app: &AppHandle, art: &str, was: &str) {
    let _g = LOCK.lock().unwrap();
    let mut m = load(app);
    m.learn_recent(art, was);
    let _ = save(app, &m);
}

#[tauri::command]
pub fn memory_get(app: AppHandle) -> Memory {
    load(&app)
}

#[tauri::command]
pub fn memory_set(app: AppHandle, name: String, inhalt: String) -> Result<Value, String> {
    if name.trim().is_empty() || inhalt.trim().is_empty() {
        return Err("Name und Inhalt nötig.".into());
    }
    let _g = LOCK.lock().unwrap();
    let mut m = load(&app);
    m.set_routine(&name, &inhalt);
    save(&app, &m)?;
    Ok(json!({ "gemerkt": name.trim() }))
}

#[tauri::command]
pub fn memory_delete(app: AppHandle, name: String) -> Result<Value, String> {
    let _g = LOCK.lock().unwrap();
    let mut m = load(&app);
    if !m.remove_routine(&name) {
        return Err(format!("„{name}“ war nicht gemerkt."));
    }
    save(&app, &m)?;
    Ok(json!({ "vergessen": name }))
}

/// Discord-Sprachkanal beitreten — das Ziel wird vorhergesagt, wo Angaben fehlen:
/// ohne Kanal der zuletzt genutzte (mit seinem Server), ohne Server der gelernte Server zum Kanal, sonst der offene.
#[tauri::command]
pub async fn discord_channel(app: AppHandle, kanal: Option<String>, server: Option<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mem = load(&app);
        let (kanal, server) = match kanal.filter(|k| !k.trim().is_empty()) {
            Some(k) => {
                let sv = server.filter(|s| !s.trim().is_empty()).or_else(|| mem.server_of(&k));
                (k, sv)
            }
            None => {
                let last = mem.discord.first().ok_or("Welcher Sprachkanal? Ich kenne noch keinen.")?;
                (last.kanal.clone(), Some(last.server.clone()))
            }
        };
        // Discord nach vorn bzw. starten; frisch gestartet braucht es eine Weile, bis das Fenster steht
        let fresh = crate::jarvis::open_app(&app, "Discord").map_err(|e| format!("Discord startet nicht: {e}"))?.started();
        let tries = if fresh { 30 } else { 1 };
        let mut last = Err("Discord hat kein Fenster.".to_string());
        for i in 0..tries {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            last = crate::screen::discord_join(server.clone(), kanal.clone());
            if last.is_ok() {
                break;
            }
        }
        let v = last?;
        let sv = v["server"].as_str().filter(|s| !s.is_empty()).map(String::from).or(server);
        if let (Some(sv), Some(k)) = (sv.as_deref(), v["beigetreten"].as_str()) {
            let _g = LOCK.lock().unwrap();
            let mut m = load(&app);
            m.learn_channel(k, sv);
            let _ = save(&app, &m);
        }
        Ok(v)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ablaeufe_und_kanaele() {
        let mut m = Memory::default();
        m.set_routine("Zocken", "LOL-Playlist, League starten");
        m.set_routine("zocken!", "neu");
        assert_eq!(m.ablaeufe.len(), 1);
        assert_eq!(m.ablaeufe[0].inhalt, "neu");
        assert!(m.remove_routine("ZOCKEN"));
        assert!(!m.remove_routine("Zocken"));
        m.learn_channel("Lounge", "Speerwurf");
        m.learn_channel("Stream-Raum", "Speerwurf");
        m.learn_channel("lounge", "Olymp");
        assert_eq!(m.discord.len(), 2);
        assert_eq!(m.server_of("Lounge").as_deref(), Some("Olymp"));
        assert_eq!(m.server_of("stream").as_deref(), Some("Speerwurf"));
        assert_eq!(m.server_of("Keller"), None);
        for i in 0..9 {
            m.learn_recent("musik", &format!("Playlist {i}"));
        }
        m.learn_recent("programm", "Rocket League");
        m.learn_recent("musik", "playlist 3");
        assert_eq!(m.zuletzt.iter().filter(|r| r.art == "musik").count(), 6);
        assert_eq!(m.zuletzt[0].was, "playlist 3");
        assert_eq!(m.zuletzt.iter().filter(|r| norm(&r.was) == "playlist 3").count(), 1);
        m.set_routine("x", &"a".repeat(2000));
        assert_eq!(m.ablaeufe[0].inhalt.len(), MAX_TEXT);
    }
}
