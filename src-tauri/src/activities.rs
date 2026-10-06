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
    /// grosser Wert rechts, z. B. "112" (Zahl oder Text)
    #[serde(default, deserialize_with = "num_or_str")]
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
    /// Countdown: Zeitpunkt (ms seit 1970), an dem progress 0 erreicht -> Balken laeuft fluessig
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_at: Option<u64>,
    /// Eingabefeld in der aufgeklappten Zeile (z. B. Suche). Tippen/Enter landen als
    /// Ereignisse "input" / "submit" / "submit-prev" mit `value` in GET /events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Input>,
    /// Herzfrequenz (Schlaege/min): das Symbol schlaegt in diesem Takt, z. B. Helio
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pulse: Option<f64>,
    /// Messwert-Trend: up2 | up | up45 | flat | down45 | down | down2
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<String>,
    /// Wert minus vorheriger Messwert
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<f64>,
    /// Verlauf (z. B. Blutzucker 24 h); die Notch schneidet die Zeitbereiche selbst
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<Chart>,
    /// LoL-Profispiel (Vantage, esnotch.rs): fertige Daten fuer die eigene Ansicht der Notch (src/esports.ts)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub esports: Option<serde_json::Value>,
    /// Prozess der App — fuer AllowSetForegroundWindow beim Doppelklick
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// ttl abgelaufen: Eintraege mit Verlauf bleiben stehen und zeigen "keine Daten"
    #[serde(default, skip_deserializing)]
    pub expired: bool,
    #[serde(default, skip_deserializing)]
    pub updated: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Chart {
    /// Zielbereich: darunter rot, darueber bernstein
    #[serde(default)]
    pub low: f64,
    #[serde(default)]
    pub high: f64,
    /// [[epoch_ms, wert], ...] — aelteste zuerst
    #[serde(default)]
    pub points: Vec<(f64, f64)>,
    #[serde(default)]
    pub ranges: Vec<u32>,
    #[serde(default)]
    pub range: Option<u32>,
}

/// "112" oder 112 -> Some("112")
fn num_or_str<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    use serde_json::Value;
    Ok(match Option::<Value>::deserialize(d)? {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s),
        Some(Value::Number(n)) => Some(match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        }),
        Some(v) => Some(v.to_string()),
    })
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Input {
    #[serde(default)]
    pub placeholder: String,
    /// aktueller Text aus Sicht der App (die Notch uebernimmt ihn, solange man nicht tippt)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
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

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// Browser-Herkunft: nur eigene Tauri-Fenster und Seiten von genau localhost/127.0.0.1/[::1].
/// Kein "null" (sandboxed iframes, data:-URLs — damit koennte jede Website schreiben) und kein
/// Praefixvergleich (sonst kaeme http://localhost.angreifer.de durch).
fn origin_ok(o: &str) -> bool {
    if matches!(o, "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost") {
        return true;
    }
    o.strip_prefix("http://").is_some_and(loopback_host)
}

/// "127.0.0.1", "localhost:5173", "[::1]:80" -> true; alles andere false.
/// Auch fuer den Host-Header: schuetzt vor DNS-Rebinding (fremde Domain, die auf 127.0.0.1 zeigt).
fn loopback_host(hostport: &str) -> bool {
    let host = match hostport.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => hostport,
    };
    matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "[::1]")
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

pub fn upsert(app: &AppHandle, mut act: Activity) {
    act.updated = now_ms();
    act.expired = false;
    if let Some(c) = act.chart.as_mut() {
        // nur gueltige Punkte der letzten 24 h, hoechstens 2000
        c.points.retain(|p| p.0.is_finite() && p.1.is_finite());
        c.points.sort_by(|a, b| a.0.total_cmp(&b.0));
        if let Some(&(last, _)) = c.points.last() {
            c.points.retain(|p| p.0 >= last - 24.5 * 3_600_000.0);
        }
        let n = c.points.len();
        if n > 2000 {
            c.points.drain(..n - 2000);
        }
    }
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

pub fn push_event(activity: &str, action: &str) {
    push_event_value(activity, action, None);
}

fn push_event_value(activity: &str, action: &str, value: Option<String>) {
    let mut q = EVENTS.lock().unwrap();
    q.push_back(Event {
        seq: SEQ.fetch_add(1, Ordering::Relaxed) + 1,
        activity: activity.into(),
        action: action.into(),
        value,
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
            // Eintraege mit Verlauf (Messwerte) verschwinden nicht einfach: sie bleiben als
            // "keine Daten" stehen, bis die App sie loescht oder man sie wegklickt.
            let mut marked = false;
            for e in s.values_mut() {
                if e.act.chart.is_some() && e.expires.is_some_and(|t| t <= now) {
                    e.expires = None;
                    e.act.expired = true;
                    marked = true;
                }
            }
            s.retain(|_, e| e.expires.map_or(true, |t| t > now));
            before != s.len() || marked
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
            let host_ok = req
                .headers()
                .iter()
                .find(|h| h.field.equiv("Host"))
                .is_none_or(|h| loopback_host(h.value.as_str()));
            if !host_ok {
                let _ = req.respond(Response::from_string("forbidden host").with_status_code(403));
                continue;
            }
            if let Some(o) = &origin {
                if !origin_ok(o) {
                    let _ = req.respond(Response::from_string("forbidden origin").with_status_code(403));
                    continue;
                }
            }
            let cors = |r: Response<std::io::Cursor<Vec<u8>>>| match &origin {
                Some(o) => r
                    .with_header(header("Access-Control-Allow-Origin", o))
                    .with_header(header("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS"))
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
                // Sport-Einstellungen fuer Arena (eigene Sport-App): lesen und schreiben, bereinigt
                (Method::Get, "/sport/settings") => {
                    let v = crate::sport::sanitize_settings(&crate::settings_value("/sport").unwrap_or(serde_json::Value::Null));
                    Response::from_string(v.to_string()).with_header(json)
                }
                (Method::Put, "/sport/settings") => {
                    let mut body = String::new();
                    let _ = req.as_reader().take(64 * 1024).read_to_string(&mut body);
                    match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(v) if v.is_object() => {
                            let clean = crate::sport::sanitize_settings(&v);
                            let out = clean.to_string();
                            crate::settings_update(&app, |s| s["sport"] = clean);
                            Response::from_string(out).with_header(json)
                        }
                        Ok(_) => Response::from_string("Objekt erwartet").with_status_code(400),
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
    // Wegklicken einer Anfrage vom iPhone = ablehnen
    if id == crate::share::ASK_ID {
        crate::share::action(&app, &id, "decline");
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

/// Eingabefeld einer Zeile: kind = "input" (getippt), "submit" (Enter), "submit-prev" (Umschalt+Enter).
#[tauri::command]
pub fn activity_input(id: String, kind: String, value: String) {
    if matches!(kind.as_str(), "input" | "submit" | "submit-prev") {
        push_event_value(&id, &kind, Some(value.chars().take(500).collect()));
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
    if id == crate::update::ID {
        crate::update::action(&app, &action);
        return Ok(());
    }
    if id.starts_with("notch:share:") {
        crate::share::action(&app, &id, &action);
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

#[cfg(test)]
mod tests {
    use super::{loopback_host, origin_ok};

    #[test]
    fn origin_nur_eigene_fenster_und_loopback() {
        for o in ["http://localhost", "http://localhost:5173", "http://127.0.0.1:8080", "http://[::1]:3000",
                  "tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"] {
            assert!(origin_ok(o), "{o} sollte erlaubt sein");
        }
        for o in ["null", "http://localhost.angreifer.de", "http://127.0.0.1.nip.io", "http://localhost:80.evil.de",
                  "https://localhost", "https://example.com", "tauri://evil", "http://127.0.0.1:abc", ""] {
            assert!(!origin_ok(o), "{o} sollte abgewiesen werden");
        }
    }

    #[test]
    fn host_nur_loopback() {
        for h in ["127.0.0.1", "127.0.0.1:47800", "localhost:47800", "LOCALHOST", "[::1]:47800"] {
            assert!(loopback_host(h), "{h} sollte erlaubt sein");
        }
        for h in ["rebind.angreifer.de:47800", "localhost.angreifer.de", "192.168.0.5:47800", ""] {
            assert!(!loopback_host(h), "{h} sollte abgewiesen werden");
        }
    }
}
