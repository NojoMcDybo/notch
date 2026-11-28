//! iPhone als Webcam (Windows): Safari auf dem iPhone schickt Kamerabilder an die Notch, die sie als virtuelle Kamera
//! „Notch iPhone“ bereitstellt (vcam.rs). Teams, Zoom, Chrome und die Kamera-App sehen eine normale Webcam.
//!
//! Safari erlaubt die Kamera nur ueber HTTPS. Die Notch bedient deshalb zusaetzlich zum HTTP-Server (share.rs) einen
//! HTTPS-Server mit selbst erzeugtem Zertifikat (Adresse und Rechnername des PCs); beim ersten Oeffnen fragt Safari
//! einmal „Verbindung nicht privat“ → „Trotzdem besuchen“. Die Seite (`cam-page.html`) sendet je Bild ein JPEG
//! (`POST /cam/<schluessel>/frame`, 1280x720); die Notch decodiert, wandelt nach NV12 und schreibt es fuer die Quelle.
//! Aus, bis der Schalter in Einstellungen › iPhone an ist; die einmalige Einrichtung der virtuellen Kamera braucht
//! Administratorrechte (siehe vcam.rs).

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tiny_http::{Method, Request, Response, Server, SslConfig};

use crate::activities;
use crate::cam_audio;
use crate::share::{self, header, plain};
use crate::vcam;

const PAGE: &str = include_str!("cam-page.html");
const ACT_ID: &str = "notch:iphone:cam";
const PORT: u16 = 53443;
/// groesstes angenommenes JPEG (1280x720 sind ~100 KB)
const MAX_FRAME: u64 = 3 * 1024 * 1024;
/// ein Tonstueck (~0,1 s) ist ~20 KB
const MAX_AUDIO: u64 = 512 * 1024;
/// so lange gilt ein Geraet ohne neues Bild noch als sendend
const ACTIVE: Duration = Duration::from_secs(3);
/// schweigt das Live-Geraet so lange, uebernimmt ein anderes sendendes (wie lange die Kamera das letzte Bild haelt)
const LIVE_FALLBACK: Duration = Duration::from_secs(5);
/// so lange bleibt ein verstummtes Geraet in der Liste (Einstellungen, Vorschau)
const FORGET: Duration = Duration::from_secs(20);
/// Einstellungen, die Notch und Geraet austauschen (Bedeutung siehe cam-page.html / Camera.swift)
const SETTING_KEYS: &[&str] = &["front", "lens", "quality", "rate", "mirror", "zoom", "torch"];

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Ein Geraet, das Bilder schickt (Safari-Seite, Notch-App, Bildschirmuebertragung). Mehrere duerfen gleichzeitig
/// senden; die virtuelle Kamera zeigt nur das Live-Geraet, die anderen sieht man als Vorschau in der Notch.
struct Src {
    name: String,
    /// erster Kontakt; die Liste ist danach sortiert
    first: Instant,
    /// letztes Bild (`None`: bisher nur Einstellungen gemeldet)
    last: Option<Instant>,
    /// letztes Bild als JPEG, fuer die Vorschau in der Notch
    jpeg: Vec<u8>,
    /// zaehlt die Bilder; die Vorschau holt nur neue
    seq: u64,
    /// Bildschirmuebertragung statt Kamera
    screen: bool,
    /// Bilder im laufenden Zeitfenster, Beginn des Fensters, Bildrate
    count: u32,
    since: Instant,
    fps: u32,
    /// Einstellungen (beidseitig: Geraet meldet seine, die Notch aendert sie) und was das Geraet kann
    settings: Value,
    caps: Value,
    /// Stand der Einstellungen; das Geraet erfaehrt ihn mit jeder Antwort und holt neue ab
    rev: u64,
}

impl Src {
    fn new(name: &str) -> Self {
        let now = Instant::now();
        Src {
            name: name.into(), first: now, last: None, jpeg: Vec::new(), seq: 0, screen: false,
            count: 0, since: now, fps: 0, settings: json!({}), caps: json!({}), rev: 0,
        }
    }
    fn active(&self) -> bool {
        self.last.is_some_and(|t| t.elapsed() < ACTIVE)
    }
}

#[derive(Default)]
struct Live {
    server: Option<(Arc<Server>, u16)>,
    /// Adressen, fuer die das laufende Zertifikat gilt
    names: Vec<String>,
    writer: Option<vcam::Writer>,
    /// verbundene Geraete (Schluessel siehe `sender`)
    sources: HashMap<String, Src>,
    /// dieses Geraet liefert das Bild der virtuellen Kamera
    live_key: Option<String>,
    /// Zeile „iPhone-Kamera“ wird angezeigt; zuletzt aktualisiert
    shown: Option<Instant>,
    error: String,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);

fn live<R>(f: impl FnOnce(&mut Live) -> R) -> R {
    let mut g = LIVE.lock().unwrap();
    f(g.get_or_insert_with(Live::default))
}

fn flag_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("iphone-cam.txt"))
}

// ---------- Zertifikat ----------

/// Zertifikat fuer die Adressen, unter denen das iPhone den PC erreicht; bleibt gleich, solange sich die Adressen
/// nicht aendern (sonst fragt Safari jedes Mal neu)
fn cert_names() -> Vec<String> {
    let mut names = vec!["localhost".to_string()];
    let ip = share::lan_ip();
    if !ip.is_empty() {
        names.push(ip);
    }
    if let Some(h) = share::computer_name().filter(|n| n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')) {
        names.push(format!("{h}.local"));
    }
    names
}

fn cert(app: &AppHandle, names: Vec<String>) -> Result<(String, String), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let (cf, kf, nf) = (dir.join("cam-cert.pem"), dir.join("cam-key.pem"), dir.join("cam-names.txt"));
    let want = names.join(",");
    if std::fs::read_to_string(&nf).is_ok_and(|s| s == want) {
        if let (Ok(c), Ok(k)) = (std::fs::read_to_string(&cf), std::fs::read_to_string(&kf)) {
            return Ok((c, k));
        }
    }
    let mut params = rcgen::CertificateParams::new(names).map_err(|e| e.to_string())?;
    params.distinguished_name.push(rcgen::DnType::CommonName, "Notch iPhone-Kamera");
    params.not_before = rcgen::date_time_ymd(2026, 1, 1);
    params.not_after = rcgen::date_time_ymd(2036, 1, 1);
    let key = rcgen::KeyPair::generate().map_err(|e| e.to_string())?;
    let c = params.self_signed(&key).map_err(|e| e.to_string())?;
    let (cp, kp) = (c.pem(), key.serialize_pem());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Schluessel liegt im App-Ordner des Benutzers (Standardrechte des Profils, kein eigener Schutz); gelingt das Ablegen
    // nicht, gilt das Zertifikat nur bis zum Ende der Sitzung und Safari fragt beim naechsten Start erneut
    for (f, c) in [(&cf, &cp), (&kf, &kp), (&nf, &want)] {
        if let Err(e) = std::fs::write(f, c) {
            eprintln!("[notch] Kamera: {} nicht gespeichert: {e}", f.display());
        }
    }
    Ok((cp, kp))
}

// ---------- Server ----------

fn start_server(app: &AppHandle) -> Result<u16, String> {
    if let Some((_, p)) = live(|l| l.server.clone()) {
        return Ok(p);
    }
    eprintln!("[notch] Kamera: Zertifikat ...");
    let names = cert_names();
    let (c, k) = cert(app, names.clone())?;
    eprintln!("[notch] Kamera: Zertifikat da, binde Port");
    let mut last = String::new();
    for port in PORT..PORT + 4 {
        let cfg = SslConfig { certificate: c.clone().into_bytes(), private_key: k.clone().into_bytes() };
        match Server::https(("0.0.0.0", port), cfg) {
            Ok(s) => {
                let s = Arc::new(s);
                live(|l| (l.server, l.names) = (Some((s.clone(), port)), names));
                let app2 = app.clone();
                std::thread::spawn(move || {
                    for req in s.incoming_requests() {
                        let app = app2.clone();
                        std::thread::spawn(move || handle(&app, req));
                    }
                });
                return Ok(port);
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("HTTPS-Server startet nicht ({last})"))
}

fn handle(app: &AppHandle, mut req: Request) {
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("");
    let rest = path.strip_prefix("/cam/").unwrap_or("");
    let (tok, sub) = rest.split_once('/').unwrap_or((rest, ""));
    let who = sender(&req, &url);
    let name = param(&url, "n").map(|n| n.chars().filter(|c| !c.is_control()).take(40).collect::<String>()).unwrap_or_default();
    let r = if !ENABLED.load(Ordering::Relaxed) {
        plain(403, "Kamera ist aus (Notch › Einstellungen › iPhone).")
    } else if !crate::iphone::token_ok(app, tok) {
        plain(403, "Nicht gekoppelt: Adresse neu aus der Notch holen (Einstellungen › iPhone).")
    } else {
        match (req.method(), sub) {
            (Method::Get, "") => Response::from_string(PAGE.replace("{{NAME}}", &share::html_escape(&share::alias())))
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_header(header("Cache-Control", "no-store")),
            (Method::Post, "frame") => frame(app, &mut req, false, &who, &name),
            // Bildschirm: gleiche virtuelle Kamera, aber mit schwarzen Raendern statt gestreckt
            (Method::Post, "screen") => frame(app, &mut req, true, &who, &name),
            (Method::Get, "settings") => settings_get(&who),
            (Method::Post, "settings") => settings_post(&mut req, &who, &name),
            // Ton nur vom Live-Geraet (sonst liefe der Ton einer Uebertragung, deren Bild gar nicht gezeigt wird)
            (Method::Post, "audio") if !is_live(&who) => {
                let _ = req.as_reader().take(MAX_AUDIO + 1).read_to_end(&mut Vec::new());
                Response::from_string("").with_status_code(204)
            }
            (Method::Post, "audio") => audio(&url, &mut req),
            _ => plain(404, "nicht gefunden"),
        }
    };
    let _ = req.respond(r);
}

/// Absender einer Anfrage: Kennung der Seite bzw. App (`?c=`, je Start neu), sonst Programm (User-Agent) und Adresse;
/// so sind Safari, die Notch-App und die Bildschirmuebertragung auch ohne Kennung auseinanderzuhalten
fn sender(req: &Request, url: &str) -> String {
    if let Some(id) = param(url, "c").filter(|v| !v.is_empty() && v.len() <= 64) {
        return format!("c:{id}");
    }
    let ua = req.headers().iter().find(|h| h.field.equiv("User-Agent")).map(|h| h.value.to_string()).unwrap_or_default();
    let ip = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    format!("ua:{ip}:{ua}")
}

/// Wert eines Parameters der Adresse (`?k=v&…`), Prozent-Kodierung aufgeloest
fn param(url: &str, key: &str) -> Option<String> {
    let raw = url.split_once('?')?.1.split('&').find_map(|p| p.strip_prefix(key)?.strip_prefix('='))?;
    let b = raw.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("x"), 16) {
                    Ok(v) => { out.push(v); i += 3; continue; }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// Live-Geraet pruefen: verstummt es, uebernimmt das zuletzt gestartete sendende; lange stille Geraete vergessen
fn prune(l: &mut Live) {
    l.sources.retain(|_, s| s.last.unwrap_or(s.first).elapsed() < FORGET);
    let ok = l.live_key.as_ref().and_then(|k| l.sources.get(k)).is_some_and(|s| s.last.is_some_and(|t| t.elapsed() < LIVE_FALLBACK));
    if !ok {
        l.live_key = l.sources.iter().filter(|(_, s)| s.active()).max_by_key(|(_, s)| s.first).map(|(k, _)| k.clone());
    }
}

fn is_live(key: &str) -> bool {
    live(|l| {
        prune(l);
        l.live_key.as_deref().is_none_or(|k| k == key)
    })
}

/// Bild eines Geraets merken (Vorschau). Wer zum ersten Mal sendet, wird live: wer am Geraet auf Start drueckt, will
/// gesehen werden; zurueck geht es in der Notch. Rueckgabe: ist es das Live-Geraet, Stand seiner Einstellungen.
fn accept(key: &str, name: &str, screen: bool, jpeg: &[u8]) -> (bool, u64) {
    live(|l| {
        let now = Instant::now();
        let s = l.sources.entry(key.to_string()).or_insert_with(|| Src::new(name));
        let first_frame = s.last.is_none();
        if !name.is_empty() {
            s.name = name.to_string();
        }
        s.last = Some(now);
        s.screen = screen;
        s.seq += 1;
        s.jpeg.clear();
        s.jpeg.extend_from_slice(jpeg);
        s.count += 1;
        let dt = s.since.elapsed();
        if dt >= Duration::from_secs(2) {
            s.fps = (s.count as f64 / dt.as_secs_f64()).round() as u32;
            (s.count, s.since) = (0, now);
        }
        let rev = s.rev;
        if first_frame {
            l.live_key = Some(key.to_string());
        }
        prune(l);
        (l.live_key.as_deref() == Some(key), rev)
    })
}

/// Antwort mit dem Stand fuer das Geraet: ist es live, welche Fassung seiner Einstellungen gilt
fn with_state(r: Response<std::io::Cursor<Vec<u8>>>, live_now: bool, rev: u64) -> Response<std::io::Cursor<Vec<u8>>> {
    r.with_header(header("X-Notch-Live", if live_now { "1" } else { "0" })).with_header(header("X-Notch-Rev", &rev.to_string()))
}

fn frame(app: &AppHandle, req: &mut Request, screen: bool, who: &str, name: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut body = Vec::new();
    if req.as_reader().take(MAX_FRAME + 1).read_to_end(&mut body).is_err() || body.len() as u64 > MAX_FRAME {
        return plain(400, "Bild zu gross oder abgebrochen");
    }
    let (is_live, rev) = accept(who, name, screen, &body);
    // nur das Live-Geraet geht in die virtuelle Kamera; die anderen sieht man als Vorschau in der Notch
    if is_live {
        if let Err(e) = push(&body, screen) {
            return plain(400, &e);
        }
    }
    tick(app);
    with_state(Response::from_string("").with_status_code(204), is_live, rev)
}

/// `GET …/settings`: Einstellungen, die die Notch fuer dieses Geraet hat (nach einer Aenderung in der Notch)
fn settings_get(who: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let (body, live_now, rev) = live(|l| {
        prune(l);
        let live_now = l.live_key.as_deref() == Some(who);
        match l.sources.get(who) {
            Some(s) => (json!({ "rev": s.rev, "settings": s.settings, "live": live_now }), live_now, s.rev),
            None => (json!({ "rev": 0, "settings": {}, "live": false }), false, 0),
        }
    });
    with_state(Response::from_string(body.to_string()).with_header(header("Content-Type", "application/json")), live_now, rev)
}

/// `POST …/settings` mit `{settings, caps}`: das Geraet meldet seine Einstellungen (am Geraet geaendert) und was es
/// kann (Objektive, Zoom, Licht). Rueckgabe: neuer Stand.
fn settings_post(req: &mut Request, who: &str, name: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut body = Vec::new();
    if req.as_reader().take(16 * 1024 + 1).read_to_end(&mut body).is_err() || body.len() > 16 * 1024 {
        return plain(400, "Einstellungen zu gross");
    }
    let Ok(v) = serde_json::from_slice::<Value>(&body) else { return plain(400, "kein JSON") };
    let (rev, live_now) = live(|l| {
        let s = l.sources.entry(who.to_string()).or_insert_with(|| Src::new(name));
        if !name.is_empty() {
            s.name = name.to_string();
        }
        if let Some(set) = v.get("settings").and_then(Value::as_object) {
            s.settings = Value::Object(set.iter().filter(|(k, _)| SETTING_KEYS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect());
            s.rev += 1;
        }
        if let Some(c) = v.get("caps").filter(|c| c.is_object()) {
            s.caps = c.clone();
        }
        let rev = s.rev;
        prune(l);
        (rev, l.live_key.as_deref() == Some(who))
    });
    with_state(Response::from_string(json!({ "rev": rev }).to_string()).with_header(header("Content-Type", "application/json")), live_now, rev)
}

/// Ton der Bildschirmuebertragung: `POST …/audio?rate=48000&ch=2`, Rumpf = 16-bit-PCM
fn audio(url: &str, req: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let q = |k: &str| {
        url.split_once('?')?.1.split('&').find_map(|p| p.strip_prefix(k)?.strip_prefix('=')?.parse::<u32>().ok())
    };
    let (Some(rate), Some(ch)) = (q("rate"), q("ch")) else { return plain(400, "rate und ch fehlen") };
    if !(8000..=96000).contains(&rate) || !(1..=2).contains(&ch) {
        return plain(400, "Tonformat nicht unterstuetzt");
    }
    let mut body = Vec::new();
    if req.as_reader().take(MAX_AUDIO + 1).read_to_end(&mut body).is_err() || body.len() as u64 > MAX_AUDIO {
        return plain(400, "Ton zu gross oder abgebrochen");
    }
    cam_audio::feed(rate, ch as u16, &body);
    Response::from_string("").with_status_code(204)
}

/// Bild einpassen, Rest schwarz (Hochformat-Bildschirm in 16:9)
fn letterbox(src: &image::RgbImage) -> image::RgbImage {
    let (w, h) = src.dimensions();
    let (cw, ch) = (vcam::W as u32, vcam::H as u32);
    let s = (cw as f64 / w as f64).min(ch as f64 / h as f64);
    let (nw, nh) = (((w as f64 * s).round() as u32).clamp(1, cw), ((h as f64 * s).round() as u32).clamp(1, ch));
    let small = if (nw, nh) == (w, h) { src.clone() } else { image::imageops::resize(src, nw, nh, image::imageops::FilterType::Triangle) };
    let mut canvas = image::RgbImage::new(cw, ch);
    image::imageops::overlay(&mut canvas, &small, ((cw - nw) / 2) as i64, ((ch - nh) / 2) as i64);
    canvas
}

/// JPEG -> NV12 -> gemeinsame Datei der virtuellen Kamera
fn push(jpeg: &[u8], fit: bool) -> Result<(), String> {
    // erst die Masse lesen: ein kleines JPEG darf keinen riesigen Puffer anfordern
    let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(jpeg), image::ImageFormat::Jpeg)
        .into_dimensions()
        .map_err(|e| format!("kein JPEG: {e}"))?;
    if w > 4096 || h > 4096 {
        return Err("Bild zu gross".into());
    }
    let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).map_err(|e| format!("kein JPEG: {e}"))?;
    let rgb = img.to_rgb8();
    let rgb = if rgb.dimensions() == (vcam::W as u32, vcam::H as u32) {
        rgb
    } else if fit {
        letterbox(&rgb)
    } else {
        image::imageops::resize(&rgb, vcam::W as u32, vcam::H as u32, image::imageops::FilterType::Triangle)
    };
    let mut nv12 = vec![0u8; vcam::FRAME];
    vcam::rgb_to_nv12(rgb.as_raw(), &mut nv12);
    live(|l| {
        if l.writer.is_none() {
            l.writer = Some(vcam::Writer::open().map_err(|e| e.to_string())?);
        }
        l.writer.as_mut().unwrap().write(&nv12).map_err(|e| e.to_string())
    })
}

/// Zeile „iPhone-Kamera“ in der Notch: beim ersten Bild sofort, danach hoechstens alle 2 s aktualisieren
fn tick(app: &AppHandle) {
    let due = live(|l| {
        let due = l.shown.is_none_or(|t| t.elapsed() >= Duration::from_secs(2));
        if due {
            l.shown = Some(Instant::now());
        }
        due
    });
    if due {
        activity(app, true);
        share::status_event(app);
    }
}

/// Live-Geraet und Zahl der sendenden Geraete
fn summary() -> (bool, u32, String, usize) {
    live(|l| {
        prune(l);
        let n = l.sources.values().filter(|s| s.active()).count();
        match l.live_key.as_ref().and_then(|k| l.sources.get(k)) {
            Some(s) => (s.screen, s.fps, s.name.clone(), n),
            None => (false, 0, String::new(), n),
        }
    })
}

fn activity(app: &AppHandle, streaming: bool) {
    if !streaming {
        live(|l| l.shown = None);
        activities::remove(app, ACT_ID);
        return;
    }
    let (screen, fps, name, n) = summary();
    let mut sub = if fps > 0 { format!("Sendet · {fps} Bilder/s") } else { "Sendet".to_string() };
    if n > 1 {
        sub = format!("{sub} · live: {}", if name.is_empty() { "Gerät" } else { &name });
    }
    let mut a = share::act(ACT_ID, if screen { "iPhone-Bildschirm" } else { "iPhone-Kamera" }.into(), sub);
    a.icon = Some(if screen { "📱" } else { "📷" }.into());
    a.color = Some("#30d158".into());
    a.priority = 5;
    a.ttl = Some(6);
    activities::upsert(app, a);
}

/// Alle Sekunde: kommt kein Bild mehr, verschwindet die Anzeige
fn watchdog(app: AppHandle) {
    std::thread::spawn(move || loop {
        for _ in 0..10 {
            std::thread::sleep(Duration::from_secs(1));
            watch_tick(&app);
        }
        renew_server(&app);
    });
}

/// Alten Server wirklich schliessen. tiny_http gibt den Port beim Verwerfen nur frei, wenn es sich dabei selbst
/// erreicht; an 0.0.0.0 klappt das unter Windows nicht, der Port blieb belegt und nach vier Neustarts des Servers
/// scheiterte jeder weitere mit „Adresse schon in Verwendung“ (os error 10048).
fn retire(s: Arc<Server>, port: u16) {
    s.unblock();
    std::thread::spawn(move || {
        // erst wenn der Anfrage-Thread seine Kopie abgegeben hat, setzt das Verwerfen die Schliess-Marke
        for _ in 0..50 {
            if Arc::strong_count(&s) == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        drop(s);
        // der Annahme-Thread merkt die Marke erst mit der naechsten Verbindung und gibt dann den Port frei
        if port != 0 {
            let _ = std::net::TcpStream::connect(("127.0.0.1", port));
        }
    });
}

/// Aendert sich die Adresse des PCs (WLAN spaet verbunden, neue DHCP-Adresse), braucht der Server ein passendes Zertifikat
fn renew_server(app: &AppHandle) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let old = live(|l| l.server.clone().map(|(s, _)| (s, l.names.clone())));
    if let Some((s, names)) = old {
        if names != cert_names() {
            let port = live(|l| l.server.take()).map_or(0, |(_, p)| p);
            retire(s, port);
            if let Err(e) = start_server(app) {
                live(|l| l.error = e);
            }
            share::status_event(app);
        }
    }
}

fn watch_tick(app: &AppHandle) {
    let (was, now) = live(|l| {
        prune(l);
        (l.shown.is_some(), l.sources.values().any(Src::active))
    });
    if was && !now {
        activity(app, false);
        share::status_event(app);
    }
}

// ---------- Einschalten ----------

fn bring_up(app: &AppHandle, may_elevate: bool) -> Result<(), String> {
    if !vcam::installed() {
        if !may_elevate {
            return Err("Einrichtung nötig".into());
        }
        vcam::elevate_setup(&std::env::current_exe().map_err(|e| e.to_string())?)?;
    }
    eprintln!("[notch] Kamera: eingerichtet, starte virtuelle Kamera");
    vcam::start()?;
    eprintln!("[notch] Kamera: virtuelle Kamera laeuft, starte HTTPS-Server");
    let port = start_server(app)?;
    eprintln!("[notch] Kamera: HTTPS-Server auf Port {port}");
    Ok(())
}

pub fn spawn(app: AppHandle) {
    let on = flag_file(&app).and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|s| s.trim() == "1");
    ENABLED.store(on, Ordering::Relaxed);
    watchdog(app.clone());
    if on {
        // ohne Administratorabfrage beim Start: ist die Kamera noch nicht eingerichtet, steht das in den Einstellungen
        std::thread::spawn(move || {
            if let Err(e) = bring_up(&app, false) {
                eprintln!("[notch] Kamera: {e}");
                live(|l| l.error = e);
            }
            share::status_event(&app);
        });
    }
}

/// Schalter in Einstellungen › iPhone; blockiert, solange Windows nach Administratorrechten fragt
#[tauri::command(async)]
pub fn cam_set(app: AppHandle, on: bool) -> Result<(), String> {
    if on {
        live(|l| l.error.clear());
        if let Err(e) = bring_up(&app, true) {
            live(|l| l.error = e.clone());
            share::status_event(&app);
            return Err(e);
        }
    } else {
        vcam::stop();
        activity(&app, false);
    }
    ENABLED.store(on, Ordering::Relaxed);
    if let Some(f) = flag_file(&app) {
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, if on { "1" } else { "0" });
    }
    share::status_event(&app);
    Ok(())
}

/// Adresse der Kameraseite fuer das iPhone (leer, solange die Kamera aus ist)
pub fn url(app: &AppHandle) -> String {
    let ip = share::lan_ip();
    match live(|l| l.server.as_ref().map(|s| s.1)) {
        Some(port) if ENABLED.load(Ordering::Relaxed) && !ip.is_empty() => {
            format!("https://{ip}:{port}/cam/{}/", crate::iphone::token_for(app))
        }
        _ => String::new(),
    }
}

/// QR-Code zur Kameraseite
#[tauri::command]
pub fn cam_setup(app: AppHandle) -> Result<Value, String> {
    let u = url(&app);
    if u.is_empty() {
        return Err("Kamera ist aus oder noch nicht bereit.".into());
    }
    Ok(json!({ "url": u, "svg": share::qr_svg(&u)? }))
}

// ---------- Karte in der Notch (src/camcard.ts) ----------

/// Verbundene Geraete fuer die Karte: Name, live, sendet, Bildrate, Bildzaehler, Einstellungen und Faehigkeiten
#[tauri::command]
pub fn cam_sources() -> Value {
    live(|l| {
        prune(l);
        let mut v: Vec<_> = l
            .sources
            .iter()
            .filter(|(_, s)| s.last.is_some())
            .map(|(k, s)| {
                (s.first, json!({
                    "key": k, "name": s.name, "live": l.live_key.as_deref() == Some(k.as_str()), "active": s.active(),
                    "fps": s.fps, "seq": s.seq, "screen": s.screen, "settings": s.settings, "caps": s.caps,
                }))
            })
            .collect();
        v.sort_by_key(|(first, _)| *first);
        Value::Array(v.into_iter().map(|(_, j)| j).collect())
    })
}

/// Letztes Bild eines Geraets als JPEG (Base64) fuer die Vorschau
#[tauri::command]
pub fn cam_preview(key: String) -> Option<String> {
    use base64::Engine;
    live(|l| l.sources.get(&key).filter(|s| !s.jpeg.is_empty()).map(|s| base64::engine::general_purpose::STANDARD.encode(&s.jpeg)))
}

fn set_live(key: &str) -> Result<(), String> {
    live(|l| {
        if !l.sources.get(key).is_some_and(Src::active) {
            return Err("Das Gerät sendet gerade nicht.".to_string());
        }
        l.live_key = Some(key.to_string());
        Ok(())
    })
}

/// Dieses Geraet in die virtuelle Kamera schalten
#[tauri::command]
pub fn cam_live(app: AppHandle, key: String) -> Result<(), String> {
    set_live(&key)?;
    activity(&app, true);
    Ok(())
}

/// Einstellung eines Geraets in der Notch aendern (`{"front": true}` usw.); das Geraet holt sie mit der naechsten
/// Antwort ab (X-Notch-Rev)
#[tauri::command]
pub fn cam_config(key: String, patch: Value) -> Result<(), String> {
    let Value::Object(p) = patch else { return Err("Ungültige Einstellung".into()) };
    live(|l| {
        let s = l.sources.get_mut(&key).ok_or_else(|| "Das Gerät ist nicht mehr verbunden.".to_string())?;
        if !s.settings.is_object() {
            s.settings = json!({});
        }
        let o = s.settings.as_object_mut().expect("Objekt");
        for (k, v) in p {
            if SETTING_KEYS.contains(&k.as_str()) {
                o.insert(k, v);
            }
        }
        s.rev += 1;
        Ok(())
    })
}

pub fn status_json(app: &AppHandle) -> Value {
    let (_, fps, _, n) = summary();
    let (streaming, error) = (n > 0, live(|l| l.error.clone()));
    json!({
        "supported": true,
        "on": ENABLED.load(Ordering::Relaxed),
        "installed": vcam::installed(),
        "running": vcam::running(),
        "url": url(app),
        "streaming": streaming,
        "fps": fps,
        "error": error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// die Tests teilen sich den Zustand (LIVE); nacheinander, damit sich Live-Geraete nicht in die Quere kommen
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn mehrere_geraete_eines_ist_live() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (a, b) = ("test:geraet-a", "test:geraet-b");
        assert_eq!(accept(a, "iPhone", false, b"x"), (true, 0), "erstes Geraet ist live");
        std::thread::sleep(Duration::from_millis(5));
        assert!(accept(b, "iPad", false, b"y").0, "neu gestartetes Geraet wird live");
        assert!(!accept(a, "", false, b"z").0, "das andere liefert nur noch die Vorschau");
        assert!(!is_live(a) && is_live(b), "Ton nur vom Live-Geraet");
        // in der Notch zurueckschalten
        set_live(a).unwrap();
        assert!(accept(a, "", false, b"z").0);
        // Live-Geraet verstummt: das andere uebernimmt
        live(|l| {
            if let Some(s) = l.sources.get_mut(a) {
                s.last = s.last.map(|t| t - LIVE_FALLBACK);
            }
        });
        assert!(accept(b, "", false, b"y").0);
        live(|l| {
            l.sources.remove(a);
            l.sources.remove(b);
            l.live_key = None;
        });
    }

    #[test]
    fn einstellungen_aus_der_notch_erhoehen_den_stand() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let k = "test:geraet-einst";
        accept(k, "iPhone", false, b"x");
        cam_config(k.into(), json!({ "front": true, "zoom": 2.5, "boese": 1 })).unwrap();
        let (rev, set) = live(|l| {
            let s = &l.sources[k];
            (s.rev, s.settings.clone())
        });
        assert_eq!(rev, 1);
        assert_eq!(set, json!({ "front": true, "zoom": 2.5 }), "nur bekannte Einstellungen");
        live(|l| {
            l.sources.remove(k);
            l.live_key = None;
        });
    }

    #[test]
    fn parameter_werden_dekodiert() {
        assert_eq!(param("/x?c=ab&n=iPhone%20%C2%B7%20Safari", "n").as_deref(), Some("iPhone · Safari"));
        assert_eq!(param("/x?n=a%2", "n").as_deref(), Some("a%2"));
        assert_eq!(param("/x?c=1", "n"), None);
    }

    #[test]
    fn letterbox_portrait_screen_gets_black_bars() {
        let o = letterbox(&image::RgbImage::from_pixel(333, 720, image::Rgb([255, 255, 255])));
        assert_eq!(o.dimensions(), (vcam::W as u32, vcam::H as u32));
        assert_eq!(o.get_pixel(0, 0).0, [0, 0, 0]);
        assert_eq!(o.get_pixel(vcam::W as u32 / 2, vcam::H as u32 / 2).0, [255, 255, 255]);
    }
}
