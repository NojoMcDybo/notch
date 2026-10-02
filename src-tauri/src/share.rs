//! iPhone <-> Notch: Dateien und Text im WLAN austauschen, ohne Cloud und ohne Kabel.
//!
//! Echtes AirDrop geht unter Windows nicht: Apple funkt dafuer ueber ein eigenes WLAN-Protokoll (AWDL)
//! mit Apple-Zertifikaten. Die Notch bietet zwei Wege, die sich fast genauso anfuehlen:
//!
//! 1. LocalSend (kostenlos, Open Source, im App Store). Die Notch spricht das offene LocalSend-Protokoll v2
//!    und taucht in der App als Geraet auf. iPhone: Teilen › LocalSend › „Notch“ — die Notch fragt nach
//!    (Annehmen / Ablehnen / Immer annehmen), die Dateien landen in Downloads\Notch und in der Ablage, Text
//!    erscheint in der Notch zum Kopieren. Umgekehrt: Ablage › Rechtsklick › An iPhone senden (LocalSend offen).
//! 2. Ohne App: QR-Code in der Notch mit der Kamera scannen -> Seite in Safari zum Hoch- und Herunterladen.
//!
//! Sicherheit: aus, bis man es einschaltet (dann lauscht die Notch im lokalen Netz auf Port 53317).
//! Jede Sendung eines Geraets muss bestaetigt werden, ausser man hat es als vertrauenswuerdig markiert.
//! Die Browser-Seite braucht den zufaelligen Schluessel aus dem QR-Code und verfaellt 30 Min nach dem Anzeigen.
//! Uebertragung unverschluesselt (HTTP, wie LocalSend mit ausgeschalteter Verschluesselung) — fuers Heim-WLAN.
//!
//! Einstellungen: Schema im Frontend (settings-model.ts -> "share"): on, name, folder, trusted.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::activities::{self, Action, Activity};

pub const PORT: u16 = 53317;
const GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);
/// so lange darf man ueberlegen, ob man eine Sendung annimmt (LocalSend wartet so lange)
const ASK_FOR: Duration = Duration::from_secs(60);
/// Browser-Seite: so lange gilt ein QR-Code nach dem Anzeigen (jede Nutzung verlaengert)
const WEB_FOR: Duration = Duration::from_secs(30 * 60);
const MAX_FILES: usize = 1000;
const MAX_TOTAL: u64 = 50 * 1024 * 1024 * 1024;

pub const ASK_ID: &str = "notch:share:ask";
const RECV_ID: &str = "notch:share:recv";
const DONE_ID: &str = "notch:share:done";
const TEXT_ID: &str = "notch:share:text";
const SEND_ID: &str = "notch:share:send";

const PHONE_ICON: &str = "data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'%3E%3Cpath fill='%2364d2ff' d='M8 1.8h8A2.2 2.2 0 0 1 18.2 4v16a2.2 2.2 0 0 1-2.2 2.2H8A2.2 2.2 0 0 1 5.8 20V4A2.2 2.2 0 0 1 8 1.8zm0 1.7a.5.5 0 0 0-.5.5v16a.5.5 0 0 0 .5.5h8a.5.5 0 0 0 .5-.5V4a.5.5 0 0 0-.5-.5h-1.6l-.4.8h-3.9l-.4-.8zM10 18.4h4v1.2h-4z'/%3E%3C/svg%3E";

// ---------- Zustand ----------

#[derive(Clone, Debug, Serialize)]
pub struct Device {
    pub fp: String,
    pub alias: String,
    pub model: String,
    pub kind: String,
    #[serde(skip)]
    pub ip: IpAddr,
    #[serde(skip)]
    pub port: u16,
    #[serde(skip)]
    pub https: bool,
    #[serde(skip)]
    pub seen: Option<Instant>,
}

struct InFile {
    name: String,
    size: u64,
    token: String,
    kind: String,
    done: Option<PathBuf>,
}

struct Incoming {
    id: String,
    alias: String,
    ip: IpAddr,
    files: HashMap<String, InFile>,
    total: u64,
    got: u64,
    cancelled: bool,
}

struct Offer {
    id: String,
    path: PathBuf,
    name: String,
    size: u64,
}

#[derive(Default)]
struct State {
    /// laeuft der Server gerade (Port offen)
    running: bool,
    error: String,
    ip: String,
    port: u16,
    fp: String,
    devices: HashMap<String, Device>,
    incoming: Option<Incoming>,
    /// Antwort auf eine offene Frage (Annehmen = true)
    ask: Option<(String, mpsc::Sender<(bool, bool)>)>,
    web: Option<(String, Instant)>,
    offers: Vec<Offer>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);
static STOP: AtomicBool = AtomicBool::new(false);

// ---------- Hilfen ----------

/// Zufall aus dem Betriebssystem (BCryptGenRandom), als Hex
pub fn random_hex(bytes: usize) -> String {
    use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
    let mut b = vec![0u8; bytes];
    let ok = unsafe { BCryptGenRandom(None, &mut b, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.is_ok();
    if !ok {
        // nie erwartet; dann wenigstens nicht vorhersagbar fuer andere Prozesse
        use std::hash::{BuildHasher, Hasher};
        for chunk in b.chunks_mut(8) {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
            let v = h.finish().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Dateiname aus einer fremden Quelle: ohne Pfad, ohne verbotene Zeichen, keine reservierten Namen
pub fn safe_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let mut s: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect();
    s = s.trim().trim_end_matches(['.', ' ']).to_string();
    while s.starts_with('.') {
        s.remove(0);
    }
    if s.is_empty() {
        s = "Datei".into();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        s = format!("_{s}");
    }
    // hoechstens 150 Zeichen, Endung behalten
    if s.chars().count() > 150 {
        let (stem, ext) = match s.rsplit_once('.') {
            Some((a, b)) if b.len() <= 10 => (a.to_string(), format!(".{b}")),
            _ => (s.clone(), String::new()),
        };
        s = stem.chars().take(150 - ext.chars().count()).collect::<String>() + &ext;
    }
    s
}

/// Namen fest belegen (leere Datei anlegen), damit zwei gleichzeitige Sendungen nicht dieselbe Datei treffen
pub fn reserve(dir: &Path, name: &str) -> Option<PathBuf> {
    (0..50).find_map(|_| {
        let p = unique_path(dir, name);
        File::create_new(&p).ok().map(|_| p)
    })
}

/// "foto.jpg" gibt es schon -> "foto (2).jpg"
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((a, b)) => (a.to_string(), format!(".{b}")),
        None => (name.to_string(), String::new()),
    };
    (2..10_000)
        .map(|i| dir.join(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{stem} ({}){ext}", random_hex(3))))
}

pub fn fmt_size(b: u64) -> String {
    let f = b as f64;
    if f >= 1e9 {
        format!("{:.1} GB", f / 1e9).replace('.', ",")
    } else if f >= 1e6 {
        format!("{:.1} MB", f / 1e6).replace('.', ",")
    } else if f >= 1e3 {
        format!("{:.0} KB", f / 1e3)
    } else {
        format!("{b} B")
    }
}

fn kind_of(name: &str, mime: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|x| x.1.to_ascii_lowercase()).unwrap_or_default();
    if mime.starts_with("image/") || matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "heic" | "heif" | "gif" | "webp") {
        "Foto"
    } else if mime.starts_with("video/") || matches!(ext.as_str(), "mov" | "mp4" | "m4v") {
        "Video"
    } else {
        "Datei"
    }
}

/// "3 Fotos", "1 Video und 2 Dateien"
pub fn count_text(kinds: &[&str]) -> String {
    let mut parts = Vec::new();
    for (k, one, many) in [("Foto", "Foto", "Fotos"), ("Video", "Video", "Videos"), ("Datei", "Datei", "Dateien")] {
        let n = kinds.iter().filter(|x| **x == k).count();
        if n > 0 {
            parts.push(format!("{n} {}", if n == 1 { one } else { many }));
        }
    }
    match parts.len() {
        0 => "nichts".into(),
        1 => parts.remove(0),
        _ => {
            let last = parts.pop().unwrap();
            format!("{} und {last}", parts.join(", "))
        }
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn query<'a>(q: &'a str, key: &str) -> Option<&'a str> {
    q.split('&').find_map(|kv| kv.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------- Einstellungen ----------

fn enabled() -> bool {
    crate::settings_value("/share/on").and_then(|v| v.as_bool()).unwrap_or(false)
}

fn alias() -> String {
    let n = crate::settings_value("/share/name").and_then(|v| v.as_str().map(|s| s.trim().to_string())).unwrap_or_default();
    if !n.is_empty() {
        return n.chars().take(40).collect();
    }
    let pc = std::env::var("COMPUTERNAME").unwrap_or_default();
    if pc.is_empty() { "Notch".into() } else { format!("Notch · {pc}") }
}

fn trusted(fp: &str) -> bool {
    crate::settings_value("/share/trusted")
        .and_then(|v| v.as_array().cloned())
        .is_some_and(|a| a.iter().any(|t| t["fp"].as_str() == Some(fp)))
}

fn folder(app: &AppHandle) -> PathBuf {
    let custom = crate::settings_value("/share/folder").and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    if !custom.trim().is_empty() {
        return PathBuf::from(custom.trim());
    }
    app.path()
        .download_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("Notch")
}

/// Fingerabdruck (LocalSend ohne Verschluesselung: zufaellige Zeichenkette), bleibt ueber Neustarts gleich
fn fingerprint(app: &AppHandle) -> String {
    let file = app.path().app_data_dir().ok().map(|d| d.join("share-id.txt"));
    if let Some(f) = &file {
        if let Ok(s) = std::fs::read_to_string(f) {
            let s = s.trim().to_string();
            if s.len() >= 16 {
                return s;
            }
        }
    }
    let id = random_hex(16);
    if let Some(f) = file {
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, &id);
    }
    id
}

/// Nur Entwicklungs-Builds: NOTCH_SHARE_TEST=1 -> nur 127.0.0.1, keine Geraete-Suche (keine Firewall-Abfrage),
/// Adresse der Browser-Seite steht im Log. In ausgelieferten Versionen gibt es das nicht.
fn test_mode() -> bool {
    cfg!(debug_assertions) && std::env::var("NOTCH_SHARE_TEST").is_ok()
}

/// Adresse dieses PCs im lokalen Netz (die Route ins Internet; es wird nichts gesendet)
fn local_ip() -> Option<Ipv4Addr> {
    if test_mode() {
        return Some(Ipv4Addr::LOCALHOST);
    }
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(v) if !v.is_loopback() && !v.is_unspecified() => Some(v),
        _ => None,
    }
}

fn info(fp: &str, port: u16, announce: Option<bool>) -> Value {
    let mut v = json!({
        "alias": alias(),
        "version": "2.1",
        "deviceModel": "Windows",
        "deviceType": "desktop",
        "fingerprint": fp,
        "port": port,
        "protocol": "http",
        "download": false,
    });
    if let Some(a) = announce {
        v["announce"] = a.into();
    }
    v
}

// ---------- Aktivitaeten in der Notch ----------

fn act(id: &str, title: String, subtitle: String) -> Activity {
    Activity {
        id: id.into(),
        app: "iPhone".into(),
        title,
        subtitle: Some(subtitle),
        icon: Some(PHONE_ICON.into()),
        color: Some("#64d2ff".into()),
        priority: 6,
        ..Default::default()
    }
}

fn btn(id: &str, label: &str, icon: Option<&str>) -> Action {
    Action { id: id.into(), label: label.into(), icon: icon.map(String::from), ..Default::default() }
}

fn status_event(app: &AppHandle) {
    let _ = app.emit("share-state", status(app));
}

/// Aktionsknopf an einer Teilen-Zeile (Annehmen, Ablehnen, Ordner …)
pub fn action(app: &AppHandle, id: &str, action: &str) {
    match (id, action) {
        (ASK_ID, "accept" | "trust" | "decline") => {
            let ask = STATE.lock().unwrap().ask.take();
            if let Some((_, tx)) = ask {
                let _ = tx.send((action != "decline", action == "trust"));
            }
            activities::remove(app, ASK_ID);
        }
        (RECV_ID, "cancel") => {
            if let Some(i) = STATE.lock().unwrap().incoming.as_mut() {
                i.cancelled = true;
            }
        }
        (DONE_ID, "folder") => {
            let _ = crate::open::open_target(app, &folder(app).to_string_lossy());
        }
        (TEXT_ID, "copy") => {
            if let Some(t) = activities::get(TEXT_ID).and_then(|a| a.subtitle) {
                let full = LAST_TEXT.lock().unwrap().clone();
                let text = if full.is_empty() { t } else { full };
                let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(text));
                let mut a = act(TEXT_ID, "Kopiert".into(), "Text liegt in der Zwischenablage".into());
                a.ttl = Some(4);
                activities::upsert(app, a);
            }
        }
        (TEXT_ID, "open") => {
            let t = LAST_TEXT.lock().unwrap().clone();
            if let Some(url) = first_url(&t) {
                let _ = crate::open::open_target(app, &url);
            }
            activities::remove(app, TEXT_ID);
        }
        (SEND_ID, "cancel") => SEND_CANCEL.store(true, Ordering::Relaxed),
        _ => {
            if action == "dismiss" || action == "close" {
                activities::remove(app, id);
            }
        }
    }
}

static LAST_TEXT: Mutex<String> = Mutex::new(String::new());
static SEND_CANCEL: AtomicBool = AtomicBool::new(false);

pub fn first_url(t: &str) -> Option<String> {
    t.split_whitespace()
        .find(|w| w.starts_with("https://") || w.starts_with("http://"))
        .map(|w| w.trim_end_matches(['.', ',', ')', '"', '\'']).to_string())
}

/// "Immer annehmen": Geraet in die Einstellungen (share.trusted) schreiben
fn trust(app: &AppHandle, fp: &str, alias: &str) {
    crate::settings_update(app, |s| {
        if !s["share"].is_object() {
            s["share"] = json!({});
        }
        let list = s["share"]["trusted"].as_array().cloned().unwrap_or_default();
        if list.iter().any(|t| t["fp"].as_str() == Some(fp)) {
            return;
        }
        let mut list = list;
        list.push(json!({ "fp": fp, "alias": alias }));
        s["share"]["trusted"] = Value::Array(list);
    });
}

// ---------- Empfangen (LocalSend: prepare-upload / upload / cancel) ----------

fn json_resp(v: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(v.to_string()).with_header(header("Content-Type", "application/json; charset=utf-8"))
}

fn plain(code: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(msg).with_status_code(code)
}

fn read_body(req: &mut Request, max: u64) -> Option<String> {
    let mut body = String::new();
    req.as_reader().take(max).read_to_string(&mut body).ok()?;
    Some(body)
}

fn remember(dev: &Value, ip: IpAddr, fp_self: &str) -> Option<Device> {
    let fp = dev["fingerprint"].as_str().unwrap_or("").to_string();
    if fp.is_empty() || fp == fp_self {
        return None;
    }
    let d = Device {
        fp: fp.clone(),
        alias: dev["alias"].as_str().unwrap_or("Gerät").chars().take(40).collect(),
        model: dev["deviceModel"].as_str().unwrap_or("").chars().take(40).collect(),
        kind: dev["deviceType"].as_str().unwrap_or("").into(),
        ip,
        port: dev["port"].as_u64().map(|p| p as u16).unwrap_or(PORT),
        https: dev["protocol"].as_str() != Some("http"),
        seen: Some(Instant::now()),
    };
    STATE.lock().unwrap().devices.insert(fp, d.clone());
    Some(d)
}

fn prepare_upload(app: &AppHandle, req: &mut Request, ip: IpAddr) -> Response<std::io::Cursor<Vec<u8>>> {
    let Some(body) = read_body(req, 4 * 1024 * 1024) else { return plain(400, "Invalid body") };
    let Ok(v) = serde_json::from_str::<Value>(&body) else { return plain(400, "Invalid body") };
    let fp_self = STATE.lock().unwrap().fp.clone();
    let from_alias = v["info"]["alias"].as_str().unwrap_or("Ein Gerät").chars().take(40).collect::<String>();
    let from_fp = v["info"]["fingerprint"].as_str().unwrap_or("").to_string();
    let _ = remember(&v["info"], ip, &fp_self);
    let Some(files) = v["files"].as_object() else { return plain(400, "Invalid body") };
    if files.is_empty() || files.len() > MAX_FILES {
        return plain(400, "Invalid body");
    }

    // nur Text (LocalSend "Nachricht"): anzeigen, nichts uebertragen
    let texts: Vec<String> = files
        .values()
        .filter(|f| f["fileType"].as_str().is_some_and(|t| t.starts_with("text/")) && f["preview"].is_string())
        .map(|f| f["preview"].as_str().unwrap_or("").to_string())
        .collect();
    if texts.len() == files.len() {
        let text: String = texts.join("\n").chars().take(20_000).collect();
        *LAST_TEXT.lock().unwrap() = text.clone();
        let mut a = act(TEXT_ID, format!("Text von {from_alias}"), text.chars().take(240).collect());
        a.actions = vec![btn("copy", "Kopieren", Some("check"))];
        if first_url(&text).is_some() {
            a.actions.push(btn("open", "Öffnen", Some("open")));
        }
        a.ttl = Some(180);
        a.alert = true;
        activities::upsert(app, a);
        return plain(204, "");
    }

    {
        let st = STATE.lock().unwrap();
        if st.incoming.is_some() || st.ask.is_some() {
            return plain(409, "Blocked by another session");
        }
    }
    let mut list = HashMap::new();
    let mut total = 0u64;
    let mut kinds = Vec::new();
    let mut names = Vec::new();
    for (id, f) in files {
        let name = safe_name(f["fileName"].as_str().unwrap_or("Datei"));
        let size = f["size"].as_u64().unwrap_or(0);
        total = total.saturating_add(size);
        let kind = kind_of(&name, f["fileType"].as_str().unwrap_or(""));
        kinds.push(kind);
        names.push(name.clone());
        list.insert(id.clone(), InFile { name, size, token: random_hex(16), kind: kind.into(), done: None });
    }
    if total > MAX_TOTAL {
        return plain(403, "Rejected");
    }

    // nachfragen (ausser bei vertrauenswuerdigen Geraeten)
    let what = count_text(&kinds);
    let sid = random_hex(12);
    let accepted = if !from_fp.is_empty() && trusted(&from_fp) {
        true
    } else {
        let (tx, rx) = mpsc::channel();
        STATE.lock().unwrap().ask = Some((sid.clone(), tx));
        let mut shown = names.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
        if names.len() > 3 {
            shown += &format!(" + {}", names.len() - 3);
        }
        let mut a = act(ASK_ID, format!("{from_alias} möchte {what} senden"), format!("{shown} · {}", fmt_size(total)));
        a.actions = vec![btn("accept", "Annehmen", None), btn("decline", "Ablehnen", None), btn("trust", "Immer annehmen", None)];
        a.alert = true;
        a.priority = 20;
        a.ttl = Some(ASK_FOR.as_secs());
        activities::upsert(app, a);
        let answer = rx.recv_timeout(ASK_FOR).ok();
        {
            let mut st = STATE.lock().unwrap();
            if st.ask.as_ref().is_some_and(|(id, _)| *id == sid) {
                st.ask = None;
            }
        }
        activities::remove(app, ASK_ID);
        match answer {
            Some((ok, always)) => {
                if ok && always && !from_fp.is_empty() {
                    trust(app, &from_fp, &from_alias);
                }
                ok
            }
            None => false,
        }
    };
    if !accepted {
        return plain(403, "Rejected");
    }
    let tokens: serde_json::Map<String, Value> = list.iter().map(|(id, f)| (id.clone(), Value::String(f.token.clone()))).collect();
    STATE.lock().unwrap().incoming = Some(Incoming { id: sid.clone(), alias: from_alias.clone(), ip, files: list, total, got: 0, cancelled: false });
    progress(app);
    json_resp(json!({ "sessionId": sid, "files": tokens }))
}

/// Fortschritt der laufenden Sendung als Zeile in der Notch
fn progress(app: &AppHandle) {
    let (title, sub, p) = {
        let st = STATE.lock().unwrap();
        let Some(i) = &st.incoming else { return };
        let done = i.files.values().filter(|f| f.done.is_some()).count();
        (
            format!("Empfange von {}", i.alias),
            format!("{} / {} · {done} von {}", fmt_size(i.got), fmt_size(i.total), i.files.len()),
            if i.total > 0 { (i.got as f64 / i.total as f64).min(1.0) } else { -1.0 },
        )
    };
    let mut a = act(RECV_ID, title, sub);
    a.progress = Some(p);
    a.value = Some(if p >= 0.0 { format!("{}", (p * 100.0).round() as u32) } else { "…".into() });
    a.unit = Some("%".into());
    a.actions = vec![btn("cancel", "Abbrechen", Some("close"))];
    a.ttl = Some(120);
    activities::upsert(app, a);
}

fn upload(app: &AppHandle, req: &mut Request, ip: IpAddr, q: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let (Some(sid), Some(fid), Some(tok)) = (query(q, "sessionId"), query(q, "fileId"), query(q, "token")) else {
        return plain(400, "Missing parameters");
    };
    let (sid, fid, tok) = (url_decode(sid), url_decode(fid), url_decode(tok));
    let (name, size) = {
        let st = STATE.lock().unwrap();
        let Some(i) = st.incoming.as_ref().filter(|i| i.id == sid) else { return plain(409, "Blocked by another session") };
        if i.ip != ip {
            return plain(403, "Invalid token or IP address");
        }
        let Some(f) = i.files.get(&fid).filter(|f| f.token == tok && f.done.is_none()) else {
            return plain(403, "Invalid token or IP address");
        };
        (f.name.clone(), f.size)
    };
    let dir = folder(app);
    if std::fs::create_dir_all(&dir).is_err() {
        return plain(500, "Zielordner fehlt");
    }
    let Some(target) = reserve(&dir, &name) else { return plain(500, "Datei nicht schreibbar") };
    let part = target.with_extension(format!("{}notchpart", target.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()));
    let Ok(mut out) = File::create(&part) else {
        let _ = std::fs::remove_file(&target);
        return plain(500, "Datei nicht schreibbar");
    };
    let mut buf = vec![0u8; 256 * 1024];
    let mut written = 0u64;
    let mut last_emit = Instant::now();
    let reader = req.as_reader();
    let ok = loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break true,
            Ok(n) => n,
            Err(_) => break false,
        };
        written += n as u64;
        // nie mehr annehmen als angekuendigt (+ etwas Luft fuer ungenaue Groessen)
        if size > 0 && written > size + 1024 * 1024 {
            break false;
        }
        if out.write_all(&buf[..n]).is_err() {
            break false;
        }
        let cancelled = {
            let mut st = STATE.lock().unwrap();
            match st.incoming.as_mut().filter(|i| i.id == sid) {
                Some(i) => {
                    i.got += n as u64;
                    i.cancelled
                }
                None => true,
            }
        };
        if cancelled {
            break false;
        }
        if last_emit.elapsed() > Duration::from_millis(250) {
            last_emit = Instant::now();
            progress(app);
        }
    };
    drop(out);
    if !ok || std::fs::rename(&part, &target).is_err() {
        let _ = std::fs::remove_file(&part);
        let _ = std::fs::remove_file(&target);
        let cancelled = STATE.lock().unwrap().incoming.as_ref().is_none_or(|i| i.cancelled);
        if cancelled {
            finish(app, true);
        }
        return plain(500, "Übertragung abgebrochen");
    }
    let all_done = {
        let mut st = STATE.lock().unwrap();
        match st.incoming.as_mut().filter(|i| i.id == sid) {
            Some(i) => {
                if let Some(f) = i.files.get_mut(&fid) {
                    f.done = Some(target.clone());
                }
                i.files.values().all(|f| f.done.is_some())
            }
            None => false,
        }
    };
    if all_done {
        finish(app, false);
    } else {
        progress(app);
    }
    Response::from_string("")
}

/// Sendung fertig (oder abgebrochen): Ablage fuellen, Notch zeigt es kurz
fn finish(app: &AppHandle, cancelled: bool) {
    let Some(i) = STATE.lock().unwrap().incoming.take() else { return };
    activities::remove(app, RECV_ID);
    let paths: Vec<String> = i.files.values().filter_map(|f| f.done.as_ref().map(|p| p.to_string_lossy().to_string())).collect();
    let kinds: Vec<&str> = i.files.values().filter(|f| f.done.is_some()).map(|f| f.kind.as_str()).collect();
    if !paths.is_empty() {
        crate::shelf::shelf_add(app.clone(), paths.clone());
    }
    let title = if cancelled {
        format!("Abgebrochen · {} angekommen", count_text(&kinds))
    } else {
        format!("{} von {}", count_text(&kinds), i.alias)
    };
    let mut a = act(DONE_ID, title, format!("In der Ablage und in {}", folder(app).to_string_lossy()));
    a.actions = vec![btn("folder", "Ordner", Some("folder"))];
    a.ttl = Some(20);
    a.alert = !cancelled;
    activities::upsert(app, a);
    let _ = app.emit("share-received", json!({ "from": i.alias, "files": paths, "cancelled": cancelled }));
}

// ---------- Browser-Seite (ohne App) ----------

const PAGE: &str = include_str!("share-page.html");

fn web_ok(tok: &str) -> bool {
    let mut st = STATE.lock().unwrap();
    match &mut st.web {
        Some((t, until)) if t == tok && *until > Instant::now() => {
            // jede Nutzung verlaengert ein wenig
            *until = (*until).max(Instant::now() + Duration::from_secs(10 * 60));
            true
        }
        _ => false,
    }
}

fn web(app: &AppHandle, req: &mut Request, path: &str, q: &str) -> Option<Response<Box<dyn Read + Send>>> {
    let rest = path.strip_prefix("/w/")?;
    let (tok, sub) = rest.split_once('/').unwrap_or((rest, ""));
    let boxed = |r: Response<std::io::Cursor<Vec<u8>>>| r.boxed();
    if !web_ok(tok) {
        return Some(boxed(
            Response::from_string("<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'><body style='font:17px system-ui;background:#000;color:#fff;padding:40px 24px'>Dieser QR-Code ist abgelaufen. Bitte in der Notch neu anzeigen lassen.")
                .with_status_code(403)
                .with_header(header("Content-Type", "text/html; charset=utf-8")),
        ));
    }
    let resp = match (req.method(), sub) {
        (Method::Get, "") => Response::from_string(PAGE.replace("{{NAME}}", &html_escape(&alias())))
            .with_header(header("Content-Type", "text/html; charset=utf-8"))
            .with_header(header("Cache-Control", "no-store")),
        (Method::Get, "list") => {
            let st = STATE.lock().unwrap();
            let offers: Vec<Value> = st
                .offers
                .iter()
                .map(|o| json!({ "id": o.id, "name": o.name, "size": o.size, "kind": kind_of(&o.name, "") }))
                .collect();
            json_resp(json!({ "name": alias(), "offers": offers }))
        }
        (Method::Put, "up") => {
            let name = safe_name(&url_decode(query(q, "name").unwrap_or("Datei")));
            let size: u64 = query(q, "size").and_then(|s| s.parse().ok()).unwrap_or(0);
            if size > MAX_TOTAL {
                return Some(boxed(plain(413, "zu groß")));
            }
            let dir = folder(app);
            let _ = std::fs::create_dir_all(&dir);
            let Some(target) = reserve(&dir, &name) else { return Some(boxed(plain(500, "konnte nicht speichern"))) };
            let ok = File::create(&target)
                .ok()
                .and_then(|mut f| std::io::copy(&mut req.as_reader().take(MAX_TOTAL), &mut f).ok())
                .is_some();
            if !ok {
                let _ = std::fs::remove_file(&target);
                return Some(boxed(plain(500, "konnte nicht speichern")));
            }
            let p = target.to_string_lossy().to_string();
            crate::shelf::shelf_add(app.clone(), vec![p.clone()]);
            let mut a = act(DONE_ID, format!("{} vom iPhone (Browser)", kind_of(&name, "")), name.clone());
            a.actions = vec![btn("folder", "Ordner", Some("folder"))];
            a.ttl = Some(15);
            a.alert = true;
            activities::upsert(app, a);
            json_resp(json!({ "ok": true, "name": target.file_name().map(|n| n.to_string_lossy().to_string()) }))
        }
        (Method::Get, s) if s.starts_with("f/") => {
            let id = &s[2..];
            let found = STATE.lock().unwrap().offers.iter().find(|o| o.id == id).map(|o| (o.path.clone(), o.name.clone()));
            let Some((path, name)) = found else { return Some(boxed(plain(404, "nicht gefunden"))) };
            let Ok(f) = File::open(&path) else { return Some(boxed(plain(404, "nicht gefunden"))) };
            let inline = query(q, "view").is_some();
            let disp = format!("{}; filename*=UTF-8''{}", if inline { "inline" } else { "attachment" }, pct(&name));
            return Some(
                Response::from_file(f)
                    .with_header(header("Content-Type", mime_of(&name)))
                    .with_header(header("Content-Disposition", &disp))
                    .boxed(),
            );
        }
        _ => plain(404, "nicht gefunden"),
    };
    Some(resp.boxed())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn mime_of(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|x| x.1.to_ascii_lowercase()).as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        Some("pdf") => "application/pdf",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("mp3") => "audio/mpeg",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

// ---------- Server ----------

fn handle(app: AppHandle, mut req: Request) {
    let ip = req.remote_addr().map(|a| a.ip()).unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let full = req.url().to_string();
    let (path, q) = full.split_once('?').unwrap_or((full.as_str(), ""));
    let (path, q) = (path.to_string(), q.to_string());
    if path.starts_with("/w/") {
        if let Some(r) = web(&app, &mut req, &path, &q) {
            let _ = req.respond(r);
        }
        return;
    }
    let (fp, port) = {
        let st = STATE.lock().unwrap();
        (st.fp.clone(), st.port)
    };
    let resp = match (req.method().clone(), path.as_str()) {
        (Method::Get, "/api/localsend/v2/info" | "/api/localsend/v1/info") => json_resp(info(&fp, port, None)),
        (Method::Post, "/api/localsend/v2/register") => {
            if let Some(v) = read_body(&mut req, 64 * 1024).and_then(|b| serde_json::from_str::<Value>(&b).ok()) {
                if remember(&v, ip, &fp).is_some() {
                    status_event(&app);
                }
            }
            json_resp(info(&fp, port, None))
        }
        (Method::Post, "/api/localsend/v2/prepare-upload") => prepare_upload(&app, &mut req, ip),
        (Method::Post, "/api/localsend/v2/upload") => upload(&app, &mut req, ip, &q),
        (Method::Post, "/api/localsend/v2/cancel") => {
            let sid = url_decode(query(&q, "sessionId").unwrap_or(""));
            let hit = {
                let mut st = STATE.lock().unwrap();
                match st.incoming.as_mut().filter(|i| i.id == sid && i.ip == ip) {
                    Some(i) => {
                        i.cancelled = true;
                        true
                    }
                    None => false,
                }
            };
            if hit {
                finish(&app, true);
            }
            Response::from_string("")
        }
        (Method::Get, "/") => Response::from_string("Notch").with_status_code(200),
        _ => plain(404, "not found"),
    };
    let _ = req.respond(resp);
}

/// Multicast: ankuendigen und auf andere LocalSend-Geraete hoeren
fn discovery(app: AppHandle, sock: UdpSocket, fp: String, port: u16) {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
    let target = SocketAddr::V4(SocketAddrV4::new(GROUP, PORT));
    let announce = |announce: bool| {
        let _ = sock.send_to(info(&fp, port, Some(announce)).to_string().as_bytes(), target);
    };
    announce(true);
    let mut last = Instant::now();
    let mut buf = vec![0u8; 8192];
    while !STOP.load(Ordering::Relaxed) {
        if last.elapsed() > Duration::from_secs(45) {
            last = Instant::now();
            announce(true);
        }
        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        let Ok(v) = serde_json::from_slice::<Value>(&buf[..n]) else { continue };
        let Some(dev) = remember(&v, from.ip(), &fp) else { continue };
        status_event(&app);
        if v["announce"].as_bool() == Some(true) {
            // antworten: per HTTP an das Geraet (LocalSend bevorzugt das), sonst per Multicast
            let me = info(&fp, port, None);
            let ok = register_at(&dev, &me);
            if !ok {
                announce(false);
            }
        }
    }
}

fn client(timeout: Option<Duration>) -> ureq::Agent {
    // LocalSend-Geraete mit Verschluesselung haben selbst ausgestellte Zertifikate: Pruefung aus (nur im LAN)
    ureq::Agent::config_builder()
        .timeout_global(timeout)
        .timeout_connect(Some(Duration::from_secs(4)))
        .http_status_as_error(false)
        .tls_config(ureq::tls::TlsConfig::builder().disable_verification(true).build())
        .build()
        .into()
}

fn base(d: &Device) -> String {
    format!("{}://{}:{}/api/localsend/v2", if d.https { "https" } else { "http" }, d.ip, d.port)
}

fn register_at(d: &Device, me: &Value) -> bool {
    client(Some(Duration::from_secs(4)))
        .post(&format!("{}/register", base(d)))
        .send_json(me)
        .is_ok_and(|r| r.status().is_success())
}

fn start(app: &AppHandle) -> Result<(Arc<Server>, u16), String> {
    // TCP: 53317 wie LocalSend; belegt (z. B. LocalSend fuer Windows laeuft) -> naechster freier
    let mut last_err = String::new();
    for port in [PORT, PORT + 1, PORT + 2, PORT + 3] {
        match Server::http((if test_mode() { "127.0.0.1" } else { "0.0.0.0" }, port)) {
            Ok(s) => {
                let s = Arc::new(s);
                let fp = fingerprint(app);
                {
                    let mut st = STATE.lock().unwrap();
                    st.fp = fp.clone();
                    st.port = port;
                    st.running = true;
                    st.error.clear();
                    st.ip = local_ip().map(|i| i.to_string()).unwrap_or_default();
                }
                let srv = s.clone();
                let app2 = app.clone();
                std::thread::spawn(move || {
                    for req in srv.incoming_requests() {
                        let app = app2.clone();
                        std::thread::spawn(move || handle(app, req));
                    }
                });
                if test_mode() {
                    let tok = random_hex(16);
                    STATE.lock().unwrap().web = Some((tok.clone(), Instant::now() + WEB_FOR));
                    eprintln!("[notch] share test: http://127.0.0.1:{port}/w/{tok}/");
                    return Ok((s, port));
                }
                match multicast_socket() {
                    Ok(sock) => {
                        let app3 = app.clone();
                        std::thread::spawn(move || discovery(app3, sock, fp, port));
                    }
                    Err(e) => {
                        STATE.lock().unwrap().error = format!("Geräte-Suche nicht möglich ({e}) – Senden per QR-Code geht trotzdem.");
                    }
                }
                return Ok((s, port));
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(format!("Port {PORT} belegt: {last_err}"))
}

fn multicast_socket() -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?; // LocalSend fuer Windows darf parallel lauschen
    s.bind(&SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT)).into())?;
    let iface = local_ip().unwrap_or(Ipv4Addr::UNSPECIFIED);
    s.join_multicast_v4(&GROUP, &iface)?;
    if iface != Ipv4Addr::UNSPECIFIED {
        let _ = s.set_multicast_if_v4(&iface);
    }
    s.set_multicast_loop_v4(false)?;
    s.set_multicast_ttl_v4(4)?;
    Ok(s.into())
}

/// Ein- und Ausschalten folgt der Einstellung (alle 2 s nachsehen)
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut server: Option<Arc<Server>> = None;
        let mut failed_at: Option<Instant> = None;
        loop {
            let want = enabled();
            if want && server.is_none() && failed_at.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) {
                STOP.store(false, Ordering::Relaxed);
                match start(&app) {
                    Ok((s, _)) => {
                        server = Some(s);
                        failed_at = None;
                    }
                    Err(e) => {
                        failed_at = Some(Instant::now());
                        let mut st = STATE.lock().unwrap();
                        st.running = false;
                        st.error = e;
                    }
                }
                status_event(&app);
            } else if !want && server.is_some() {
                STOP.store(true, Ordering::Relaxed);
                if let Some(s) = server.take() {
                    s.unblock();
                }
                {
                    let mut st = STATE.lock().unwrap();
                    st.running = false;
                    st.devices.clear();
                    st.web = None;
                    st.offers.clear();
                    if let Some((_, tx)) = st.ask.take() {
                        let _ = tx.send((false, false));
                    }
                }
                status_event(&app);
            }
            // Geraete, die sich lange nicht gemeldet haben, vergessen; Adresse kann wechseln (WLAN)
            {
                let mut st = STATE.lock().unwrap();
                let before = st.devices.len();
                st.devices.retain(|_, d| d.seen.is_some_and(|s| s.elapsed() < Duration::from_secs(10 * 60)));
                let changed = st.devices.len() != before;
                if st.running {
                    if let Some(ip) = local_ip() {
                        st.ip = ip.to_string();
                    }
                }
                drop(st);
                if changed {
                    status_event(&app);
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

// ---------- Befehle ----------

fn status(app: &AppHandle) -> Value {
    let folder = folder(app).to_string_lossy().to_string();
    let st = STATE.lock().unwrap();
    let mut devices: Vec<&Device> = st.devices.values().collect();
    devices.sort_by_key(|d| d.seen.map(|s| s.elapsed()).unwrap_or(Duration::MAX));
    json!({
        "on": enabled(),
        "running": st.running,
        "error": st.error,
        "ip": st.ip,
        "port": st.port,
        "alias": alias(),
        "folder": folder,
        "devices": devices.iter().map(|d| json!({ "fp": d.fp, "alias": d.alias, "model": d.model, "kind": d.kind, "recent": d.seen.is_some_and(|s| s.elapsed() < Duration::from_secs(120)) })).collect::<Vec<_>>(),
        "offers": st.offers.iter().map(|o| json!({ "id": o.id, "name": o.name, "size": o.size })).collect::<Vec<_>>(),
    })
}

#[tauri::command]
pub fn share_status(app: AppHandle) -> Value {
    status(&app)
}

fn qr_svg(text: &str) -> Result<String, String> {
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M).map_err(|e| e.to_string())?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(180, 180)
        .quiet_zone(true)
        .dark_color(qrcode::render::svg::Color("#000000"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build())
}

/// QR-Code fuer die Browser-Seite (neuer Schluessel, 30 Min gueltig)
#[tauri::command]
pub fn share_qr() -> Result<Value, String> {
    let (ip, port, running) = {
        let st = STATE.lock().unwrap();
        (st.ip.clone(), st.port, st.running)
    };
    if !running {
        return Err("iPhone-Austausch ist aus (Einstellungen › iPhone).".into());
    }
    if ip.is_empty() {
        return Err("Kein lokales Netzwerk gefunden.".into());
    }
    let tok = {
        let mut st = STATE.lock().unwrap();
        let reuse = st.web.as_ref().filter(|(_, until)| *until > Instant::now() + Duration::from_secs(5 * 60)).map(|(t, _)| t.clone());
        let t = reuse.unwrap_or_else(|| random_hex(16));
        st.web = Some((t.clone(), Instant::now() + WEB_FOR));
        t
    };
    let url = format!("http://{ip}:{port}/w/{tok}/");
    Ok(json!({ "url": url, "svg": qr_svg(&url)?, "minutes": WEB_FOR.as_secs() / 60 }))
}

/// Dateien auf der Browser-Seite zum Herunterladen anbieten (ersetzt die bisherige Auswahl)
#[tauri::command]
pub fn share_offer(paths: Vec<String>) -> Result<Value, String> {
    let offers: Vec<Offer> = paths
        .iter()
        .filter_map(|p| {
            let path = PathBuf::from(p);
            let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
            Some(Offer {
                id: random_hex(8),
                name: path.file_name()?.to_string_lossy().to_string(),
                path,
                size: meta.len(),
            })
        })
        .collect();
    if offers.is_empty() {
        return Err("Nur Dateien lassen sich anbieten (keine Ordner).".into());
    }
    STATE.lock().unwrap().offers = offers;
    share_qr()
}

/// Per LocalSend an ein Geraet senden (das iPhone muss die App offen haben und bestaetigen)
#[tauri::command]
pub fn share_send(app: AppHandle, fp: String, paths: Vec<String>) -> Result<(), String> {
    let dev = STATE.lock().unwrap().devices.get(&fp).cloned().ok_or("Gerät nicht mehr da – LocalSend auf dem iPhone öffnen.")?;
    let files: Vec<(String, PathBuf, u64)> = paths
        .iter()
        .filter_map(|p| {
            let path = PathBuf::from(p);
            let m = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
            Some((random_hex(8), path, m.len()))
        })
        .collect();
    if files.is_empty() {
        return Err("Nur Dateien lassen sich senden (keine Ordner).".into());
    }
    let (fp_self, port) = {
        let st = STATE.lock().unwrap();
        (st.fp.clone(), st.port)
    };
    SEND_CANCEL.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        let total: u64 = files.iter().map(|f| f.2).sum();
        let kinds: Vec<&str> = files.iter().map(|f| kind_of(&f.1.to_string_lossy(), "")).collect();
        let what = count_text(&kinds);
        let show = |title: String, sub: String, p: Option<f64>, done: bool| {
            let mut a = act(SEND_ID, title, sub);
            a.progress = p;
            if !done {
                a.actions = vec![btn("cancel", "Abbrechen", Some("close"))];
            }
            a.ttl = Some(if done { 12 } else { 150 });
            a.alert = done;
            activities::upsert(&app, a);
        };
        show(format!("Warte auf {}", dev.alias), format!("{what} · {} – bitte auf dem iPhone annehmen", fmt_size(total)), Some(-1.0), false);
        let meta: serde_json::Map<String, Value> = files
            .iter()
            .map(|(id, p, size)| {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                (id.clone(), json!({ "id": id, "fileName": name, "size": size, "fileType": mime_of(&name) }))
            })
            .collect();
        let body = json!({ "info": info(&fp_self, port, None), "files": meta });
        let r = client(Some(Duration::from_secs(130))).post(&format!("{}/prepare-upload", base(&dev))).send_json(&body);
        let mut resp = match r {
            Ok(r) => r,
            Err(e) => return show("Senden fehlgeschlagen".into(), format!("{} nicht erreichbar: {e}", dev.alias), None, true),
        };
        let code = resp.status().as_u16();
        if code == 204 {
            return show(format!("An {} gesendet", dev.alias), what, Some(1.0), true);
        }
        if code != 200 {
            let why = match code {
                403 => "abgelehnt",
                409 => "ist gerade mit etwas anderem beschäftigt",
                401 => "verlangt eine PIN (in LocalSend ausschalten)",
                _ => "hat nicht angenommen",
            };
            return show("Nicht gesendet".into(), format!("{} {why}", dev.alias), None, true);
        }
        let v: Value = resp.body_mut().read_json().unwrap_or(Value::Null);
        let sid = v["sessionId"].as_str().unwrap_or("").to_string();
        let mut sent = 0u64;
        let mut count = 0usize;
        for (id, path, size) in &files {
            let Some(tok) = v["files"][id].as_str() else { continue }; // vom iPhone abgewaehlt
            let Ok(f) = File::open(path) else { continue };
            let base_sent = sent;
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let mut last = Instant::now();
            let mut counted = Counting {
                inner: f,
                n: 0,
                on: Box::new(|n| {
                    if last.elapsed() > Duration::from_millis(300) {
                        last = Instant::now();
                        let p = if total > 0 { (base_sent + n) as f64 / total as f64 } else { -1.0 };
                        show(format!("Sende an {}", dev.alias), format!("{name} · {} / {}", fmt_size(base_sent + n), fmt_size(total)), Some(p), false);
                    }
                    !SEND_CANCEL.load(Ordering::Relaxed)
                }),
            };
            let url = format!("{}/upload?sessionId={}&fileId={}&token={}", base(&dev), pct(&sid), pct(id), pct(tok));
            let ok = client(None)
                .post(&url)
                .header("Content-Length", &size.to_string())
                .header("Content-Type", "application/octet-stream")
                .send(ureq::SendBody::from_reader(&mut counted))
                .is_ok_and(|r| r.status().is_success());
            if SEND_CANCEL.load(Ordering::Relaxed) {
                let _ = client(Some(Duration::from_secs(4))).post(&format!("{}/cancel?sessionId={}", base(&dev), pct(&sid))).send_empty();
                return show("Senden abgebrochen".into(), format!("{count} von {} angekommen", files.len()), None, true);
            }
            if !ok {
                return show("Senden fehlgeschlagen".into(), format!("{name} kam nicht an"), None, true);
            }
            sent += size;
            count += 1;
        }
        show(format!("An {} gesendet", dev.alias), format!("{} · {}", count_text(&kinds), fmt_size(sent)), Some(1.0), true);
    });
    Ok(())
}

/// Zaehlt gelesene Bytes; `on` liefert false = abbrechen
struct Counting<'a> {
    inner: File,
    n: u64,
    on: Box<dyn FnMut(u64) -> bool + 'a>,
}

impl Read for Counting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !(self.on)(self.n) {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "abgebrochen"));
        }
        let k = self.inner.read(buf)?;
        self.n += k as u64;
        Ok(k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dateinamen_sicher() {
        assert_eq!(safe_name("IMG_0042.HEIC"), "IMG_0042.HEIC");
        assert_eq!(safe_name("../../Windows/system32/evil.dll"), "evil.dll");
        assert_eq!(safe_name("C:\\Users\\x\\a.txt"), "a.txt");
        assert_eq!(safe_name("con.txt"), "_con.txt");
        assert_eq!(safe_name("COM1"), "_COM1");
        assert_eq!(safe_name("a<b>c:d|e?f*.png"), "abcdef.png");
        assert_eq!(safe_name("..."), "Datei");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name("ende. "), "ende");
        let long = format!("{}.jpg", "x".repeat(400));
        let s = safe_name(&long);
        assert_eq!(s.chars().count(), 150);
        assert!(s.ends_with(".jpg"));
    }

    #[test]
    fn eindeutige_namen() {
        let dir = std::env::temp_dir().join(format!("notch-share-{}", random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "a.jpg"), dir.join("a.jpg"));
        std::fs::write(dir.join("a.jpg"), b"1").unwrap();
        assert_eq!(unique_path(&dir, "a.jpg"), dir.join("a (2).jpg"));
        std::fs::write(dir.join("a (2).jpg"), b"1").unwrap();
        assert_eq!(unique_path(&dir, "a.jpg"), dir.join("a (3).jpg"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn texte() {
        assert_eq!(count_text(&["Foto", "Foto", "Foto"]), "3 Fotos");
        assert_eq!(count_text(&["Video", "Datei", "Datei"]), "1 Video und 2 Dateien");
        assert_eq!(count_text(&["Foto", "Video", "Datei"]), "1 Foto, 1 Video und 1 Datei");
        assert_eq!(fmt_size(12_400_000), "12,4 MB");
        assert_eq!(fmt_size(900), "900 B");
        assert_eq!(kind_of("IMG_1.HEIC", ""), "Foto");
        assert_eq!(kind_of("clip.mov", ""), "Video");
        assert_eq!(first_url("schau mal https://example.com/a?b=1."), Some("https://example.com/a?b=1".into()));
        assert_eq!(first_url("kein link"), None);
    }

    #[test]
    fn adressen_und_zufall() {
        assert_eq!(url_decode("Foto%20vom%20Urlaub.jpg"), "Foto vom Urlaub.jpg");
        assert_eq!(url_decode("%C3%BCber+alles"), "über alles");
        assert_eq!(url_decode("kaputt%2"), "kaputt%2");
        assert_eq!(query("sessionId=a&fileId=b&token=c", "fileId"), Some("b"));
        assert_eq!(query("sessionIdx=a", "sessionId"), None);
        let a = random_hex(16);
        assert_eq!(a.len(), 32);
        assert_ne!(a, random_hex(16));
        assert_eq!(pct("ä b.jpg"), "%C3%A4%20b.jpg");
    }
}
