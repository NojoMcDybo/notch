//! Spotify ueber die offizielle Web-API: Jarvis kennt die eigenen Playlists und Lieblingssongs und spielt direkt ab,
//! ohne Suche in der App und ohne Klicken (Abspielen braucht Premium).
//!
//! Anmeldung: OAuth mit PKCE — kein geheimer Schluessel, nur die Client-ID einer eigenen Spotify-App
//! (developer.spotify.com, Redirect-URI `http://127.0.0.1:8738/callback`). Der Browser oeffnet die Spotify-Anmeldung,
//! die Antwort kommt an einen kurzlebigen lokalen Server (nur 127.0.0.1, prueft `state`). Das Erneuerungs-Token liegt
//! mit Windows verschluesselt (DPAPI) in `spotify.bin`, die Client-ID in `spotify.json`; Zugriffs-Token nur im Speicher.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::voice::{data_file, dpapi, store_as};

const PORT: u16 = 8738;
const REDIRECT: &str = "http://127.0.0.1:8738/callback";
const SCOPES: &str = "playlist-read-private playlist-read-collaborative user-library-read user-read-playback-state user-modify-playback-state user-read-currently-playing";
const API: &str = "https://api.spotify.com/v1";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

static ACCESS: Mutex<Option<(String, Instant)>> = Mutex::new(None);

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).http_status_as_error(false).build().into()
}

fn b64url(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn random(n: usize) -> String {
    let mut buf = vec![0u8; n];
    unsafe {
        let _ = windows::Win32::Security::Cryptography::BCryptGenRandom(None, &mut buf, windows::Win32::Security::Cryptography::BCRYPT_USE_SYSTEM_PREFERRED_RNG);
    }
    b64url(&buf)
}

/// PKCE: Pruefwert zum Geheimnis (SHA-256, base64url)
fn challenge(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn client_id(app: &AppHandle) -> Option<String> {
    let t = std::fs::read_to_string(data_file(app, "spotify.json")?).ok()?;
    serde_json::from_str::<Value>(&t).ok()?["client_id"].as_str().map(String::from)
}

fn refresh_token(app: &AppHandle) -> Option<String> {
    String::from_utf8(dpapi(&std::fs::read(data_file(app, "spotify.bin")?).ok()?, false).ok()?).ok()
}

/// Token-Antwort von Spotify auswerten: Zugriffs-Token merken, neues Erneuerungs-Token speichern
fn take_tokens(app: &AppHandle, v: &Value) -> Result<String, String> {
    let access = v["access_token"].as_str().ok_or_else(|| v["error_description"].as_str().unwrap_or("keine Anmeldung").to_string())?;
    let secs = v["expires_in"].as_u64().unwrap_or(3600);
    *ACCESS.lock().unwrap() = Some((access.to_string(), Instant::now() + Duration::from_secs(secs.saturating_sub(60))));
    if let Some(r) = v["refresh_token"].as_str() {
        store_as(app, "spotify.bin", r)?;
    }
    Ok(access.to_string())
}

fn token(app: &AppHandle) -> Result<String, String> {
    if let Some((t, until)) = ACCESS.lock().unwrap().clone() {
        if Instant::now() < until {
            return Ok(t);
        }
    }
    let (id, refresh) = client_id(app).zip(refresh_token(app)).ok_or("Spotify ist nicht verbunden (Einstellungen › App).")?;
    let mut r = agent()
        .post(TOKEN_URL)
        .send_form([("grant_type", "refresh_token"), ("refresh_token", refresh.as_str()), ("client_id", id.as_str())])
        .map_err(|e| e.to_string())?;
    let v: Value = r.body_mut().read_json().map_err(|e| e.to_string())?;
    take_tokens(app, &v)
}

/// Aufruf der Web-API; leere Antwort (204) = Null
fn api(app: &AppHandle, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let t = token(app)?;
    let url = if path.starts_with("http") { path.to_string() } else { format!("{API}{path}") };
    let auth = format!("Bearer {t}");
    let a = agent();
    let mut r = match (method, body) {
        ("GET", _) => a.get(&url).header("Authorization", &auth).call(),
        ("PUT", Some(b)) => a.put(&url).header("Authorization", &auth).send_json(b),
        ("PUT", None) => a.put(&url).header("Authorization", &auth).send_empty(),
        (_, Some(b)) => a.post(&url).header("Authorization", &auth).send_json(b),
        (_, None) => a.post(&url).header("Authorization", &auth).send_empty(),
    }
    .map_err(|e| format!("Spotify nicht erreichbar: {e}"))?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status >= 400 {
        let reason = v.pointer("/error/reason").and_then(|x| x.as_str()).unwrap_or("");
        return Err(match reason {
            "PREMIUM_REQUIRED" => "Abspielen geht über die Schnittstelle nur mit Spotify Premium.".into(),
            "NO_ACTIVE_DEVICE" => "Kein Spotify-Gerät aktiv.".into(),
            _ => format!("Spotify {status}: {}", v.pointer("/error/message").and_then(|x| x.as_str()).unwrap_or(&text)),
        });
    }
    Ok(v)
}

// ---------- Anmeldung ----------

#[derive(Serialize)]
pub struct Status {
    verbunden: bool,
    nutzer: String,
    client_id: String,
}

#[tauri::command]
pub async fn spotify_status(app: AppHandle) -> Status {
    tauri::async_runtime::spawn_blocking(move || {
        let id = client_id(&app).unwrap_or_default();
        let nutzer = if refresh_token(&app).is_some() {
            api(&app, "GET", "/me", None).ok().and_then(|v| v["display_name"].as_str().map(String::from)).unwrap_or_default()
        } else {
            String::new()
        };
        Status { verbunden: !nutzer.is_empty(), nutzer, client_id: id }
    })
    .await
    .unwrap_or(Status { verbunden: false, nutzer: String::new(), client_id: String::new() })
}

/// Im Browser bei Spotify anmelden (wartet hoechstens 3 Minuten auf die Rueckkehr)
#[tauri::command]
pub async fn spotify_connect(app: AppHandle, client_id: String) -> Result<Status, String> {
    let id = client_id.trim().to_string();
    if id.len() != 32 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Die Client-ID hat 32 Zeichen (0–9, a–f), zu finden im Spotify-Dashboard deiner App.".into());
    }
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let server = tiny_http::Server::http(("127.0.0.1", PORT)).map_err(|e| format!("Port {PORT} belegt: {e}"))?;
        let verifier = random(48);
        let state = random(16);
        let url = format!(
            "https://accounts.spotify.com/authorize?response_type=code&client_id={id}&scope={}&redirect_uri={}&code_challenge_method=S256&code_challenge={}&state={state}",
            enc(SCOPES),
            enc(REDIRECT),
            challenge(&verifier)
        );
        a.opener().open_url(&url, None::<&str>).map_err(|e| e.to_string())?;
        let until = Instant::now() + Duration::from_secs(180);
        let code = loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("Keine Antwort von Spotify (Anmeldung abgebrochen?).".to_string());
            }
            let Ok(Some(req)) = server.recv_timeout(left) else { continue };
            let q = req.url().split_once('?').map(|x| x.1.to_string()).unwrap_or_default();
            let param = |k: &str| q.split('&').find_map(|p| p.strip_prefix(&format!("{k}="))).map(String::from);
            if !req.url().starts_with("/callback") {
                let _ = req.respond(tiny_http::Response::empty(404));
                continue;
            }
            let ok = param("state").as_deref() == Some(state.as_str());
            let html = if ok && param("code").is_some() { "Notch ist mit Spotify verbunden. Du kannst dieses Fenster schließen." } else { "Anmeldung bei Spotify fehlgeschlagen." };
            let page = format!("<!doctype html><meta charset=utf-8><title>Notch</title><body style=\"font:16px system-ui;padding:40px\">{html}</body>");
            let _ = req.respond(tiny_http::Response::from_string(page).with_header(tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap()));
            if !ok {
                return Err("Antwort passte nicht zur Anmeldung.".to_string());
            }
            match param("code") {
                Some(c) => break c,
                None => return Err(format!("Spotify: {}", param("error").unwrap_or_else(|| "abgelehnt".into()))),
            }
        };
        let mut r = agent()
            .post(TOKEN_URL)
            .send_form([("grant_type", "authorization_code"), ("code", code.as_str()), ("redirect_uri", REDIRECT), ("client_id", id.as_str()), ("code_verifier", verifier.as_str())])
            .map_err(|e| e.to_string())?;
        let v: Value = r.body_mut().read_json().map_err(|e| e.to_string())?;
        let f = data_file(&a, "spotify.json").ok_or("kein App-Ordner")?;
        std::fs::write(f, json!({ "client_id": id }).to_string()).map_err(|e| e.to_string())?;
        take_tokens(&a, &v)?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(spotify_status(app).await)
}

#[tauri::command]
pub fn spotify_disconnect(app: AppHandle) -> Status {
    *ACCESS.lock().unwrap() = None;
    if let Some(f) = data_file(&app, "spotify.bin") {
        let _ = std::fs::remove_file(f);
    }
    Status { verbunden: false, nutzer: String::new(), client_id: client_id(&app).unwrap_or_default() }
}

// ---------- Fuer den Sprachassistenten ----------

fn norm(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Wie gut passt ein gesprochener Name zu einem Playlist-Namen? 0 = gar nicht
fn score(q: &str, name: &str) -> u32 {
    let (q, n) = (norm(q), norm(name));
    if q.is_empty() || n.is_empty() {
        return 0;
    }
    if q == n {
        return 100;
    }
    if n.starts_with(&q) || q.starts_with(&n) {
        return 80;
    }
    if n.contains(&q) || q.contains(&n) {
        return 60;
    }
    let qw: Vec<&str> = q.split(' ').filter(|w| w.len() > 2).collect();
    let hits = qw.iter().filter(|w| n.split(' ').any(|x| x == **w)).count();
    if !qw.is_empty() && hits * 2 >= qw.len() { 40 } else { 0 }
}

/// Eigene Playlists (Name, URI), hoechstens 200
fn playlists(app: &AppHandle) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let mut next = Some("/me/playlists?limit=50".to_string());
    while let Some(p) = next.take() {
        let v = api(app, "GET", &p, None)?;
        for it in v["items"].as_array().into_iter().flatten() {
            if let (Some(n), Some(u)) = (it["name"].as_str(), it["uri"].as_str()) {
                out.push((n.to_string(), u.to_string()));
            }
        }
        next = v["next"].as_str().filter(|_| out.len() < 200).map(String::from);
    }
    Ok(out)
}

#[tauri::command]
pub async fn spotify_library(app: AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let pls = playlists(&app)?;
        let liked = api(&app, "GET", "/me/tracks?limit=1", None)?["total"].as_u64().unwrap_or(0);
        Ok(json!({ "playlists": pls.into_iter().map(|p| p.0).collect::<Vec<_>>(), "lieblingssongs": liked }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Geraet zum Abspielen: das aktive, sonst dieser PC (Spotify-App starten, falls zu)
fn device(app: &AppHandle) -> Result<String, String> {
    let pick = |v: &Value| -> Option<String> {
        let list = v["devices"].as_array()?;
        list.iter()
            .find(|d| d["is_active"].as_bool() == Some(true))
            .or_else(|| list.iter().find(|d| d["type"].as_str() == Some("Computer")))
            .or_else(|| list.first())
            .and_then(|d| d["id"].as_str().map(String::from))
    };
    if let Some(id) = pick(&api(app, "GET", "/me/player/devices", None)?) {
        return Ok(id);
    }
    crate::jarvis::open_app(app, "Spotify").map_err(|e| format!("Spotify-App startet nicht: {e}"))?;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(750));
        if let Some(id) = pick(&api(app, "GET", "/me/player/devices", None)?) {
            return Ok(id);
        }
    }
    Err("Die Spotify-App meldet sich nicht als Gerät.".into())
}

/// Abspielen: `art` = playlist | lieblingssongs | song | kuenstler | album
#[tauri::command]
pub async fn spotify_play(app: AppHandle, art: String, was: Option<String>, zufall: Option<bool>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let was = was.unwrap_or_default();
        let (body, label) = match art.as_str() {
            "lieblingssongs" => {
                let me = api(&app, "GET", "/me", None)?;
                let id = me["id"].as_str().ok_or("Spotify-Konto unbekannt")?;
                (json!({ "context_uri": format!("spotify:user:{id}:collection") }), "Lieblingssongs".to_string())
            }
            "playlist" => {
                let best = playlists(&app)?.into_iter().map(|p| (score(&was, &p.0), p)).filter(|x| x.0 > 0).max_by_key(|x| x.0);
                match best {
                    Some((_, (n, u))) => (json!({ "context_uri": u }), n),
                    None => {
                        // keine eigene: oeffentliche Playlist suchen
                        let v = api(&app, "GET", &format!("/search?type=playlist&limit=1&q={}", enc(&was)), None)?;
                        let it = v.pointer("/playlists/items/0").filter(|x| !x.is_null()).ok_or(format!("Keine Playlist „{was}“ gefunden."))?;
                        (json!({ "context_uri": it["uri"] }), it["name"].as_str().unwrap_or(&was).to_string())
                    }
                }
            }
            kind @ ("song" | "kuenstler" | "album") => {
                let t = match kind { "song" => "track", "kuenstler" => "artist", _ => "album" };
                let v = api(&app, "GET", &format!("/search?type={t}&limit=1&q={}", enc(&was)), None)?;
                let it = v.pointer(&format!("/{t}s/items/0")).filter(|x| !x.is_null()).ok_or(format!("Nichts gefunden zu „{was}“."))?;
                let name = it["name"].as_str().unwrap_or(&was).to_string();
                let by = it.pointer("/artists/0/name").and_then(|x| x.as_str()).map(|a| format!(" – {a}")).unwrap_or_default();
                let body = if kind == "song" { json!({ "uris": [it["uri"]] }) } else { json!({ "context_uri": it["uri"] }) };
                (body, format!("{name}{by}"))
            }
            _ => return Err(format!("Unbekannte Art „{art}“.")),
        };
        let dev = device(&app)?;
        if let Some(z) = zufall {
            let _ = api(&app, "PUT", &format!("/me/player/shuffle?state={z}&device_id={dev}"), None);
        }
        api(&app, "PUT", &format!("/me/player/play?device_id={dev}"), Some(body))?;
        crate::memory::remember(&app, "musik", &format!("{art}: {label}"));
        Ok(json!({ "spielt": label }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_und_namen() {
        // Beispiel aus RFC 7636, Anhang B
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(random(48).len(), 64);
        assert_ne!(random(16), random(16));
        assert_eq!(score("lol", "LOL"), 100);
        assert!(score("cyberpunk", "Cyberpunk") > score("cyber", "Summer"));
        assert_eq!(score("summer", "Nico"), 0);
        assert!(score("meine lol playlist", "LOL") > 0);
        assert_eq!(enc("a b&c"), "a%20b%26c");
    }
}
