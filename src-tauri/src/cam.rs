//! iPhone als Webcam (Windows): Safari auf dem iPhone schickt Kamerabilder an die Notch, die sie als virtuelle Kamera
//! „Notch iPhone“ bereitstellt (vcam.rs). Teams, Zoom, Chrome und die Kamera-App sehen eine normale Webcam.
//!
//! Safari erlaubt die Kamera nur ueber HTTPS. Die Notch bedient deshalb zusaetzlich zum HTTP-Server (share.rs) einen
//! HTTPS-Server mit selbst erzeugtem Zertifikat (Adresse und Rechnername des PCs); beim ersten Oeffnen fragt Safari
//! einmal „Verbindung nicht privat“ → „Trotzdem besuchen“. Die Seite (`cam-page.html`) sendet je Bild ein JPEG
//! (`POST /cam/<schluessel>/frame`, 1280x720); die Notch decodiert, wandelt nach NV12 und schreibt es fuer die Quelle.
//! Aus, bis der Schalter in Einstellungen › iPhone an ist; die einmalige Einrichtung der virtuellen Kamera braucht
//! Administratorrechte (siehe vcam.rs).

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tiny_http::{Method, Request, Response, Server, SslConfig};

use crate::activities;
use crate::share::{self, header, plain};
use crate::vcam;

const PAGE: &str = include_str!("cam-page.html");
const ACT_ID: &str = "notch:iphone:cam";
const PORT: u16 = 53443;
/// groesstes angenommenes JPEG (1280x720 sind ~100 KB)
const MAX_FRAME: u64 = 3 * 1024 * 1024;

static ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Live {
    server: Option<(Arc<Server>, u16)>,
    /// Adressen, fuer die das laufende Zertifikat gilt
    names: Vec<String>,
    writer: Option<vcam::Writer>,
    /// Bilder im laufenden Zeitfenster, Beginn des Fensters, letztes Bild
    count: u32,
    since: Option<Instant>,
    last: Option<Instant>,
    fps: u32,
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
    let r = if !ENABLED.load(Ordering::Relaxed) {
        plain(403, "Kamera ist aus (Notch › Einstellungen › iPhone).")
    } else if !crate::iphone::token_ok(app, tok) {
        plain(403, "Nicht gekoppelt: Adresse neu aus der Notch holen (Einstellungen › iPhone).")
    } else {
        match (req.method(), sub) {
            (Method::Get, "") => Response::from_string(PAGE.replace("{{NAME}}", &share::html_escape(&share::alias())))
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_header(header("Cache-Control", "no-store")),
            (Method::Post, "frame") => frame(app, &mut req),
            _ => plain(404, "nicht gefunden"),
        }
    };
    let _ = req.respond(r);
}

fn frame(app: &AppHandle, req: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut body = Vec::new();
    if req.as_reader().take(MAX_FRAME + 1).read_to_end(&mut body).is_err() || body.len() as u64 > MAX_FRAME {
        return plain(400, "Bild zu gross oder abgebrochen");
    }
    if let Err(e) = push(&body) {
        return plain(400, &e);
    }
    tick(app);
    Response::from_string("").with_status_code(204)
}

/// JPEG -> NV12 -> gemeinsame Datei der virtuellen Kamera
fn push(jpeg: &[u8]) -> Result<(), String> {
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

/// Bildzaehler fuer „30 fps“ in der Anzeige
fn tick(app: &AppHandle) {
    let show = live(|l| {
        let now = Instant::now();
        let first = l.last.is_none_or(|t| t.elapsed() > Duration::from_secs(3));
        l.last = Some(now);
        if first || l.since.is_none() {
            (l.since, l.count) = (Some(now), 0);
        }
        l.count += 1;
        match l.since {
            Some(s) if s.elapsed() >= Duration::from_secs(2) => {
                l.fps = (l.count as f64 / s.elapsed().as_secs_f64()).round() as u32;
                (l.since, l.count) = (Some(now), 0);
                true
            }
            _ => first,
        }
    });
    if show {
        activity(app, true);
        share::status_event(app);
    }
}

fn activity(app: &AppHandle, streaming: bool) {
    if !streaming {
        activities::remove(app, ACT_ID);
        return;
    }
    let fps = live(|l| l.fps);
    let mut a = share::act(ACT_ID, "iPhone-Kamera".into(), if fps > 0 { format!("Sendet · {fps} Bilder/s") } else { "Sendet".into() });
    a.icon = Some("📷".into());
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

/// Aendert sich die Adresse des PCs (WLAN spaet verbunden, neue DHCP-Adresse), braucht der Server ein passendes Zertifikat
fn renew_server(app: &AppHandle) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let old = live(|l| l.server.clone().map(|(s, _)| (s, l.names.clone())));
    if let Some((s, names)) = old {
        if names != cert_names() {
            s.unblock();
            live(|l| l.server = None);
            if let Err(e) = start_server(app) {
                live(|l| l.error = e);
            }
            share::status_event(app);
        }
    }
}

fn watch_tick(app: &AppHandle) {
    let (was, now) = live(|l| {
        let was = l.fps > 0 || l.last.is_some();
        let alive = l.last.is_some_and(|t| t.elapsed() < Duration::from_secs(3));
        if !alive && l.last.is_some() {
            (l.last, l.fps) = (None, 0);
        }
        (was, alive)
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

pub fn status_json(app: &AppHandle) -> Value {
    let (streaming, fps, error) = live(|l| (l.last.is_some_and(|t| t.elapsed() < Duration::from_secs(3)), l.fps, l.error.clone()));
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
