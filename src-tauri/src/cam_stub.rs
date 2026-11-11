//! iPhone-Kamera gibt es nur unter Windows (virtuelle Kamera per Media Foundation); hier nur leere Gegenstuecke.

use serde_json::{json, Value};
use tauri::AppHandle;

pub fn spawn(_app: AppHandle) {}

#[tauri::command]
pub fn cam_set(_app: AppHandle, _on: bool) -> Result<(), String> {
    Err("Die iPhone-Kamera gibt es nur unter Windows.".into())
}

#[tauri::command]
pub fn cam_setup(_app: AppHandle) -> Result<Value, String> {
    Err("Die iPhone-Kamera gibt es nur unter Windows.".into())
}

pub fn url(_app: &AppHandle) -> String {
    String::new()
}

pub fn status_json(_app: &AppHandle) -> Value {
    json!({ "supported": false })
}
