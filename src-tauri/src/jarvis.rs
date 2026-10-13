//! Jarvis, Schritt M1 (docs/jarvis.md): PC-Zugriff fuer den Sprachassistenten.
//!
//! - Programme starten oder, wenn sie schon laufen, nach vorn holen; offene Fenster nach vorn holen; Seiten/Suchen oeffnen.
//! - Nur Stufe 1 (harmlos, umkehrbar). Gestartet wird ausschliesslich, was im App-Katalog steht (alles aus
//!   `shell:AppsFolder`, also Startmenue + Store-Apps), nie ein freier Pfad oder Befehl; Seiten nur http(s).
//! - Jede Aktion erscheint kurz als Zeile in der Notch (`notch:jarvis`).

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem, KF_FLAG_DEFAULT,
    SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};

use crate::activities::{self, Activity};
use crate::win;

pub const ID: &str = "notch:jarvis";

#[derive(Clone, Debug)]
pub struct App {
    /// wie im Startmenue, z. B. "Google Chrome"
    pub name: String,
    /// Parsing-Name in shell:AppsFolder: AUMID ("Spotify…!Spotify", "Chrome") oder Pfad mit Known-Folder-GUID
    pub id: String,
    key: String,
}

/// Katalog, eine Stunde gueltig
static CATALOG: LazyLock<Mutex<(Option<Instant>, Vec<App>)>> = LazyLock::new(Default::default);

/// Kleinbuchstaben, nur Buchstaben/Ziffern, Woerter mit einem Leerzeichen getrennt
fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn app(name: &str, id: &str) -> App {
    App { name: name.into(), id: id.into(), key: norm(name) }
}

/// Alle Eintraege von shell:AppsFolder (eigener Faden mit STA, wie der Explorer)
fn load_catalog() -> Vec<App> {
    std::thread::spawn(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut out = Vec::new();
        let Ok(folder) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) else { return out };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return out };
        loop {
            let mut got = [None];
            if items.Next(&mut got, None).is_err() {
                break;
            }
            let Some(it) = got[0].take() else { break };
            if let (Some(name), Some(id)) = (text(&it, SIGDN_NORMALDISPLAY), text(&it, SIGDN_PARENTRELATIVEPARSING)) {
                let k = norm(&name);
                // Deinstallationsprogramme und Hilfeseiten will niemand per Sprache starten
                if !k.is_empty() && !["deinstall", "uninstall", "entfernen"].iter().any(|w| k.contains(w)) {
                    out.push(app(&name, &id));
                }
            }
        }
        out
    })
    .join()
    .unwrap_or_default()
}

unsafe fn text(it: &IShellItem, sigdn: SIGDN) -> Option<String> {
    let p = it.GetDisplayName(sigdn).ok()?;
    let s = p.to_string().ok();
    windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
    s
}

pub fn catalog() -> Vec<App> {
    let mut g = CATALOG.lock().unwrap();
    if g.0.is_none_or(|t| t.elapsed() > Duration::from_secs(3600)) || g.1.is_empty() {
        *g = (Some(Instant::now()), load_catalog());
    }
    g.1.clone()
}

/// gesprochene Namen, die im Startmenue anders heissen
const ALIASES: &[(&str, &str)] = &[
    ("vs code", "visual studio code"),
    ("vscode", "visual studio code"),
    ("lol", "league of legends"),
    ("league", "league of legends"),
    ("datei explorer", "explorer"),
    ("dateiexplorer", "explorer"),
    ("taschenrechner", "rechner"),
    ("notepad", "editor"),
    ("browser", "chrome"),
];

/// Wie gut passt der gesprochene Name zum App-Namen? 0 = gar nicht
fn score(q: &str, key: &str) -> u32 {
    if q.is_empty() || key.is_empty() {
        return 0;
    }
    if key == q {
        return 100;
    }
    // ein, zwei Buchstaben (abgeschnittener Teiltext) treffen nur genau
    if q.chars().count() < 3 {
        return 0;
    }
    let words: Vec<&str> = key.split(' ').collect();
    // "chrome" in "google chrome", "studio code" in "visual studio code"
    if format!(" {key} ").contains(&format!(" {q} ")) {
        return if key.starts_with(q) { 90 } else { 80 };
    }
    let qw: Vec<&str> = q.split(' ').collect();
    if qw.iter().all(|w| words.iter().any(|k| k.starts_with(w))) {
        return 70;
    }
    if q.len() >= 4 && key.contains(q) {
        return 60;
    }
    0
}

/// Beste Treffer fuer einen gesprochenen Namen (hoechste Punktzahl, bei Gleichstand der kuerzere Name)
pub fn find<'a>(query: &str, apps: &'a [App]) -> Vec<(u32, &'a App)> {
    let mut q = norm(query);
    for (a, b) in ALIASES {
        if q == *a {
            q = (*b).into();
        }
    }
    let mut hits: Vec<(u32, &App)> = apps.iter().map(|a| (score(&q, &a.key), a)).filter(|h| h.0 > 0).collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.key.len().cmp(&b.1.key.len())));
    hits.truncate(5);
    hits
}

/// Programmname aus dem Parsing-Namen: "…\Notepad++\notepad++.exe" -> "notepad++.exe", "x!Spotify" -> "spotify.exe"
fn exe_of(id: &str) -> Option<String> {
    let l = id.to_lowercase();
    if l.ends_with(".exe") {
        return l.rsplit(['\\', '/']).next().map(String::from);
    }
    l.rsplit_once('!').map(|(_, app)| format!("{app}.exe"))
}

#[derive(Serialize)]
pub struct Done {
    pub app: String,
    /// "nach vorn" | "gestartet"
    pub aktion: String,
}

fn show(app: &AppHandle, title: &str, what: &str, icon_of: Option<&str>) {
    let icon = icon_of.and_then(|id| crate::thumb::render_now(&format!("shell:AppsFolder\\{id}"), 64));
    activities::upsert(
        app,
        Activity {
            id: ID.into(),
            app: "Jarvis".into(),
            title: title.into(),
            subtitle: Some(what.into()),
            icon: Some(icon.unwrap_or_else(|| "✓".into())),
            color: Some("#8fb8ff".into()),
            ttl: Some(6),
            priority: 1,
            ..Default::default()
        },
    );
}

/// Programm nach vorn holen, wenn es laeuft, sonst starten
pub fn open_app(handle: &AppHandle, name: &str) -> Result<Done, String> {
    let apps = catalog();
    let hits = find(name, &apps);
    let Some((_, a)) = hits.first() else {
        return Err(format!("Kein Programm „{name}“ gefunden."));
    };
    // laeuft schon? ueber den Programmnamen, sonst ueber den Fenstertitel ("… - Google Chrome")
    let running = exe_of(&a.id).is_some_and(|e| win::focus_exe(&e)) || (a.name.len() >= 4 && win::focus_title(&a.name));
    let aktion = if running {
        "nach vorn"
    } else {
        win::allow_foreground_any();
        std::process::Command::new("explorer.exe")
            .arg(format!("shell:AppsFolder\\{}", a.id))
            .spawn()
            .map_err(|e| format!("Start fehlgeschlagen: {e}"))?;
        "gestartet"
    };
    show(handle, &a.name, if running { "nach vorn geholt" } else { "geöffnet" }, Some(&a.id));
    Ok(Done { app: a.name.clone(), aktion: aktion.into() })
}

/// "Lissabon flug" -> Google-Suche; "github.com", "https://…" -> diese Seite. Nur http(s).
pub fn page_url(adresse: Option<&str>, suche: Option<&str>) -> Result<String, String> {
    if let Some(a) = adresse.map(str::trim).filter(|a| !a.is_empty()) {
        let url = if a.contains("://") { a.to_string() } else { format!("https://{a}") };
        let ok = (url.starts_with("https://") || url.starts_with("http://"))
            && !url.contains(char::is_whitespace)
            && url.split("://").nth(1).is_some_and(|h| h.split('/').next().is_some_and(|h| h.contains('.') || h.starts_with("localhost")));
        return if ok { Ok(url) } else { Err(format!("„{a}“ ist keine Webadresse.")) };
    }
    let q = suche.map(str::trim).filter(|q| !q.is_empty()).ok_or("Adresse oder Suche fehlt.")?;
    let enc: String = q
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    Ok(format!("https://www.google.com/search?q={enc}"))
}

// ---------- Befehle fuer den Sprachassistenten (voice.ts) ----------

#[derive(Serialize)]
pub struct Miss {
    fehler: String,
    /// naechste Treffer im Katalog bzw. offene Programme, damit das Modell nachfragen kann
    vorschlaege: Vec<String>,
}

#[tauri::command]
pub async fn jarvis_app(app: AppHandle, name: String) -> Result<Done, Miss> {
    tauri::async_runtime::spawn_blocking(move || {
        open_app(&app, &name).map_err(|fehler| {
            let apps = catalog();
            // Vorschlaege: Apps, in denen irgendein Wort der Anfrage vorkommt
            let words: Vec<String> = norm(&name).split(' ').filter(|w| w.len() >= 3).map(String::from).collect();
            let vorschlaege = apps.iter().filter(|a| words.iter().any(|w| a.key.contains(w.as_str()))).take(6).map(|a| a.name.clone()).collect();
            Miss { fehler, vorschlaege }
        })
    })
    .await
    .map_err(|e| Miss { fehler: e.to_string(), vorschlaege: vec![] })?
}

/// Offenes Fenster nach vorn holen (Titel oder Programmname)
#[tauri::command]
pub async fn jarvis_window(app: AppHandle, name: String) -> Result<Done, Miss> {
    tauri::async_runtime::spawn_blocking(move || {
        let q = norm(&name);
        let wins = win::top_windows();
        let best = wins
            .iter()
            .map(|w| (score(&q, &norm(&w.title)).max(score(&q, &norm(w.exe.trim_end_matches(".exe")))), w))
            .filter(|h| h.0 > 0)
            .max_by_key(|h| h.0);
        match best {
            Some((_, w)) if win::bring_to_front(w.hwnd) => {
                let label = w.exe.trim_end_matches(".exe").to_string();
                show(&app, &label, "nach vorn geholt", None);
                Ok(Done { app: label, aktion: "nach vorn".into() })
            }
            _ => {
                // nur Programmnamen, keine Fenstertitel (die koennen Privates enthalten)
                let mut names: Vec<String> = wins.iter().map(|w| w.exe.trim_end_matches(".exe").to_string()).collect();
                names.sort();
                names.dedup();
                Err(Miss { fehler: format!("Kein offenes Fenster „{name}“."), vorschlaege: names })
            }
        }
    })
    .await
    .map_err(|e| Miss { fehler: e.to_string(), vorschlaege: vec![] })?
}

#[tauri::command]
pub fn jarvis_page(app: AppHandle, adresse: Option<String>, suche: Option<String>) -> Result<String, String> {
    let url = page_url(adresse.as_deref(), suche.as_deref())?;
    win::allow_foreground_any();
    app.opener().open_url(&url, None::<&str>).map_err(|e| e.to_string())?;
    let label = suche.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| url.clone());
    show(&app, &label, "im Browser geöffnet", None);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Vec<App> {
        ["Google Chrome", "Google Chrome Canary", "Spotify", "Visual Studio Code", "Visual Studio Installer", "League of Legends",
         "Editor", "Discord", "Microsoft Edge", "Rechner", "Explorer", "Steam", "Steam Support Center"]
            .iter()
            .map(|n| app(n, n))
            .collect()
    }

    fn best(q: &str) -> Option<String> {
        let c = cat();
        find(q, &c).first().map(|h| h.1.name.clone())
    }

    #[test]
    fn gesprochene_namen_treffen() {
        assert_eq!(best("Chrome").as_deref(), Some("Google Chrome"));
        assert_eq!(best("google chrome").as_deref(), Some("Google Chrome"));
        assert_eq!(best("spotify").as_deref(), Some("Spotify"));
        assert_eq!(best("VS Code").as_deref(), Some("Visual Studio Code"));
        assert_eq!(best("visual studio code").as_deref(), Some("Visual Studio Code"));
        assert_eq!(best("League").as_deref(), Some("League of Legends"));
        assert_eq!(best("Steam").as_deref(), Some("Steam"));
        assert_eq!(best("taschenrechner").as_deref(), Some("Rechner"));
        assert_eq!(best("discord.").as_deref(), Some("Discord"));
        assert_eq!(best("Photoshop"), None);
        assert_eq!(best(""), None);
        // ein Buchstabe trifft nicht zufaellig irgendwas
        assert_eq!(best("e"), None);
    }

    #[test]
    fn programmname_aus_parsing_name() {
        assert_eq!(exe_of(r"{6D809377-6AF0-444B-8957-A3773F02200E}\Notepad++\notepad++.exe").as_deref(), Some("notepad++.exe"));
        assert_eq!(exe_of("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify").as_deref(), Some("spotify.exe"));
        assert_eq!(exe_of("Chrome"), None);
    }

    #[test]
    fn seiten_nur_http() {
        assert_eq!(page_url(Some("github.com"), None).unwrap(), "https://github.com");
        assert_eq!(page_url(Some("https://x.de/a?b=1"), None).unwrap(), "https://x.de/a?b=1");
        assert_eq!(page_url(None, Some("Flüge nach Lissabon")).unwrap(), "https://www.google.com/search?q=Fl%C3%BCge+nach+Lissabon");
        assert!(page_url(Some("file:///C:/Windows"), None).is_err());
        assert!(page_url(Some("javascript:alert(1)"), None).is_err());
        assert!(page_url(Some("cmd /c del"), None).is_err());
        assert!(page_url(None, None).is_err());
    }

    /// Echter Katalog dieses PCs: `cargo test --lib jarvis_katalog -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn jarvis_katalog() {
        let c = catalog();
        println!("{} Programme", c.len());
        for q in ["Chrome", "Spotify", "Discord", "VS Code", "League", "Explorer", "Editor", "Steam", "Haze", "Notch"] {
            let h = find(q, &c);
            println!("{q:>10} -> {:?}", h.iter().map(|(s, a)| format!("{} ({s}) [{}]", a.name, a.id)).collect::<Vec<_>>());
        }
        assert!(c.len() > 20);
    }
}
