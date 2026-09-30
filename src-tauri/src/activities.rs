//! Live-Activity-Schnittstelle: jede App kann per HTTP auf 127.0.0.1:47800 etwas in die Notch legen.
//!
//!   POST   /activity        JSON-Body (siehe Activity) -> anlegen oder ersetzen (gleiche id)
//!   DELETE /activity/<id>   entfernen
//!   GET    /activities      aktuelle Liste
//!   GET    /health          "ok"
//!
//! Nur localhost. Browser-Seiten duerfen nur von localhost/tauri aus schreiben,
//! damit nicht jede Website, die du besuchst, dir Zeug in die Notch spammt.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tiny_http::{Header, Method, Response, Server};

pub const PORT: u16 = 47800;

#[derive(Serialize, Deserialize, Clone, Debug)]
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
    /// 0..1 -> Fortschrittsbalken
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
    #[serde(default, skip_deserializing)]
    pub updated: u64,
}

struct Entry {
    act: Activity,
    expires: Option<Instant>,
}

static STORE: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(Default::default);

pub fn list() -> Vec<Activity> {
    let store = STORE.lock().unwrap();
    let mut v: Vec<Activity> = store.values().map(|e| e.act.clone()).collect();
    v.sort_by(|a, b| b.priority.cmp(&a.priority).then(b.updated.cmp(&a.updated)));
    v
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

fn upsert(app: &AppHandle, mut act: Activity) {
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
            let url = req.url().split('?').next().unwrap_or("").to_string();
            let method = req.method().clone();

            let resp = match (method, url.as_str()) {
                (Method::Options, _) => Response::from_string("").with_status_code(204),
                (Method::Get, "/health") => Response::from_string("ok"),
                (Method::Get, "/activities") => {
                    Response::from_string(serde_json::to_string(&list()).unwrap()).with_header(json)
                }
                (Method::Post, "/activity") => {
                    let mut body = String::new();
                    let _ = req.as_reader().take(256 * 1024).read_to_string(&mut body);
                    match serde_json::from_str::<Activity>(&body) {
                        Ok(a) if !a.id.is_empty() => {
                            upsert(&app, a);
                            Response::from_string("{\"ok\":true}").with_header(json)
                        }
                        Ok(_) => Response::from_string("id fehlt").with_status_code(400),
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
    remove(&app, &id);
}
