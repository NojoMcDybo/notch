//! Zwischenablage-Verlauf: die letzten 5 kopierten Dinge (Text, Dateien, Bilder).
//!
//! - Fragt alle 300 ms die Sequenznummer der Zwischenablage ab (kostet nichts) und liest
//!   nur bei einer Aenderung.
//! - Respektiert die Windows-Kennzeichen, mit denen Passwortmanager ihre Inhalte vom
//!   Verlauf ausschliessen ("ExcludeClipboardContentFromMonitorProcessing",
//!   "CanIncludeInClipboardHistory" = 0, "Clipboard Viewer Ignore") — wie der Windows-Verlauf.
//! - Nur im Speicher; Bilder liegen als PNG in app_data/clip und werden beim Start geloescht.
//! - Gleicher Inhalt erneut kopiert -> rutscht nach oben statt doppelt zu stehen.

use std::borrow::Cow;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

const MAX: usize = 5;
const MAX_TEXT: usize = 20_000;
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;

#[derive(Serialize, Clone)]
pub struct Clip {
    pub id: u64,
    /// "text" | "image" | "files"
    pub kind: String,
    pub text: Option<String>,
    /// PNG-Datei bei Bildern
    pub path: Option<String>,
    pub files: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub at: u64,
    #[serde(skip)]
    key: u64,
}

static CLIPS: LazyLock<Mutex<Vec<Clip>>> = LazyLock::new(Default::default);
static NEXT: AtomicU64 = AtomicU64::new(1);
static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn list() -> Vec<Clip> {
    CLIPS.lock().unwrap().clone()
}

fn emit(app: &AppHandle) {
    let _ = app.emit("clips", list());
}

fn hash<T: Hash>(t: &T) -> u64 {
    let mut h = DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

fn owner() -> Option<HWND> {
    let h = crate::HWND.load(Ordering::Relaxed);
    (h != 0).then(|| HWND(h as *mut _))
}

/// Zwischenablage oeffnen (andere Programme halten sie manchmal kurz fest) und beim Verlassen schliessen.
struct Open;
impl Open {
    fn new(owner: Option<HWND>) -> Option<Self> {
        for _ in 0..8 {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Some(Open);
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        None
    }
}
impl Drop for Open {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

fn available(fmt: u32) -> bool {
    unsafe { IsClipboardFormatAvailable(fmt) }.is_ok()
}

/// Passwortmanager & Co. markieren ihre Inhalte als "nicht in den Verlauf".
fn excluded() -> bool {
    unsafe {
        let ex = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let ign = RegisterClipboardFormatW(w!("Clipboard Viewer Ignore"));
        if available(ex) || available(ign) {
            return true;
        }
        let can = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if available(can) {
            if let Ok(h) = GetClipboardData(can) {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u32;
                if !p.is_null() {
                    let v = *p;
                    let _ = GlobalUnlock(g);
                    if v == 0 {
                        return true;
                    }
                }
            }
        }
        false
    }
}

fn read_text() -> Option<String> {
    if !available(CF_UNICODETEXT) {
        return None;
    }
    unsafe {
        let h = GetClipboardData(CF_UNICODETEXT).ok()?;
        let g = HGLOBAL(h.0);
        let p = GlobalLock(g) as *const u16;
        if p.is_null() {
            return None;
        }
        let n = GlobalSize(g) / 2;
        let s = std::slice::from_raw_parts(p, n);
        let end = s.iter().position(|&c| c == 0).unwrap_or(n);
        let t = String::from_utf16_lossy(&s[..end]);
        let _ = GlobalUnlock(g);
        Some(t)
    }
}

fn read_files() -> Option<Vec<String>> {
    if !available(CF_HDROP) {
        return None;
    }
    unsafe {
        let h = GetClipboardData(CF_HDROP).ok()?;
        let drop = HDROP(h.0);
        let n = DragQueryFileW(drop, u32::MAX, None);
        let mut v = Vec::new();
        for i in 0..n {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(drop, i, Some(&mut buf));
            v.push(String::from_utf16_lossy(&buf[..len]));
        }
        Some(v)
    }
}

fn has_image() -> bool {
    let png = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    available(CF_DIB) || available(CF_DIBV5) || available(png)
}

fn now_ms() -> u64 {
    crate::activities::now_ms()
}

fn new_clip(kind: &str, key: u64) -> Clip {
    Clip {
        id: NEXT.fetch_add(1, Ordering::Relaxed),
        kind: kind.into(),
        text: None,
        path: None,
        files: Vec::new(),
        width: 0,
        height: 0,
        at: now_ms(),
        key,
    }
}

/// Schon im Verlauf? Dann nach oben holen und true liefern.
fn bump(app: &AppHandle, key: u64) -> bool {
    let mut v = CLIPS.lock().unwrap();
    let Some(i) = v.iter().position(|c| c.key == key) else { return false };
    let mut c = v.remove(i);
    c.at = now_ms();
    v.insert(0, c);
    drop(v);
    emit(app);
    true
}

fn add(app: &AppHandle, c: Clip) {
    let mut v = CLIPS.lock().unwrap();
    v.insert(0, c);
    while v.len() > MAX {
        if let Some(old) = v.pop() {
            if let Some(p) = old.path {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    drop(v);
    emit(app);
}

fn capture(app: &AppHandle, dir: &Path) {
    // Erst nur lesen, was drin ist; Bilder holt arboard danach (eigenes Oeffnen)
    let (files, text, image) = {
        let Some(_open) = Open::new(None) else { return };
        if excluded() {
            return;
        }
        let files = read_files().filter(|f| !f.is_empty());
        let text = if files.is_none() { read_text().filter(|t| !t.trim().is_empty()) } else { None };
        let image = files.is_none() && text.is_none() && has_image();
        (files, text, image)
    };

    if let Some(files) = files {
        let key = hash(&("files", &files));
        if !bump(app, key) {
            let mut c = new_clip("files", key);
            c.files = files;
            add(app, c);
        }
    } else if let Some(mut t) = text {
        if t.chars().count() > MAX_TEXT {
            t = t.chars().take(MAX_TEXT).collect();
        }
        let key = hash(&("text", &t));
        if !bump(app, key) {
            let mut c = new_clip("text", key);
            c.text = Some(t);
            add(app, c);
        }
    } else if image {
        let Ok(mut cb) = arboard::Clipboard::new() else { return };
        let Ok(img) = cb.get_image() else { return };
        let key = hash(&("image", img.width, img.height, img.bytes.as_ref()));
        if bump(app, key) {
            return;
        }
        let (w, h) = (img.width as u32, img.height as u32);
        let Some(buf) = image::RgbaImage::from_raw(w, h, img.bytes.into_owned()) else { return };
        let mut c = new_clip("image", key);
        let file = dir.join(format!("{}.png", c.id));
        if buf.save(&file).is_err() {
            return;
        }
        c.path = Some(file.to_string_lossy().into_owned());
        c.width = w;
        c.height = h;
        add(app, c);
    }
}

pub fn spawn(app: AppHandle) {
    let Ok(dir) = app.path().app_data_dir().map(|d| d.join("clip")) else { return };
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    *DIR.lock().unwrap() = Some(dir.clone());
    std::thread::spawn(move || {
        let mut seq = 0u32; // 0 = beim Start den aktuellen Inhalt einmal uebernehmen
        loop {
            let s = unsafe { GetClipboardSequenceNumber() };
            if s != seq {
                // Programme legen mehrere Formate nacheinander ab — kurz warten
                std::thread::sleep(Duration::from_millis(120));
                seq = unsafe { GetClipboardSequenceNumber() };
                capture(&app, &dir);
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    });
}

// ---------- Zurueck in die Zwischenablage ----------

fn put(fmt: u32, bytes: &[u8]) -> bool {
    unsafe {
        let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return false };
        let p = GlobalLock(g) as *mut u8;
        if p.is_null() {
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(g);
        // Bei Erfolg gehoert der Speicher jetzt Windows
        SetClipboardData(fmt, Some(HANDLE(g.0))).is_ok()
    }
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect()
}

fn set_text(t: &str) -> Result<(), String> {
    let _open = Open::new(owner()).ok_or("Zwischenablage belegt")?;
    unsafe { EmptyClipboard() }.map_err(|e| e.to_string())?;
    if put(CF_UNICODETEXT, &utf16z(t)) { Ok(()) } else { Err("Text konnte nicht kopiert werden".into()) }
}

fn set_files(files: &[String]) -> Result<(), String> {
    // DROPFILES: pFiles=20, pt=(0,0), fNC=0, fWide=1, danach die Pfade (UTF-16, je 0-terminiert, am Ende 0)
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(&20u32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&1i32.to_le_bytes());
    for f in files {
        b.extend(utf16z(f));
    }
    b.extend_from_slice(&0u16.to_le_bytes());
    let _open = Open::new(owner()).ok_or("Zwischenablage belegt")?;
    unsafe { EmptyClipboard() }.map_err(|e| e.to_string())?;
    if !put(CF_HDROP, &b) {
        return Err("Dateien konnten nicht kopiert werden".into());
    }
    // Einfuegen soll kopieren, nicht verschieben
    let effect = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
    put(effect, &1u32.to_le_bytes());
    Ok(())
}

fn set_image(path: &str) -> Result<(), String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_image(arboard::ImageData { width: w as usize, height: h as usize, bytes: Cow::Owned(img.into_raw()) })
        .map_err(|e| e.to_string())
}

fn find(id: u64) -> Option<Clip> {
    CLIPS.lock().unwrap().iter().find(|c| c.id == id).cloned()
}

#[tauri::command]
pub fn clip_list() -> Vec<Clip> {
    list()
}

/// Eintrag wieder in die Zwischenablage legen (rutscht dadurch nach oben).
#[tauri::command]
pub async fn clip_copy(id: u64) -> Result<(), String> {
    let c = find(id).ok_or("nicht mehr da")?;
    tauri::async_runtime::spawn_blocking(move || match c.kind.as_str() {
        "text" => set_text(c.text.as_deref().unwrap_or("")),
        "files" => set_files(&c.files),
        "image" => set_image(c.path.as_deref().unwrap_or("")),
        _ => Ok(()),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn clip_remove(app: AppHandle, id: u64) {
    let mut v = CLIPS.lock().unwrap();
    if let Some(i) = v.iter().position(|c| c.id == id) {
        let c = v.remove(i);
        if let Some(p) = c.path {
            let _ = std::fs::remove_file(p);
        }
    }
    drop(v);
    emit(&app);
}

#[tauri::command]
pub fn clip_clear(app: AppHandle) {
    let old: Vec<Clip> = std::mem::take(&mut *CLIPS.lock().unwrap());
    for c in old {
        if let Some(p) = c.path {
            let _ = std::fs::remove_file(p);
        }
    }
    emit(&app);
}

/// Bild oder Dateien in die Ablage legen (Bild wird dafuer als Datei behalten).
#[tauri::command]
pub fn clip_to_shelf(app: AppHandle, id: u64) {
    let Some(c) = find(id) else { return };
    let paths = match c.kind.as_str() {
        "files" => c.files,
        "image" => {
            // die PNG im clip-Ordner wird beim naechsten Start geloescht -> dauerhaft in Bilder\Notch kopieren
            let Some(src) = c.path else { return };
            let dest_dir = app
                .path()
                .picture_dir()
                .map(|d| d.join("Notch"))
                .unwrap_or_else(|_| DIR.lock().unwrap().clone().unwrap_or_default());
            let _ = std::fs::create_dir_all(&dest_dir);
            let dest = dest_dir.join(format!("Zwischenablage {}.png", c.at));
            if std::fs::copy(&src, &dest).is_err() {
                return;
            }
            vec![dest.to_string_lossy().into_owned()]
        }
        _ => return,
    };
    crate::shelf::shelf_add(app, paths);
}
