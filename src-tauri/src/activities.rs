//! Live-Activity-Schnittstelle: jede App kann per HTTP auf 127.0.0.1:47800 etwas in die Notch legen.
//!
//!   POST   /activity         JSON-Body (siehe Activity) -> anlegen oder ersetzen (gleiche id)
//!   DELETE /activity/<id>    entfernen
//!   GET    /activities       aktuelle Liste
//!   GET    /events?after=N   Klicks auf Aktionsknoepfe (fuer Apps ohne eigenen Server)
//!   GET    /health           "ok"
//!
//! Nur localhost. Browser-Seiten duerfen nur von localhost/tauri aus schreiben,
//! damit nicht jede Website, die du besuchst, dir Zeug in die Notch spammt.

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tiny_http::{Header, Method, Response, Server};

pub const PORT: u16 = 47800;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Action {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// eingebautes Symbol: play, pause, stop, plus, folder, open, check, close
    #[serde(default)]
    pub icon: Option<String>,
    /// beim Klick oeffnen (gleiche Regeln wie Activity.open)
    #[serde(default)]
    pub open: Option<String>,
    /// beim Klick {"activity","action"} an diese localhost-URL posten
    #[serde(default)]
    pub post: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Activity {
    /// eindeutig pro Sache, z. B. "blutzucker" oder "folio-export"
    pub id: String,
    /// Absender, z. B. "Glukose"
    #[serde(default)]
    pub app: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    /// grosser Wert rechts, z. B. "112"
    #[serde(default)]
    pub value: Option<String>,
    /// kleine Einheit hinter dem Wert, z. B. "mg/dL"
    #[serde(default)]
    pub unit: Option<String>,
    /// Emoji, kurzer Text oder data:-URL
    #[serde(default)]
    pub icon: Option<String>,
    /// Akzentfarbe, CSS-Farbe
    #[serde(default)]
    pub color: Option<String>,
    /// 0..1 -> Fortschrittsbalken, negativ -> laufender Balken ohne Ende
    #[serde(default)]
    pub progress: Option<f64>,
    /// Sekunden bis zum automatischen Verschwinden
    #[serde(default)]
    pub ttl: Option<u64>,
    /// hoeher = weiter vorn; die hoechste zeigt die Notch im kompakten Zustand
    #[serde(default)]
    pub priority: i32,
    /// true = Notch klappt kurz auf (wie ein Anruf auf dem iPhone)
    #[serde(default)]
    pub alert: bool,
    /// Klick auf die Zeile: URL, Datei, "reveal:<pfad>" oder Programm (.exe wird nach vorn geholt)
    #[serde(default)]
    pub open: Option<String>,
    /// Knoepfe unter der Zeile
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default, skip_deserializing)]
    pub updated: u64,
}

struct Entry {
    act: Activity,
    expires: Option<Instant>,
}

#[derive(Serialize, Clone)]
struct Event {
    seq: u64,
    activity: String,
    action: String,
    ts: u64,
}

static STORE: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(Default::default);
static EVENTS: LazyLock<Mutex<VecDeque<Event>>> = LazyLock::new(Default::default);
static SEQ: AtomicU64 = AtomicU64::new(0);

pub fn list() -> Vec<Activity> {
    let store = STORE.lock().unwrap();
    let mut v: Vec<Activity> = store.values().map(|e| e.act.clone()).collect();
    v.sort_by(|a, b| b.priority.cmp(&a.priority).then(b.updated.cmp(&a.updated)));
    v
}

pub fn get(id: &str) -> Option<Activity> {
    STORE.lock().unwrap().get(id).map(|e| e.act.clone())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn origin_ok(o: &str) -> bool {
    o == "null"
        || o.starts_with("http://localhost")
        || o.starts_with("http://127.0.0.1")
        || o.starts_with("tauri://")
        || o.starts_with("http://tauri.localhost")
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

pub fn upsert(app: &AppHandle, mut act: Activity) {
    act.updated = now_ms();
    let expires = act.ttl.map(|s| Instant::now() + Duration::from_secs(s));
    let alert = act.alert;
    let id = act.id.clone();
    STORE.lock().unwrap().insert(id.clone(), Entry { act, expires });
    let _ = app.emit("activities", list());
    if alert {
        let _ = app.emit("activity-alert", id);
    }
}

pub fn remove(app: &AppHandle, id: &str) -> bool {
    let hit = STORE.lock().unwrap().remove(id).is_some();
    if hit {
        let _ = app.emit("activities", list());
    }
    hit
}

fn push_event(activity: &str, action: &str) {
    let mut q = EVENTS.lock().unwrap();
    q.push_back(Event {
        seq: SEQ.fetch_add(1, Ordering::Relaxed) + 1,
        activity: activity.into(),
        action: action.into(),
        ts: now_ms(),
    });
    while q.len() > 100 {
        q.pop_front();
    }
}

pub fn spawn(app: AppHandle) {
    // Abgelaufene Eintraege aufraeumen
    let app2 = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let now = Instant::now();
        let removed = {
            let mut s = STORE.lock().unwrap();
            let before = s.len();
            s.retain(|_, e| e.expires.map_or(true, |t| t > now));
            before != s.len()
        };
        if removed {
            let _ = app2.emit("activities", list());
        }
    });

    std::thread::spawn(move || {
        let server = match Server::http(("127.0.0.1", PORT)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[notch] Port {PORT} belegt: {e}");
                return;
            }
        };
        for mut req in server.incoming_requests() {
            let origin = req
                .headers()
                .iter()
                .find(|h| h.field.equiv("Origin"))
                .map(|h| h.value.to_string());
            if let Some(o) = &origin {
                if !origin_ok(o) {
                    let _ = req.respond(Response::from_string("forbidden origin").with_status_code(403));
                    continue;
                }
            }
            let cors = |r: Response<std::io::Cursor<Vec<u8>>>| match &origin {
                Some(o) => r
                    .with_header(header("Access-Control-Allow-Origin", o))
                    .with_header(header("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS"))
                    .with_header(header("Access-Control-Allow-Headers", "Content-Type")),
                None => r,
            };
            let json = header("Content-Type", "application/json; charset=utf-8");
            let full = req.url().to_string();
            let (url, query) = full.split_once('?').unwrap_or((full.as_str(), ""));
            let url = url.to_string();
            let method = req.method().clone();

            let resp = match (method, url.as_str()) {
                (Method::Options, _) => Response::from_string("").with_status_code(204),
                (Method::Get, "/health") => Response::from_string("ok"),
                (Method::Get, "/activities") => {
                    Response::from_string(serde_json::to_string(&list()).unwrap()).with_header(json)
                }
                (Method::Get, "/events") => {
                    let after: u64 = query
                        .split('&')
                        .find_map(|kv| kv.strip_prefix("after="))
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let ev: Vec<Event> =
                        EVENTS.lock().unwrap().iter().filter(|e| e.seq > after).cloned().collect();
                    Response::from_string(serde_json::to_string(&ev).unwrap()).with_header(json)
                }
                (Method::Post, "/activity") => {
                    let mut body = String::new();
                    let _ = req.as_reader().take(512 * 1024).read_to_string(&mut body);
                    match serde_json::from_str::<Activity>(&body) {
                        Ok(a) if a.id.starts_with("notch:") => {
                            Response::from_string("ids mit notch: sind reserviert").with_status_code(400)
                        }
                        Ok(a) if !a.id.is_empty() => {
                            upsert(&app, a);
                            Response::from_string("{\"ok\":true}").with_header(json)
                        }
                        Ok(_) => Response::from_string("id fehlt").with_status_code(400),
                        Err(e) => Response::from_string(format!("ungueltiges JSON: {e}")).with_status_code(400),
                    }
                }
                (Method::Post, "/shelf") => {
                    // {"paths": ["C:\\…\\datei.pdf"]} -> Dateien in die Ablage legen (z. B. nach einem Export)
                    let mut body = String::new();
                    let _ = req.as_reader().take(256 * 1024).read_to_string(&mut body);
                    #[derive(Deserialize)]
                    struct B {
                        paths: Vec<String>,
                    }
                    match serde_json::from_str::<B>(&body) {
                        Ok(b) => {
                            crate::shelf::shelf_add(app.clone(), b.paths);
                            Response::from_string("{\"ok\":true}").with_header(json)
                        }
                        Err(e) => Response::from_string(format!("ungueltiges JSON: {e}")).with_status_code(400),
                    }
                }
                (Method::Delete, p) if p.starts_with("/activity/") => {
                    let id = &p["/activity/".len()..];
                    if remove(&app, id) {
                        Response::from_string("{\"ok\":true}").with_header(json)
                    } else {
                        Response::from_string("nicht gefunden").with_status_code(404)
                    }
                }
                _ => Response::from_string("not found").with_status_code(404),
            };
            let _ = req.respond(cors(resp));
        }
    });
}

#[tauri::command]
pub fn dismiss_activity(app: AppHandle, id: String) {
    if id == crate::timer::ID {
        crate::timer::action(&app, "stop");
    }
    remove(&app, &id);
    push_event(&id, "dismiss");
}

/// Klick auf die Zeile selbst.
#[tauri::command]
pub fn activity_open(app: AppHandle, id: String) -> Result<(), String> {
    push_event(&id, "open");
    match get(&id).and_then(|a| a.open) {
        Some(t) => crate::open::open_target(&app, &t),
        None => Ok(()),
    }
}

/// Klick auf einen Aktionsknopf.
#[tauri::command]
pub fn activity_action(app: AppHandle, id: String, action: String) -> Result<(), String> {
    push_event(&id, &action);
    if id == crate::timer::ID {
        crate::timer::action(&app, &action);
        return Ok(());
    }
    let Some(act) = get(&id) else { return Ok(()) };
    let Some(a) = act.actions.iter().find(|a| a.id == action) else { return Ok(()) };
    if let Some(url) = &a.post {
        crate::open::post_local(url, serde_json::json!({ "activity": id, "action": action }).to_string());
    }
    if let Some(t) = &a.open {
        crate::open::open_target(&app, t)?;
    }
    if a.id == "dismiss" {
        remove(&app, &id);
    }
    Ok(())
}
