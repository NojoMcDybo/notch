//! Jarvis-Bildschirmsteuerung (lesen, klicken, tippen) gibt es vorerst nur unter Windows.
//! shortcut: auf dem Mac braeuchte das die Bedienungshilfen-Freigabe und die AX-Schnittstelle; bis dahin
//! antwortet jeder Befehl mit einem klaren Hinweis, damit der Assistent es dem Nutzer sagen kann.

use serde_json::Value;

type Reply = Result<Value, String>;

const MAC: &str = "Auf dem Mac kann die Notch den Bildschirm (noch) nicht lesen oder bedienen.";

pub fn spawn() {}

#[tauri::command]
pub async fn screen_read() -> Reply {
    Err(MAC.into())
}

#[tauri::command]
pub async fn screen_click(nummer: usize, bestaetigt: Option<bool>) -> Reply {
    let _ = (nummer, bestaetigt);
    Err(MAC.into())
}

#[tauri::command]
pub async fn screen_type(text: String, nummer: Option<usize>) -> Reply {
    let _ = (text, nummer);
    Err(MAC.into())
}

#[tauri::command]
pub async fn screen_key(tasten: String, bestaetigt: Option<bool>) -> Reply {
    let _ = (tasten, bestaetigt);
    Err(MAC.into())
}

pub fn browser_url() -> Result<String, String> {
    Err(MAC.into())
}

pub fn spotify_local(_art: &str, _was: &str, _zufall: bool) -> Reply {
    Err(MAC.into())
}

pub fn discord_join(_server: Option<String>, _kanal: String) -> Reply {
    Err(MAC.into())
}

#[tauri::command]
pub async fn screen_shot() -> Reply {
    Err(MAC.into())
}
