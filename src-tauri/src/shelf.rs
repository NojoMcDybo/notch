//! Ablage: Dateien auf die Notch ziehen, dort parken, oeffnen, rausziehen, konvertieren.
//! Gespeichert werden nur Verweise (Pfade), keine Kopien. Die Liste ueberlebt Neustarts.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Item {
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
    /// image | audio | video | doc | folder | other
    pub kind: String,
    pub added: u64,
}

static SHELF: Mutex<Vec<Item>> = Mutex::new(Vec::new());
static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn kind_of(ext: &str) -> &'static str {
    match ext {
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "ico" | "tif" | "tiff" | "avif" | "heic" => "image",
        "mp3" | "wav" | "m4a" | "flac" | "ogg" | "opus" | "aac" | "wma" => "audio",
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "wmv" | "m4v" => "video",
        "pdf" | "doc" | "docx" | "txt" | "md" | "rtf" | "odt" | "ppt" | "pptx" | "xls" | "xlsx" | "csv" => "doc",
        _ => "other",
    }
}

fn item(path: &Path) -> Option<Item> {
    let meta = std::fs::metadata(path).ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let kind = if meta.is_dir() { "folder" } else { kind_of(&ext) };
    Some(Item {
        path: path.to_string_lossy().to_string(),
        name,
        ext,
        size: if meta.is_dir() { 0 } else { meta.len() },
        kind: kind.into(),
        added: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
    })
}

fn save_and_emit(app: &AppHandle) {
    let items = SHELF.lock().unwrap().clone();
    if let Some(f) = FILE.lock().unwrap().as_ref() {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(f, serde_json::to_vec_pretty(&items).unwrap_or_default());
    }
    let _ = app.emit("shelf", items);
}

pub fn load(app: &AppHandle) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    let f = dir.join("shelf.json");
    if let Ok(bytes) = std::fs::read(&f) {
        if let Ok(v) = serde_json::from_slice::<Vec<Item>>(&bytes) {
            // Weg ist weg: Dateien, die es nicht mehr gibt, fliegen raus
            *SHELF.lock().unwrap() = v.into_iter().filter(|i| Path::new(&i.path).exists()).collect();
        }
    }
    *FILE.lock().unwrap() = Some(f);
}

pub fn add_one(app: &AppHandle, path: &Path) {
    if let Some(it) = item(path) {
        let mut s = SHELF.lock().unwrap();
        s.retain(|i| !i.path.eq_ignore_ascii_case(&it.path));
        s.insert(0, it);
        s.truncate(60);
    }
    save_and_emit(app);
}

#[tauri::command]
pub fn shelf_list() -> Vec<Item> {
    SHELF.lock().unwrap().clone()
}

#[tauri::command]
pub fn shelf_add(app: AppHandle, paths: Vec<String>) {
    {
        let mut s = SHELF.lock().unwrap();
        for p in paths.iter().rev() {
            if let Some(it) = item(Path::new(p)) {
                s.retain(|i| !i.path.eq_ignore_ascii_case(&it.path));
                s.insert(0, it);
            }
        }
        s.truncate(60);
    }
    save_and_emit(&app);
}

#[tauri::command]
pub fn shelf_remove(app: AppHandle, path: String) {
    SHELF.lock().unwrap().retain(|i| i.path != path);
    save_and_emit(&app);
}

#[tauri::command]
pub fn shelf_clear(app: AppHandle) {
    SHELF.lock().unwrap().clear();
    save_and_emit(&app);
}

#[tauri::command]
pub fn reveal(path: String) {
    crate::win::reveal(&path);
}

/// Symbol fuer das Rausziehen von Dateien (das Drag-Plugin braucht eine Bilddatei).
#[tauri::command]
pub fn drag_icon(app: AppHandle) -> Result<String, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let f = dir.join("drag.png");
    if !f.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(&f, include_bytes!("../icons/32x32.png")).map_err(|e| e.to_string())?;
    }
    Ok(f.to_string_lossy().to_string())
}
