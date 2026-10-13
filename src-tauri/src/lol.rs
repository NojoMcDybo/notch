//! Jarvis: League-of-Legends-Runde per Sprache starten — ueber die lokale Schnittstelle des Clients (LCU), wie
//! Vantage (vantage/src-tauri/src/me.rs: Lockfile, Basic-Auth "riot:<passwort>", selbst signiertes Zertifikat, nur
//! 127.0.0.1). Lobby fuer einen Modus anlegen und die Suche starten. Annehmen und Championauswahl bleiben beim
//! Spieler; das Spiel selbst wird nicht automatisiert.
//!
//! Laeuft der Client noch nicht, startet ihn Jarvis und wartet im Hintergrund (bis 3 Minuten, bis man eingeloggt
//! ist); die Sprachsitzung bekommt sofort eine Antwort.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::activities::{self, Activity};

const ID: &str = "notch:jarvis-lol";

static LOCAL: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(4)))
        .tls_config(ureq::tls::TlsConfig::builder().disable_verification(true).build())
        .http_status_as_error(false)
        .build()
        .into()
});

/// gesprochener Modus -> Warteschlange
const QUEUES: &[(&str, u32, &str)] = &[
    ("aram", 450, "ARAM"),
    ("normal", 400, "Normal (Draft)"),
    ("draft", 400, "Normal (Draft)"),
    ("ranked", 420, "Ranked Solo/Duo"),
    ("solo", 420, "Ranked Solo/Duo"),
    ("flex", 440, "Ranked Flex"),
    ("swiftplay", 480, "Swiftplay"),
    ("arena", 1700, "Arena"),
];

fn queue(modus: &str) -> Option<(u32, &'static str)> {
    let m = modus.to_lowercase();
    QUEUES.iter().find(|q| m.contains(q.0)).map(|q| (q.1, q.2))
}

/// Installationsordner: Riot-Metadaten (product_install_full_path), sonst der Standardpfad (wie Vantage)
fn install_dir() -> PathBuf {
    let meta = PathBuf::from(r"C:\ProgramData\Riot Games\Metadata\league_of_legends.live\league_of_legends.live.product_settings.yaml");
    if let Ok(text) = std::fs::read_to_string(meta) {
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix("product_install_full_path:") {
                let p = PathBuf::from(v.trim().trim_matches('"'));
                if p.exists() {
                    return p;
                }
            }
        }
    }
    PathBuf::from(r"C:\Riot Games\League of Legends")
}

struct Lcu {
    port: u16,
    auth: String,
}

/// "LeagueClient:pid:port:passwort:https"
fn parse_lockfile(text: &str) -> Option<Lcu> {
    let p: Vec<&str> = text.trim().split(':').collect();
    if p.len() < 5 {
        return None;
    }
    Some(Lcu { port: p[2].parse().ok()?, auth: format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("riot:{}", p[3]))) })
}

fn lockfile() -> Option<Lcu> {
    parse_lockfile(&std::fs::read_to_string(install_dir().join("lockfile")).ok()?)
}

impl Lcu {
    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        let url = format!("https://127.0.0.1:{}{path}", self.port);
        let mut r = match (method, body) {
            ("GET", _) => LOCAL.get(&url).header("Authorization", &self.auth).call(),
            (_, Some(b)) => LOCAL.post(&url).header("Authorization", &self.auth).send_json(b),
            (_, None) => LOCAL.post(&url).header("Authorization", &self.auth).send_empty(),
        }
        .map_err(|e| e.to_string())?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().unwrap_or_default();
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status >= 400 {
            return Err(v["message"].as_str().map(String::from).unwrap_or_else(|| format!("Client-Fehler {status}")));
        }
        Ok(v)
    }

    /// eingeloggt und ansprechbar? -> Spielphase ("None", "Lobby", "Matchmaking", "ChampSelect", "InProgress" …)
    fn phase(&self) -> Option<String> {
        self.call("GET", "/lol-gameflow/v1/gameflow-phase", None).ok().and_then(|v| v.as_str().map(String::from))
    }
}

fn show(app: &AppHandle, title: &str, sub: &str) {
    activities::upsert(
        app,
        Activity {
            id: ID.into(),
            app: "Jarvis".into(),
            title: title.into(),
            subtitle: Some(sub.into()),
            icon: Some("🎮".into()),
            color: Some("#8fb8ff".into()),
            ttl: Some(8),
            priority: 1,
            ..Default::default()
        },
    );
}

/// Lobby anlegen und Suche starten
fn start(lcu: &Lcu, id: u32) -> Result<(), String> {
    match lcu.phase().as_deref() {
        Some("ChampSelect") => return Err("Du bist gerade in der Championauswahl.".into()),
        Some("InProgress") | Some("Reconnect") => return Err("Du bist gerade im Spiel.".into()),
        _ => {}
    }
    lcu.call("POST", "/lol-lobby/v2/lobby", Some(json!({ "queueId": id })))?;
    lcu.call("POST", "/lol-lobby/v2/lobby/matchmaking/search", None)?;
    Ok(())
}

static WAITING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn lol_queue(app: AppHandle, modus: String) -> Result<Value, String> {
    let (id, label) = queue(&modus).ok_or_else(|| format!("Modus „{modus}“ kenne ich nicht (ARAM, Normal, Ranked, Flex, Swiftplay, Arena)."))?;
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(lcu) = lockfile().filter(|l| l.phase().is_some()) {
            start(&lcu, id)?;
            show(&app, label, "Suche läuft");
            return Ok(json!({ "suche": label }));
        }
        // Client starten und im Hintergrund warten, bis man eingeloggt ist
        if WAITING.swap(true, Ordering::SeqCst) {
            return Ok(json!({ "hinweis": "Der Client startet schon; die Suche beginnt, sobald er bereit ist." }));
        }
        crate::jarvis::open_app(&app, "League of Legends").map_err(|e| {
            WAITING.store(false, Ordering::SeqCst);
            e
        })?;
        show(&app, label, "Client startet – Suche beginnt danach");
        std::thread::spawn(move || {
            let t = Instant::now();
            let mut result = Err("Der League-Client war nach 3 Minuten nicht bereit (eingeloggt?).".to_string());
            while t.elapsed() < Duration::from_secs(180) {
                std::thread::sleep(Duration::from_secs(3));
                if let Some(lcu) = lockfile().filter(|l| l.phase().is_some()) {
                    // kurz warten: direkt nach dem Login lehnt der Client Lobbys noch ab
                    std::thread::sleep(Duration::from_secs(4));
                    result = start(&lcu, id);
                    break;
                }
            }
            WAITING.store(false, Ordering::SeqCst);
            match result {
                Ok(()) => show(&app, label, "Suche läuft"),
                Err(e) => show(&app, label, &e),
            }
        });
        Ok(json!({ "hinweis": format!("Client startet; die Suche nach {label} beginnt automatisch, sobald er bereit ist.") }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modi_und_lockfile() {
        assert_eq!(queue("ARAM").unwrap().0, 450);
        assert_eq!(queue("eine Runde ranked").unwrap().0, 420);
        assert_eq!(queue("Flex").unwrap().0, 440);
        assert!(queue("Schach").is_none());
        let l = parse_lockfile("LeagueClient:9892:55538:geheim:https").unwrap();
        assert_eq!(l.port, 55538);
        assert_eq!(l.auth, "Basic cmlvdDpnZWhlaW0=");
        assert!(parse_lockfile("kaputt").is_none());
    }

    /// Nur lesen (Spielphase), legt nichts an: cargo test --lib lol_phase -- --ignored --nocapture
    #[test]
    #[ignore]
    fn lol_phase() {
        let l = lockfile().expect("Client laeuft nicht (kein lockfile)");
        println!("Phase: {:?}", l.phase());
    }
}
