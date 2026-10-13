//! Laya-Modell fuer Jarvis (docs/jarvis.md, M2c): erkennt beim Sprechen, ob ein genanntes Programm jetzt
//! gestartet werden soll. Die Erkennung selbst laeuft im WebView (src/laya/, onnxruntime-web mit WebGPU); hier nur:
//!
//! - Wunsch merken (`laya.json`: an/aus, Einstellungen › App › „Schon beim Sprechen handeln“).
//! - Paket laden: sechs Dateien vom GitHub-Release `laya-jarvis-1`, jede mit fester Groesse und SHA-256 (unten
//!   festgeschrieben, sonst wird nichts benutzt). Abgebrochene Downloads setzen fort (Range).
//! - Abschalten: Ordner loeschen.
//!
//! Ablage: %LOCALAPPDATA%\de.nojo.notch\laya (vom WebView ueber das Asset-Protokoll lesbar).
//! Uebernommen aus arena/src-tauri/src/laya.rs (ohne Installer-Auswahl). Paket: feingetuntes laya-multilingual,
//! Encoder INT4 mit int8-Einbettungstabelle (tools/jarvis/quantize.py; die INT4-Tabelle rechnet auf WebGPU falsch,
//! docs/jarvis.md 2d), dazu die WASM-Datei von onnxruntime-web 1.30.0 mit WebGPU (jsep).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

const BASE: &str = "https://github.com/NojoMcDybo/notch/releases/download/laya-jarvis-1";

/// Quelle des Pakets; NOTCH_LAYA_BASE ueberschreibt sie (nur zum Testen gegen einen lokalen Server)
fn base_url() -> String {
    std::env::var("NOTCH_LAYA_BASE").ok().filter(|b| b.starts_with("http")).unwrap_or_else(|| BASE.into())
}

/// (Name im Release, Name auf der Platte, Groesse, SHA-256)
const FILES: &[(&str, &str, u64, &str)] = &[
    ("jarvis-de-int4.rl_agent_config.json", "rl_agent_config.json", 549, "de2e7636d30989f9ad803cbeb12c9feaf4b3fcd715d9465714bb8853c052e8d9"),
    ("jarvis-de-int4.LICENSE.txt", "LICENSE.txt", 10_349, "6d1d968fb225eca367cb7f0b8831ab012a35d92b547e945e17ef8e7b05c3e5cc"),
    ("jarvis-de-int4.tokenizer.json", "tokenizer.json", 34_363_188, "609d8f4c067cd3950f88594c5a802616cea245823836ef5848ee4fc40aab5b6f"),
    ("jarvis-de-int4.ort-1.30.0.jsep.wasm", "ort.wasm", 28_312_028, "3ad23231b5bd6d9dda55a7f84606315e0bf35b6750c28ee993c987c54cacab0f"),
    ("jarvis-de-int4.head.onnx", "head.onnx", 22_277_227, "d5d5914641ccb3bda59925db1446944ec294ee26923856c17d70d14ed2abd1c6"),
    ("jarvis-de-int4.encoder.onnx", "encoder.onnx", 268_926_230, "c7f350e56680c8f2b5e9c2daf187974b75e40ecf095ae70a3e718ca582ea0c6b"),
];
/// Kennung des Pakets: aendert sich mit jedem neuen Modell (alte Ordner werden dann ersetzt)
const PACKAGE: &str = "jarvis-de-int4-e8@m2b+ort-1.30.0-jsep";

pub fn total_size() -> u64 {
    FILES.iter().map(|f| f.2).sum()
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    /// Nutzer moechte das Modell
    want: bool,
    /// off | missing | downloading | ready | error
    state: String,
    done: u64,
    total: u64,
    message: String,
    /// Ordner (fuer convertFileSrc im WebView), nur wenn bereit
    dir: String,
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static RUNNING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

fn base_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_local_data_dir().ok()
}
fn model_dir(app: &AppHandle) -> Option<PathBuf> {
    base_dir(app).map(|d| d.join("laya"))
}

fn read_want(app: &AppHandle) -> bool {
    base_dir(app)
        .and_then(|d| fs::read_to_string(d.join("laya.json")).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["want"].as_bool())
        .unwrap_or(false)
}

fn write_want(app: &AppHandle, want: bool) {
    if let Some(d) = base_dir(app) {
        let _ = fs::create_dir_all(&d);
        let _ = fs::write(d.join("laya.json"), serde_json::json!({ "want": want }).to_string());
    }
}

/// Bereit = Paketkennung stimmt und alle Dateien haben ihre Groesse (Pruefsummen beim Laden geprueft)
fn is_ready(dir: &Path) -> bool {
    fs::read_to_string(dir.join("package.txt")).is_ok_and(|p| p.trim() == PACKAGE)
        && FILES.iter().all(|f| fs::metadata(dir.join(f.1)).is_ok_and(|m| m.len() == f.2))
}

fn compute(app: &AppHandle) -> Status {
    let want = read_want(app);
    let dir = model_dir(app);
    let ready = dir.as_deref().is_some_and(is_ready);
    Status {
        want,
        state: if ready { "ready" } else if want { "missing" } else { "off" }.into(),
        done: if ready { total_size() } else { 0 },
        total: total_size(),
        message: String::new(),
        dir: if ready { dir.map(|d| d.to_string_lossy().into_owned()).unwrap_or_default() } else { String::new() },
    }
}

fn set(app: &AppHandle, s: Status) {
    *STATUS.lock().unwrap() = Some(s.clone());
    let _ = app.emit("laya", s);
}

fn current(app: &AppHandle) -> Status {
    let g = STATUS.lock().unwrap().clone();
    g.unwrap_or_else(|| compute(app))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(3 * 3600)))
        .user_agent(concat!("Notch/", env!("CARGO_PKG_VERSION"), " (Windows)"))
        .build()
        .into()
}

fn hash_file(path: &Path, h: &mut Sha256) -> std::io::Result<u64> {
    let mut f = File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    loop {
        let k = f.read(&mut buf)?;
        if k == 0 {
            return Ok(n);
        }
        h.update(&buf[..k]);
        n += k as u64;
    }
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Eine Datei laden (fortsetzen, wenn ein Teil da ist) und pruefen. `progress` bekommt die Bytes dieser Datei.
fn fetch_one(agent: &ureq::Agent, base: &str, dir: &Path, f: &(&str, &str, u64, &str), progress: &mut dyn FnMut(u64)) -> Result<(), String> {
    let (asset, name, size, sha) = *f;
    let dest = dir.join(name);
    if fs::metadata(&dest).is_ok_and(|m| m.len() == size) {
        return Ok(()); // schon geprueft geladen (Groesse genuegt hier; is_ready verlangt zusaetzlich package.txt)
    }
    let part = dir.join(format!("{name}.part"));
    let mut h = Sha256::new();
    let mut have = if part.exists() { hash_file(&part, &mut h).map_err(|e| e.to_string())? } else { 0 };
    if have > size {
        let _ = fs::remove_file(&part);
        h = Sha256::new();
        have = 0;
    }
    if have < size {
        let url = format!("{base}/{asset}");
        let mut req = agent.get(&url);
        if have > 0 {
            req = req.header("Range", format!("bytes={have}-"));
        }
        let res = req.call().map_err(|e| format!("Download fehlgeschlagen ({asset}): {e}"))?;
        if have > 0 && res.status().as_u16() != 206 {
            // Server kann nicht fortsetzen: von vorn
            h = Sha256::new();
            have = 0;
        }
        let mut out = OpenOptions::new().create(true).write(true).append(have > 0).truncate(have == 0).open(&part).map_err(|e| e.to_string())?;
        let mut r = res.into_body().into_reader();
        let mut buf = vec![0u8; 256 * 1024];
        let mut last = Instant::now();
        loop {
            if CANCEL.load(Ordering::SeqCst) {
                return Err("abgebrochen".into());
            }
            let k = r.read(&mut buf).map_err(|e| format!("Download unterbrochen: {e}"))?;
            if k == 0 {
                break;
            }
            out.write_all(&buf[..k]).map_err(|e| e.to_string())?;
            h.update(&buf[..k]);
            have += k as u64;
            if have > size {
                break;
            }
            if last.elapsed() > Duration::from_millis(300) {
                last = Instant::now();
                progress(have);
            }
        }
        out.flush().map_err(|e| e.to_string())?;
    }
    if have != size || hex(&h.finalize()) != sha {
        let _ = fs::remove_file(&part);
        return Err(format!("Prüfsumme stimmt nicht ({name}) – Datei verworfen."));
    }
    fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    Ok(())
}

/// Ganzes Paket nach `dir` laden; `progress(geladen, gesamt)`
fn fetch_all(dir: &Path, base: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    // anderes (altes) Paket: Ordner leeren
    if fs::read_to_string(dir.join("package.txt")).is_ok_and(|p| p.trim() != PACKAGE) {
        let _ = fs::remove_dir_all(dir);
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let agent = agent();
    let mut before = 0u64;
    for f in FILES {
        progress(before, total_size());
        fetch_one(&agent, base, dir, f, &mut |n| progress(before + n, total_size()))?;
        before += f.2;
    }
    fs::write(dir.join("package.txt"), PACKAGE).map_err(|e| e.to_string())?;
    Ok(())
}

fn download(app: AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    CANCEL.store(false, Ordering::SeqCst);
    let result = match model_dir(&app) {
        None => Err("kein Datenordner".to_string()),
        Some(dir) => fetch_all(&dir, &base_url(), &mut |done, total| {
            set(&app, Status { want: true, state: "downloading".into(), done, total, ..Default::default() });
        }),
    };
    RUNNING.store(false, Ordering::SeqCst);
    match result {
        Ok(()) => set(&app, compute(&app)),
        Err(_) if CANCEL.load(Ordering::SeqCst) => set(&app, compute(&app)),
        Err(e) => set(&app, Status { want: true, state: "error".into(), total: total_size(), message: e, ..Default::default() }),
    }
}

fn remove(app: &AppHandle) {
    CANCEL.store(true, Ordering::SeqCst);
    // laufenden Download kurz ausklingen lassen (er prueft CANCEL nach jedem Block)
    for _ in 0..40 {
        if !RUNNING.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if let Some(d) = model_dir(app) {
        let _ = fs::remove_dir_all(d);
    }
}

#[tauri::command]
pub fn laya_status(app: AppHandle) -> Status {
    current(&app)
}

/// Modell laden (Wunsch merken, Download im Hintergrund)
#[tauri::command]
pub fn laya_install(app: AppHandle) -> Status {
    write_want(&app, true);
    let s = compute(&app);
    set(&app, s.clone());
    if s.state != "ready" {
        std::thread::spawn(move || download(app));
    }
    s
}

/// Modell entfernen (Wunsch merken, Download abbrechen, Ordner loeschen)
#[tauri::command]
pub async fn laya_uninstall(app: AppHandle) -> Result<Status, String> {
    write_want(&app, false);
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || remove(&a)).await.map_err(|e| e.to_string())?;
    let s = compute(&app);
    set(&app, s.clone());
    Ok(s)
}

/// Beim Start: ist das Modell gewuenscht, aber nicht (vollstaendig) da, nachladen
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let s = compute(&app);
        set(&app, s.clone());
        if s.state == "missing" {
            // nicht gleich beim Start
            std::thread::sleep(Duration::from_secs(8));
            if read_want(&app) {
                download(app);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paket_vollstaendig() {
        assert_eq!(FILES.len(), 6);
        assert!(FILES.iter().all(|f| f.3.len() == 64 && f.3.chars().all(|c| c.is_ascii_hexdigit())));
        assert!(total_size() > 345_000_000 && total_size() < 365_000_000);
        // jede Datei, die laya-ts / der Worker braucht
        for n in ["encoder.onnx", "head.onnx", "tokenizer.json", "rl_agent_config.json", "ort.wasm"] {
            assert!(FILES.iter().any(|f| f.1 == n), "{n} fehlt");
        }
    }

    /// Gegen einen lokalen Server mit dem Paket (tools/jarvis/README.md):
    /// NOTCH_LAYA_BASE=http://127.0.0.1:8767 cargo test --lib laya_download -- --ignored --nocapture
    #[test]
    #[ignore]
    fn laya_download() {
        let base = std::env::var("NOTCH_LAYA_BASE").expect("NOTCH_LAYA_BASE setzen");
        let dir = std::env::temp_dir().join("notch-laya-download");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // halber Teil liegt schon da: muss fortgesetzt werden und trotzdem die Pruefsumme treffen
        let enc = FILES.iter().find(|f| f.1 == "encoder.onnx").unwrap();
        let src = reqwest_like(&format!("{base}/{}", enc.0), 1_000_000);
        fs::write(dir.join("encoder.onnx.part"), src).unwrap();
        let mut calls = 0;
        let t = Instant::now();
        fetch_all(&dir, &base, &mut |_, _| calls += 1).expect("Download");
        assert!(is_ready(&dir));
        println!("geladen in {:?}, {calls} Fortschrittsmeldungen", t.elapsed());
        // kaputte Datei wird erkannt und verworfen
        fs::remove_file(dir.join("LICENSE.txt")).unwrap();
        fs::write(dir.join("LICENSE.txt.part"), vec![b'x'; 10_349]).unwrap();
        assert!(fetch_all(&dir, &base, &mut |_, _| {}).is_err());
        assert!(!dir.join("LICENSE.txt.part").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// erste n Bytes einer Adresse
    fn reqwest_like(url: &str, n: usize) -> Vec<u8> {
        let mut res = agent().get(url).call().unwrap();
        let mut buf = vec![0u8; n];
        res.body_mut().as_reader().read_exact(&mut buf).unwrap();
        buf
    }

    #[test]
    fn pruefsumme() {
        let p = std::env::temp_dir().join("notch-laya-test.bin");
        fs::write(&p, b"abc").unwrap();
        let mut h = Sha256::new();
        assert_eq!(hash_file(&p, &mut h).unwrap(), 3);
        assert_eq!(hex(&h.finalize()), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = fs::remove_file(p);
    }
}
