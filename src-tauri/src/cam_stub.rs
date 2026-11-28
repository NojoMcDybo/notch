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

#[tauri::command]
pub fn cam_sources() -> Value {
    json!([])
}

#[tauri::command]
pub fn cam_live(_app: AppHandle, _key: String) -> Result<(), String> {
    Err("Die iPhone-Kamera gibt es nur unter Windows.".into())
}

#[tauri::command]
pub fn cam_config(_key: String, _patch: Value) -> Result<(), String> {
    Err("Die iPhone-Kamera gibt es nur unter Windows.".into())
}

pub fn preview_protocol(_req: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    responder.respond(tauri::http::Response::builder().status(404).body(Vec::new()).unwrap_or_default());
}

pub fn url(_app: &AppHandle) -> String {
    String::new()
}

pub fn status_json(_app: &AppHandle) -> Value {
    json!({ "supported": false })
}
