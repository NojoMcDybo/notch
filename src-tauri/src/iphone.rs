//! iPhone per Kurzbefehl: Teilen-Menue, Zwischenablage und Verbindungsanzeige, ohne App auf dem iPhone.
//!
//! Ein Kurzbefehl „Notch“ (Anleitung auf der Einrichtungsseite `/k/<schluessel>/`) schickt alles an den
//! Server aus share.rs:
//! - `POST /k/<s>/senden?name=…` aus dem Teilen-Menue: Dateien -> Downloads\Notch + Ablage, ein Link oeffnet
//!   sich im Browser, anderer Text landet in der Zwischenablage.
//! - `POST /k/<s>/clip` ohne Eingabe (Rueckseite tippen): Zwischenablage des iPhones -> Zwischenablage am PC.
//! - `GET /k/<s>/hallo?akku=64` (Automation „WLAN verbunden“ / „Ladegeraet“): meldet sich und den Akku.
//! Jede Anfrage darf `geraet` (Geraetename) und `akku` (0..1 oder 0..100) mitschicken; `name` ist der Dateiname.
//!
//! Ob das iPhone im WLAN ist, sieht die Notch ohne App: Sie loest seine Adresse per ARP neu auf
//! (ResolveIpNetEntry2, das WLAN-Modul antwortet auch, wenn das iPhone gesperrt ist). Erst nach drei
//! Fehlversuchen hintereinander gilt es als weg (Energiesparen im WLAN verschluckt einzelne Anfragen).
//!
//! Sicherheit: ein fester Schluessel (128 Bit, `iphone-token.txt` im App-Ordner), „Neu koppeln“ macht den
//! alten ungueltig. Gekoppelte Kurzbefehle muessen nicht bestaetigt werden. Dateinamen wie in share.rs bereinigt.

use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tiny_http::{Method, Request, Response};

use crate::activities;
use crate::share::{self, count_text, fmt_size, header, json_resp, plain, query, safe_name, url_decode};

/// dauerhafte Karte in der aufgeklappten Notch (nie in der kleinen, siehe main.ts currentPlan)
pub const CARD_ID: &str = "notch:iphone";
/// kurz in der kleinen Notch, wenn das iPhone ins WLAN kommt
const HELLO_ID: &str = "notch:iphone:hallo";
const CLIP_ID: &str = "notch:iphone:clip";

/// bis hierhin gilt ein Text als Text (darueber: Datei)
const TEXT_MAX: usize = 256 * 1024;
const MAX_FILE: u64 = 50 * 1024 * 1024 * 1024;
/// Dateien, die so kurz nacheinander kommen, zaehlen als eine Sendung („3 Fotos vom iPhone“)
const BATCH_GAP: Duration = Duration::from_secs(8);
const PROBE_EVERY: Duration = Duration::from_secs(20);
const MISSES_UNTIL_GONE: u32 = 3;
/// aelterer Akkustand wird nicht mehr gezeigt
const BATTERY_FOR: Duration = Duration::from_secs(6 * 3600);

const PAGE: &str = include_str!("iphone-page.html");
const SHORTCUT: &[u8] = include_bytes!("Notch.shortcut");

#[derive(Default, Clone, Serialize, Deserialize, PartialEq)]
struct Phone {
    name: String,
    ip: String,
}

#[derive(Default)]
struct Live {
    phone: Option<Phone>,
    present: bool,
    misses: u32,
    battery: Option<(u8, Instant)>,
    charging: Option<bool>,
    seen: Option<Instant>,
    batch: Option<(Vec<&'static str>, Instant)>,
}

static LIVE: LazyLock<Mutex<Live>> = LazyLock::new(Default::default);
static TOKEN: Mutex<String> = Mutex::new(String::new());

// ---------- Schluessel und gemerktes Geraet ----------

fn data_file(app: &AppHandle, name: &str) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(name))
}

fn token(app: &AppHandle) -> String {
    let mut t = TOKEN.lock().unwrap();
    if t.is_empty() {
        let f = data_file(app, "iphone-token.txt");
        let saved = f.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).map(|s| s.trim().to_string());
        *t = match saved.filter(|s| s.len() >= 32 && s.bytes().all(|b| b.is_ascii_hexdigit())) {
            Some(s) => s,
            None => new_token(app),
        };
    }
    t.clone()
}

fn new_token(app: &AppHandle) -> String {
    let t = share::random_hex(16);
    if let Some(f) = data_file(app, "iphone-token.txt") {
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, &t);
    }
    t
}

/// Vergleich ohne frueh abzubrechen (Laufzeit verraet nichts ueber den Schluessel)
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn load_phone(app: &AppHandle) {
    let mut l = LIVE.lock().unwrap();
    if l.phone.is_none() {
        l.phone = data_file(app, "iphone.json")
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|s| serde_json::from_str(&s).ok());
    }
}

fn save_phone(app: &AppHandle, p: &Phone) {
    if let Some(f) = data_file(app, "iphone.json") {
        let _ = std::fs::write(f, serde_json::to_string(p).unwrap_or_default());
    }
}

// ---------- Anzeige ----------

fn ago(t: Instant) -> String {
    let m = t.elapsed().as_secs() / 60;
    match m {
        0 => "gerade eben".into(),
        1..=59 => format!("vor {m} Min"),
        60..=1439 => format!("vor {} Std", m / 60),
        _ => format!("vor {} Tagen", m / 1440),
    }
}

/// Karte „iPhone“ (aufgeklappt): Name, Akku, im WLAN oder zuletzt gesehen. Schickt nur bei Aenderung.
fn card(app: &AppHandle) {
    let a = {
        let l = LIVE.lock().unwrap();
        let Some(p) = l.phone.clone().filter(|_| share::enabled()) else {
            drop(l);
            activities::remove(app, CARD_ID);
            return;
        };
        let sub = if l.present {
            if l.charging == Some(true) { "Im WLAN · lädt".to_string() } else { "Im WLAN".to_string() }
        } else {
            match l.seen {
                Some(s) => format!("Nicht im WLAN · zuletzt {}", ago(s)),
                None => "Nicht im WLAN".into(),
            }
        };
        let mut a = share::act(CARD_ID, p.name, sub);
        if let Some((b, _)) = l.battery.filter(|(_, at)| at.elapsed() < BATTERY_FOR) {
            a.value = Some(b.to_string());
            a.unit = Some("%".into());
        }
        a.color = Some(if l.present { "#64d2ff" } else { "#8e8e93" }.into());
        a.priority = 0;
        a
    };
    let same_as_before = activities::get(CARD_ID)
        .is_some_and(|o| o.title == a.title && o.subtitle == a.subtitle && o.value == a.value && o.color == a.color);
    if !same_as_before {
        activities::upsert(app, a);
    }
}

fn hello_pop(app: &AppHandle) {
    let (name, battery) = {
        let l = LIVE.lock().unwrap();
        (l.phone.as_ref().map(|p| p.name.clone()).unwrap_or_default(), l.battery.map(|b| b.0))
    };
    let mut a = share::act(HELLO_ID, name, "Verbunden".into());
    if let Some(b) = battery {
        a.value = Some(b.to_string());
        a.unit = Some("%".into());
    }
    a.ttl = Some(4);
    activities::upsert(app, a);
}

/// Jede Anfrage vom Kurzbefehl: Adresse, Name und Akku merken, „ist da“
fn seen(app: &AppHandle, ip: IpAddr, q: &str) {
    // Anfragen vom PC selbst (Einrichtungsseite im eigenen Browser, Tests) sind kein iPhone
    if (ip.is_loopback() || ip.to_string() == share::addr().0) && !share::test_mode() {
        return;
    }
    let name: String = query(q, "geraet").map(url_decode).unwrap_or_default().trim().chars().take(40).collect();
    let battery = query(q, "akku").and_then(|s| s.trim().replace(',', ".").parse::<f64>().ok()).map(|b| {
        // Kurzbefehle liefern je nach Version 0..1 oder 0..100
        let pct = if b <= 1.0 { b * 100.0 } else { b };
        pct.round().clamp(0.0, 100.0) as u8
    });
    let charging = query(q, "laedt").map(|s| matches!(s, "1" | "true" | "Ja" | "ja" | "yes"));
    let (arrived, changed) = {
        let mut l = LIVE.lock().unwrap();
        let old = l.phone.clone().unwrap_or_default();
        let p = Phone {
            name: if name.is_empty() { if old.name.is_empty() { "iPhone".into() } else { old.name.clone() } } else { name },
            ip: ip.to_string(),
        };
        let changed = (p != old).then(|| p.clone());
        l.phone = Some(p);
        let arrived = !l.present;
        l.present = true;
        l.misses = 0;
        l.seen = Some(Instant::now());
        if let Some(b) = battery {
            l.battery = Some((b, Instant::now()));
        }
        if charging.is_some() {
            l.charging = charging;
        }
        (arrived, changed)
    };
    if let Some(p) = changed {
        save_phone(app, &p);
    }
    if arrived {
        hello_pop(app);
    }
    card(app);
    share::status_event(app);
}

// ---------- Empfangen ----------

fn has_ext(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(a, b)| !a.is_empty() && (1..=5).contains(&b.len()) && b.bytes().all(|c| c.is_ascii_alphanumeric()) && b.bytes().any(|c| c.is_ascii_alphabetic()))
}

fn ext_for(ctype: &str) -> &'static str {
    match ctype.split(';').next().unwrap_or("").trim() {
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/heic" | "image/heif" => ".heic",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "video/quicktime" => ".mov",
        "video/mp4" => ".mp4",
        "application/pdf" => ".pdf",
        "audio/mpeg" => ".mp3",
        "audio/x-m4a" | "audio/mp4" => ".m4a",
        "text/plain" => ".txt",
        _ => "",
    }
}

/// Ist das ein Text (und keine Datei)? Klein, gueltiges UTF-8, kein Bild/Video/PDF, und ohne Dateiendung
/// (Kurzbefehle nennen geteilten Text „Text“, Links nach dem Seitentitel; Fotos heissen IMG_1234.HEIC).
pub(crate) fn looks_like_text(head: &[u8], ctype: &str, name: &str, clip: bool) -> bool {
    let binary = ["image/", "video/", "audio/", "application/pdf", "application/zip"].iter().any(|p| ctype.starts_with(p));
    !binary && head.len() <= TEXT_MAX && std::str::from_utf8(head).is_ok() && (clip || !has_ext(name))
}

fn text_in(app: &AppHandle, text: &str, clip: bool) -> Response<std::io::Cursor<Vec<u8>>> {
    let t = text.trim();
    if t.is_empty() {
        return plain(400, "leer");
    }
    // ein einzelner Link aus dem Teilen-Menue: wie AirDrop am Mac direkt oeffnen
    if !clip && share::first_url(t).is_some_and(|u| u == t) {
        if let Err(e) = crate::open::open_target(app, t) {
            return plain(500, &e);
        }
        let host = t.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(t);
        let mut a = share::act(CLIP_ID, "Link vom iPhone geöffnet".into(), host.chars().take(60).collect());
        a.ttl = Some(5);
        activities::upsert(app, a);
        return json_resp(json!({ "ok": true, "was": "link" }));
    }
    if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(t.to_string())) {
        return plain(500, &e.to_string());
    }
    // clipboard.rs sieht die Aenderung und laesst den Eintrag in die Ablage-Schale fallen
    let preview: String = t.lines().next().unwrap_or("").chars().take(60).collect();
    let mut a = share::act(CLIP_ID, "Zwischenablage vom iPhone".into(), preview);
    a.ttl = Some(4);
    activities::upsert(app, a);
    json_resp(json!({ "ok": true, "was": "text" }))
}

fn file_in(app: &AppHandle, req: &mut Request, head: Vec<u8>, name: &str, ctype: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut name = safe_name(if name.trim().is_empty() { "iPhone" } else { name });
    if !has_ext(&name) {
        name.push_str(ext_for(ctype));
    }
    let dir = share::folder(app);
    let _ = std::fs::create_dir_all(&dir);
    let Some(target) = share::reserve(&dir, &name) else { return plain(500, "konnte nicht speichern") };
    let size = File::create(&target).ok().and_then(|mut f| {
        use std::io::Write;
        f.write_all(&head).ok()?;
        let rest = std::io::copy(&mut req.as_reader().take(MAX_FILE), &mut f).ok()?;
        Some(head.len() as u64 + rest)
    });
    let Some(size) = size else {
        let _ = std::fs::remove_file(&target);
        return plain(500, "konnte nicht speichern");
    };
    let p = target.to_string_lossy().to_string();
    crate::shelf::shelf_add(app.clone(), vec![p]);
    let kind = share::kind_of(&name, ctype);
    let kinds = {
        let mut l = LIVE.lock().unwrap();
        let b = l.batch.take().filter(|(_, at)| at.elapsed() < BATCH_GAP);
        let mut kinds = b.map(|b| b.0).unwrap_or_default();
        kinds.push(kind);
        l.batch = Some((kinds.clone(), Instant::now()));
        kinds
    };
    let shown = target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(name);
    let mut a = share::act(share::DONE_ID, format!("{} vom iPhone", count_text(&kinds)), format!("{shown} · {}", fmt_size(size)));
    a.actions = vec![share::btn("folder", "Ordner", Some("folder"))];
    a.ttl = Some(15);
    a.alert = kinds.len() == 1; // nur die erste Datei einer Sendung klappt auf
    activities::upsert(app, a);
    json_resp(json!({ "ok": true, "was": "datei", "name": shown }))
}

fn receive(app: &AppHandle, req: &mut Request, q: &str, clip: bool) -> Response<std::io::Cursor<Vec<u8>>> {
    let ctype = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Content-Type"))
        .map(|h| h.value.as_str().to_ascii_lowercase())
        .unwrap_or_default();
    let name = url_decode(query(q, "name").unwrap_or(""));
    let mut head = Vec::new();
    if req.as_reader().take(TEXT_MAX as u64 + 1).read_to_end(&mut head).is_err() {
        return plain(400, "abgebrochen");
    }
    if looks_like_text(&head, &ctype, &name, clip) {
        return text_in(app, &String::from_utf8_lossy(&head), clip);
    }
    let name = if clip && name.is_empty() { "Zwischenablage".to_string() } else { name };
    file_in(app, req, head, &name, &ctype)
}

/// Alles unter /k/ (aus share.rs weitergereicht)
pub fn handle(app: &AppHandle, req: &mut Request, path: &str, q: &str, ip: IpAddr) -> Response<std::io::Cursor<Vec<u8>>> {
    let rest = path.strip_prefix("/k/").unwrap_or("");
    let (tok, sub) = rest.split_once('/').unwrap_or((rest, ""));
    if !same(tok, &token(app)) {
        return plain(403, "Nicht gekoppelt: Kurzbefehl in der Notch neu einrichten (Einstellungen › iPhone).");
    }
    load_phone(app);
    match (req.method(), sub) {
        (Method::Get, "") => {
            seen(app, ip, q); // QR-Code mit dem iPhone gescannt = gekoppelt
            let (ip_pc, port, _) = share::addr();
            let base = format!("http://{ip_pc}:{port}/k/{tok}");
            Response::from_string(PAGE.replace("{{BASE}}", &share::html_escape(&base)).replace("{{NAME}}", &share::html_escape(&share::alias())))
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_header(header("Cache-Control", "no-store"))
        }
        // signierter Kurzbefehl (Quelle tools/shortcuts/notch.cherri), fragt beim Hinzufuegen nach der Adresse
        (Method::Get, "Notch.shortcut") => Response::from_data(SHORTCUT)
            .with_header(header("Content-Type", "application/octet-stream"))
            .with_header(header("Content-Disposition", "attachment; filename=\"Notch.shortcut\"")),
        (Method::Get | Method::Post, "hallo") => {
            seen(app, ip, q);
            json_resp(json!({ "ok": true, "pc": share::alias() }))
        }
        (Method::Post, "senden" | "clip") => {
            seen(app, ip, q);
            receive(app, req, q, sub == "clip")
        }
        _ => plain(404, "nicht gefunden"),
    }
}

// ---------- Im WLAN? ----------

/// Adresse per ARP neu aufloesen (vorhandener Eintrag wird verworfen). true = das Geraet hat geantwortet.
fn arp_ok(ip: Ipv4Addr) -> bool {
    use windows::Win32::NetworkManagement::IpHelper::{GetBestInterface, ResolveIpNetEntry2, MIB_IPNET_ROW2};
    use windows::Win32::Networking::WinSock::{AF_INET, IN_ADDR, IN_ADDR_0};
    let addr = u32::from_ne_bytes(ip.octets());
    let mut idx = 0u32;
    if unsafe { GetBestInterface(addr, &mut idx) } != 0 {
        return false;
    }
    let mut row = MIB_IPNET_ROW2 { InterfaceIndex: idx, ..Default::default() };
    row.Address.Ipv4.sin_family = AF_INET;
    row.Address.Ipv4.sin_addr = IN_ADDR { S_un: IN_ADDR_0 { S_addr: addr } };
    unsafe { ResolveIpNetEntry2(&mut row, None) }.is_ok()
}

/// Alle 20 s nachsehen, ob das iPhone noch im WLAN ist; Karte nachfuehren
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        load_phone(&app);
        loop {
            std::thread::sleep(PROBE_EVERY);
            let ip = {
                let l = LIVE.lock().unwrap();
                let fresh = l.seen.is_some_and(|s| s.elapsed() < PROBE_EVERY);
                l.phone.as_ref().filter(|_| share::enabled() && !fresh).and_then(|p| p.ip.parse::<Ipv4Addr>().ok())
            };
            if let Some(ip) = ip.filter(|i| !i.is_loopback()) {
                let ok = arp_ok(ip);
                let (arrived, left) = {
                    let mut l = LIVE.lock().unwrap();
                    let was = l.present;
                    if ok {
                        l.present = true;
                        l.misses = 0;
                        l.seen = Some(Instant::now());
                    } else {
                        l.misses += 1;
                        if l.misses >= MISSES_UNTIL_GONE {
                            l.present = false;
                        }
                    }
                    (!was && l.present, was && !l.present)
                };
                if arrived {
                    hello_pop(&app);
                }
                if arrived || left {
                    share::status_event(&app);
                }
            }
            card(&app);
        }
    });
}

// ---------- Einstellungen ----------

/// fuer share::status (Einstellungen › iPhone)
pub fn status_json() -> Value {
    let l = LIVE.lock().unwrap();
    match &l.phone {
        Some(p) => json!({
            "name": p.name,
            "present": l.present,
            "battery": l.battery.filter(|(_, at)| at.elapsed() < BATTERY_FOR).map(|b| b.0),
            "seen": l.seen.map(ago),
        }),
        None => Value::Null,
    }
}

/// QR-Code zur Einrichtungsseite (dort steht die Adresse fuer den Kurzbefehl und die Anleitung)
#[tauri::command]
pub fn iphone_setup(app: AppHandle) -> Result<Value, String> {
    let (ip, port, running) = share::addr();
    if !running {
        return Err("iPhone-Austausch ist aus.".into());
    }
    if ip.is_empty() {
        return Err("Kein lokales Netzwerk gefunden.".into());
    }
    let url = format!("http://{ip}:{port}/k/{}/", token(&app));
    Ok(json!({ "url": url, "svg": share::qr_svg(&url)? }))
}

/// Neuer Schluessel: der alte Kurzbefehl geht nicht mehr, das gemerkte iPhone wird vergessen
#[tauri::command]
pub fn iphone_unpair(app: AppHandle) {
    *TOKEN.lock().unwrap() = new_token(&app);
    *LIVE.lock().unwrap() = Live::default();
    if let Some(f) = data_file(&app, "iphone.json") {
        let _ = std::fs::remove_file(f);
    }
    activities::remove(&app, CARD_ID);
    share::status_event(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_oder_datei() {
        assert!(looks_like_text(b"Hallo", "text/plain", "Text", false));
        assert!(looks_like_text(b"https://example.com", "", "Beispielseite", false));
        assert!(!looks_like_text(b"\xff\xd8\xff", "image/jpeg", "IMG_1.JPG", false));
        assert!(!looks_like_text(b"hallo", "text/plain", "notiz.txt", false)); // geteilte Datei bleibt Datei
        assert!(looks_like_text(b"notiz.txt", "text/plain", "", true)); // Zwischenablage ist immer Text
        assert!(!looks_like_text(b"%PDF-1.7", "application/pdf", "", true));
        assert!(!looks_like_text(&vec![b'a'; TEXT_MAX + 1], "text/plain", "Text", false));
    }

    #[test]
    fn schluessel_vergleich() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }

    #[test]
    fn endungen() {
        assert!(has_ext("IMG_1234.HEIC"));
        assert!(!has_ext("Text"));
        assert!(!has_ext("Version 2.0 der Seite"));
        assert_eq!(ext_for("image/jpeg; charset=binary"), ".jpg");
    }

    #[test]
    #[ignore]
    fn arp_probe() {
        let ip: Ipv4Addr = std::env::var("ARP_IP").unwrap().parse().unwrap();
        let t = Instant::now();
        println!("{ip}: {} in {:?}", arp_ok(ip), t.elapsed());
        let dead = Ipv4Addr::new(ip.octets()[0], ip.octets()[1], ip.octets()[2], 253);
        let t = Instant::now();
        println!("{dead}: {} in {:?}", arp_ok(dead), t.elapsed());
    }

}
