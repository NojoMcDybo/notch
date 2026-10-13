//! Jarvis, Schritt M1 (docs/jarvis.md): PC-Zugriff fuer den Sprachassistenten.
//!
//! - Programme starten oder, wenn sie schon laufen, nach vorn holen; offene Fenster nach vorn holen; Seiten/Suchen oeffnen.
//! - Nur Stufe 1 (harmlos, umkehrbar). Gestartet wird ausschliesslich, was im App-Katalog steht (alles aus
//!   `shell:AppsFolder`, also Startmenue + Store-Apps), nie ein freier Pfad oder Befehl; Seiten nur http(s).
//! - Jede Aktion erscheint kurz als Zeile in der Notch (`notch:jarvis`); was Jarvis neu gestartet hat, laesst sich
//!   dort (×) oder per Korrektur im Satz („… nein, Firefox“) rueckgaengig machen, solange es frisch ist.
//! - M2c: `jarvis_hint` findet im Teiltext einen sicher genannten Programmnamen (ganzer Name oder Alias), damit
//!   die Notch schon beim Sprechen starten kann (src/jarvis-live.ts entscheidet mit Laya, ob es ein Befehl ist).

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
    /// Parsing-Name in shell:AppsFolder: AUMID ("Spotify…!Spotify", "Chrome") oder Pfad mit Known-Folder-GUID;
    /// bei Spielen aus Epic/Steam die Start-Adresse ("steam://rungameid/…", "com.epicgames.launcher://apps/…")
    pub id: String,
    key: String,
    /// Programmdatei, wenn bekannt (Epic): Symbol und "laeuft schon?"
    exe: Option<String>,
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
    let name: String = name.chars().filter(|c| !matches!(c, '®' | '™' | '©')).collect();
    App { key: norm(&name), name: name.trim().into(), id: id.into(), exe: None }
}

/// nur Buchstaben, Ziffern, - und _ (Teile einer Start-Adresse aus fremden Dateien)
fn safe_part(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Spiele aus Epic und Steam: beide legen oft keinen Startmenue-Eintrag an (z. B. Rocket League). Gestartet wird ueber
/// die Adresse des Launchers; die Teile kommen aus deren Dateien und werden deshalb streng geprueft.
fn load_games() -> Vec<App> {
    let mut out = Vec::new();
    // Epic: ein JSON je installiertem Spiel
    let data = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    for e in std::fs::read_dir(format!(r"{data}\Epic\EpicGamesLauncher\Data\Manifests")).into_iter().flatten().flatten() {
        let Some(v) = std::fs::read_to_string(e.path()).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { continue };
        let f = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let (ns, item, id) = (f("CatalogNamespace"), f("CatalogItemId"), f("AppName"));
        if v["bIsApplication"].as_bool() == Some(false) || f("DisplayName").is_empty() || ![&ns, &item, &id].iter().all(|p| safe_part(p)) {
            continue;
        }
        let mut a = app(&f("DisplayName"), &format!("com.epicgames.launcher://apps/{ns}%3A{item}%3A{id}?action=launch&silent=true"));
        let exe = std::path::Path::new(&f("InstallLocation")).join(f("LaunchExecutable"));
        a.exe = exe.is_file().then(|| exe.to_string_lossy().into_owned());
        out.push(a);
    }
    // Steam: appmanifest_<id>.acf in jeder Bibliothek (libraryfolders.vdf).
    // shortcut: nur der Standard-Installationsort von Steam; liegt Steam woanders, die Registry (SteamPath) lesen.
    let steam = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into()) + r"\Steam";
    let libs = std::fs::read_to_string(format!(r"{steam}\steamapps\libraryfolders.vdf")).unwrap_or_default();
    let mut dirs: Vec<String> = vdf_values(&libs, "path").into_iter().map(|p| p.replace(r"\\", r"\")).collect();
    if dirs.is_empty() {
        dirs.push(steam);
    }
    for d in dirs {
        for e in std::fs::read_dir(format!(r"{d}\steamapps")).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !(n.starts_with("appmanifest_") && n.ends_with(".acf")) {
                continue;
            }
            let t = std::fs::read_to_string(e.path()).unwrap_or_default();
            let (Some(id), Some(name)) = (vdf_values(&t, "appid").into_iter().next(), vdf_values(&t, "name").into_iter().next()) else { continue };
            if id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() && !name.contains("Redistributable") {
                out.push(app(&name, &format!("steam://rungameid/{id}")));
            }
        }
    }
    out
}

/// Werte zu einem Schluessel im Valve-Textformat: Zeilen wie `"name"		"Cyberpunk 2077"`
fn vdf_values(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split('"').collect();
            // ["\t\t", key, "\t\t", value, ""]
            (parts.len() >= 5 && parts[1].eq_ignore_ascii_case(key)).then(|| parts[3].to_string())
        })
        .collect()
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
        let mut apps = load_catalog();
        // Spiele ergaenzen, die nicht schon im Startmenue stehen
        for game in load_games() {
            if !apps.iter().any(|a| a.key == game.key) {
                apps.push(game);
            }
        }
        *g = (Some(Instant::now()), apps);
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
    ("browser", "google chrome"),
];

/// Hersteller vor dem eigentlichen Namen: "Google Chrome" heisst gesprochen meist nur "Chrome"
const VENDORS: &[&str] = &["google", "microsoft", "adobe", "mozilla", "apple", "jetbrains", "epic games", "riot"];

/// Woerter, die zufaellig wie ein App-Name klingen, aber im Satz fast immer etwas anderes meinen
const NOT_NAMES: &[&str] = &["start", "einstellungen", "mail", "kamera", "uhr", "fotos", "kalender", "karten", "musik", "rechner", "editor"];

/// Gesprochene Namen einer App: der ganze Name, ohne Hersteller, plus Aliase
fn spoken(a: &App) -> Vec<String> {
    let mut v = vec![a.key.clone()];
    for vnd in VENDORS {
        if let Some(rest) = a.key.strip_prefix(&format!("{vnd} ")) {
            v.push(rest.to_string());
        }
    }
    v.extend(ALIASES.iter().filter(|(_, b)| *b == a.key).map(|(al, _)| al.to_string()));
    v
}

/// Programm, das im Satz sicher genannt ist: ganzer Name oder Alias als ganze Woerter. Bei mehreren das zuletzt
/// genannte ("Öffne Chrome, nein Firefox"), bei gleichem Ende der laengere Name ("Rocket League", nicht "League"). Woerter wie „Einstellungen“
/// zaehlen nicht — da wartet die Notch auf das Satzende.
pub fn mention<'a>(text: &str, apps: &'a [App]) -> Option<&'a App> {
    let t = format!(" {} ", norm(text));
    let mut best: Option<(usize, usize, &App)> = None;
    for a in apps {
        for n in spoken(a) {
            if n.len() < 3 || NOT_NAMES.contains(&n.as_str()) {
                continue;
            }
            // Ende des Namens im Satz: "Rocket League" und "League" enden gleich, dann gewinnt der laengere
            if let Some(end) = t.rfind(&format!(" {n} ")).map(|p| p + n.len()) {
                if best.is_none_or(|(be, bl, _)| end > be || (end == be && n.len() > bl)) {
                    best = Some((end, n.len(), a));
                }
            }
        }
    }
    best.map(|b| b.2)
}

/// Namen fuer die Mitschrift (`keywords` von gpt-live-transcribe): kurze, gesprochene Formen
pub fn keywords(apps: &[App], max: usize) -> Vec<String> {
    let mut v: Vec<String> = apps
        .iter()
        .filter_map(|a| {
            let lower = a.name.to_lowercase();
            let short = VENDORS.iter().find_map(|vnd| lower.starts_with(&format!("{vnd} ")).then(|| a.name[vnd.len() + 1..].to_string()));
            let n = short.unwrap_or_else(|| a.name.clone());
            let k = norm(&n);
            // die API lehnt Stichwoerter mit < > oder Zeilenumbruch ab
            let ok = n.split_whitespace().count() <= 3 && n.chars().count() <= 30 && k.len() >= 3 && !n.contains(['<', '>', '\r', '\n']);
            (ok && !NOT_NAMES.contains(&k.as_str()) && !["client", "launcher", "dienste", "store", "files"].contains(&k.as_str())).then_some(n)
        })
        .collect();
    // kurze, eigenstaendige Namen zuerst (die verhoert die Erkennung am ehesten)
    v.sort_by_key(|n| (n.split_whitespace().count(), n.to_lowercase()));
    v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    v.truncate(max);
    v
}

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

/// Was Jarvis zuletzt neu gestartet hat (fuer Rueckgaengig und gegen Doppelstarts)
struct Launch {
    id: String,
    name: String,
    exe: Option<String>,
    at: Instant,
}
static LAST: Mutex<Option<Launch>> = Mutex::new(None);
/// so lange gilt ein Start als frisch: nicht nochmal starten, Rueckgaengig erlaubt
const FRESH: Duration = Duration::from_secs(15);

#[derive(Serialize)]
pub struct Done {
    pub app: String,
    /// "nach vorn" | "gestartet"
    pub aktion: String,
}

impl Done {
    /// eben erst gestartet (Fenster kann noch fehlen)
    pub fn started(&self) -> bool {
        self.aktion == "gestartet"
    }
}

fn show(app: &AppHandle, title: &str, what: &str, icon_of: Option<&App>, undo: bool) {
    let icon = icon_of.and_then(|a| match (&a.exe, a.id.contains("://")) {
        (Some(exe), _) => crate::thumb::render_now(exe, 64),
        (None, false) => crate::thumb::render_now(&format!("shell:AppsFolder\\{}", a.id), 64),
        (None, true) => None,
    });
    let actions = if undo {
        vec![activities::Action { id: "undo".into(), label: "Rückgängig".into(), icon: Some("close".into()), ..Default::default() }]
    } else {
        vec![]
    };
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
            actions,
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
    // gerade erst gestartet (z. B. schon beim Sprechen), Fenster vielleicht noch nicht da: nicht doppelt starten
    if LAST.lock().unwrap().as_ref().is_some_and(|l| l.id == a.id && l.at.elapsed() < FRESH) {
        return Ok(Done { app: a.name.clone(), aktion: "gestartet".into() });
    }
    // laeuft schon? ueber den Programmnamen, sonst ueber den Fenstertitel ("… - Google Chrome")
    let exe = a.exe.as_deref().and_then(|p| exe_of(p)).or_else(|| exe_of(&a.id));
    let running = exe.as_deref().is_some_and(win::focus_exe) || (a.name.len() >= 4 && win::focus_title(&a.name));
    let aktion = if running {
        "nach vorn"
    } else {
        win::allow_foreground_any();
        if a.id.starts_with("com.epicgames.launcher://") && !win::process_running("EpicGamesLauncher.exe") {
            // Epic ignoriert die Start-Adresse, solange der Launcher nicht laeuft (getestet mit Rocket League):
            // erst den Launcher starten, dann die Adresse nachschicken
            epic_then(handle.clone(), a.id.clone())?;
        } else if a.id.contains("://") {
            // Spiele (Epic/Steam): Adresse ihres Launchers direkt an Windows (ShellExecute) — explorer.exe meldet
            // nichts zurueck, wenn es eine solche Adresse nicht weitergibt
            handle.opener().open_url(&a.id, None::<&str>).map_err(|e| format!("Start fehlgeschlagen: {e}"))?;
        } else {
            std::process::Command::new("explorer.exe")
                .arg(format!("shell:AppsFolder\\{}", a.id))
                .spawn()
                .map_err(|e| format!("Start fehlgeschlagen: {e}"))?;
        }
        *LAST.lock().unwrap() = Some(Launch { id: a.id.clone(), name: a.name.clone(), exe, at: Instant::now() });
        "gestartet"
    };
    show(handle, &a.name, if running { "nach vorn geholt" } else { "geöffnet" }, Some(a), !running);
    crate::memory::remember(handle, "programm", &a.name);
    Ok(Done { app: a.name.clone(), aktion: aktion.into() })
}

/// Epic-Launcher starten und die Spiel-Adresse schicken, sobald er laeuft (im Hintergrund, die Antwort kommt sofort).
/// shortcut: Standard-Installationsort des Launchers; liegt er woanders, den Pfad aus der Registry lesen
/// (HKCR\com.epicgames.launcher\shell\open\command).
fn epic_then(handle: AppHandle, uri: String) -> Result<(), String> {
    let pf = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let exe = format!(r"{pf}\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe");
    std::process::Command::new(&exe).spawn().map_err(|e| format!("Epic-Launcher startet nicht: {e}"))?;
    std::thread::spawn(move || {
        for _ in 0..60 {
            std::thread::sleep(Duration::from_secs(1));
            if win::process_running("EpicGamesLauncher.exe") {
                // laeuft, aber nimmt die Adresse erst nach dem Anmelden an
                std::thread::sleep(Duration::from_secs(8));
                let _ = handle.opener().open_url(&uri, None::<&str>);
                return;
            }
        }
    });
    Ok(())
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
    Ok(format!("https://www.google.com/search?q={}", enc(q)))
}

/// Text fuer eine Such-Adresse kodieren (Leerzeichen als +)
fn enc(q: &str) -> String {
    q.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Erste Video-ID auf einer YouTube-Ergebnisseite ("videoId":"<11 Zeichen>")
fn first_video(html: &str) -> Option<String> {
    html.match_indices("\"videoId\":\"").find_map(|(i, m)| {
        let id = html.get(i + m.len()..i + m.len() + 11)?;
        (id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && html[i + m.len() + 11..].starts_with('"')).then(|| id.to_string())
    })
}

/// Video zu einer Suche: erstes Ergebnis von YouTube (Ergebnisseite lesen, ohne API und Schluessel; SOCS=CAI lehnt
/// den Cookie-Dialog ab). Klappt das nicht, die Ergebnisseite selbst.
pub fn video_url(q: &str) -> String {
    let results = format!("https://www.youtube.com/results?search_query={}", enc(q));
    let id = ureq::get(&results)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36")
        .header("Accept-Language", "de-DE,de;q=0.9")
        .header("Cookie", "SOCS=CAI")
        .config()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .call()
        .ok()
        .and_then(|mut r| r.body_mut().read_to_string().ok())
        .and_then(|t| first_video(&t));
    id.map(|i| format!("https://www.youtube.com/watch?v={i}")).unwrap_or(results)
}

// ---------- Befehle fuer den Sprachassistenten (voice.ts) ----------

/// Video auf YouTube suchen und das erste Ergebnis abspielen
#[tauri::command]
pub async fn jarvis_video(app: AppHandle, suche: String) -> Result<String, String> {
    let q = suche.trim().to_string();
    if q.is_empty() {
        return Err("Suche fehlt.".into());
    }
    let url = tauri::async_runtime::spawn_blocking({
        let q = q.clone();
        move || video_url(&q)
    })
    .await
    .map_err(|e| e.to_string())?;
    win::allow_foreground_any();
    app.opener().open_url(&url, None::<&str>).map_err(|e| e.to_string())?;
    show(&app, &q, if url.contains("watch?v=") { "läuft auf YouTube" } else { "auf YouTube gesucht" }, None, false);
    Ok(url)
}

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
                show(&app, &label, "nach vorn geholt", None, false);
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
    show(&app, &label, "im Browser geöffnet", None, false);
    Ok(url)
}

/// Zuletzt gestartetes Programm wieder schliessen, wenn der Start frisch ist (WM_CLOSE, also wie das ×
/// am Fenster; das Programm darf nachfragen). Das Fenster kann noch am Aufgehen sein: ein paar Sekunden warten.
/// `name`: nur, wenn es genau dieses Programm war (vorab nur nach vorn geholt = nichts zu schliessen).
pub fn undo(app: &AppHandle, name: Option<&str>) -> bool {
    let Some(l) = LAST.lock().unwrap().take_if(|l| l.at.elapsed() < FRESH && name.is_none_or(|n| n == l.name)) else { return false };
    activities::remove(app, ID);
    std::thread::spawn(move || {
        for _ in 0..20 {
            // ueber den Programmnamen, sonst (Chrome heisst im Katalog nur "Chrome") ueber den Fenstertitel — lief
            // das Programm vorher nicht, sind alle seine Fenster die neuen
            let wins: Vec<_> = win::top_windows()
                .into_iter()
                .filter(|w| match &l.exe {
                    Some(e) => w.exe.eq_ignore_ascii_case(e),
                    None => l.name.len() >= 4 && w.title.ends_with(&l.name),
                })
                .collect();
            if !wins.is_empty() {
                wins.iter().for_each(|w| win::close(w.hwnd));
                return;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    });
    true
}

pub fn action(app: &AppHandle, action: &str) {
    if action == "undo" {
        undo(app, None);
    }
}

#[derive(Serialize)]
pub struct Hint {
    name: String,
}

/// Teiltext beim Sprechen: welches Programm ist sicher genannt? (Laya entscheidet danach, ob es ein Befehl ist)
#[tauri::command]
pub async fn jarvis_hint(text: String) -> Option<Hint> {
    tauri::async_runtime::spawn_blocking(move || mention(&text, &catalog()).map(|a| Hint { name: a.name.clone() })).await.ok().flatten()
}

#[tauri::command]
pub async fn jarvis_keywords() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| keywords(&catalog(), 100)).await.unwrap_or_default()
}

/// Korrektur im Satz („nein, Firefox“): das eben vorab gestartete Programm wieder schliessen
#[tauri::command]
pub fn jarvis_undo(app: AppHandle, name: String) -> bool {
    undo(&app, Some(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Vec<App> {
        ["Google Chrome", "Google Chrome Canary", "Spotify", "Visual Studio Code", "Visual Studio Installer", "League of Legends",
         "Editor", "Discord", "Microsoft Edge", "Rechner", "Explorer", "Steam", "Steam Support Center", "Rocket League®"]
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
    fn namen_im_satz() {
        let c = cat();
        let m = |t: &str| mention(t, &c).map(|a| a.name.clone());
        assert_eq!(m("Öffne Chrome, damit ich"), Some("Google Chrome".into()));
        assert_eq!(m("öffne google chrome"), Some("Google Chrome".into()));
        assert_eq!(m("Öffne Chr"), None, "abgeschnittener Name startet nichts");
        assert_eq!(m("mach mal vs code auf"), Some("Visual Studio Code".into()));
        assert_eq!(m("Öffne Steam, äh, ich meine Discord"), Some("Discord".into()), "das zuletzt Genannte");
        assert_eq!(m("öffne edge"), Some("Microsoft Edge".into()));
        assert_eq!(m("öffne den Editor"), None, "Allerweltswort: aufs Satzende warten");
        assert_eq!(m("starte League"), Some("League of Legends".into()));
        assert_eq!(m("starte Rocket League"), Some("Rocket League".into()));
        assert_eq!(m("Rocket League, nein, League of Legends"), Some("League of Legends".into()));
        assert_eq!(m("öffne youtube"), None);
        let k = keywords(&c, 60);
        assert!(k.contains(&"Chrome".to_string()) && k.contains(&"Spotify".to_string()), "{k:?}");
        assert!(!k.contains(&"Google Chrome".to_string()));
    }

    #[test]
    fn spiele_und_videos() {
        assert_eq!(app("Rocket League®", "x").name, "Rocket League");
        assert_eq!(app("Battlefield™ V", "x").key, "battlefield v");
        assert!(safe_part("530145df28a24424923f5828cc9031a1") && safe_part("Sugar"));
        assert!(!safe_part("a?b") && !safe_part("") && !safe_part("../x"));
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"1091500\"\n\t\"name\"\t\t\"Cyberpunk 2077\"\n}";
        assert_eq!(vdf_values(acf, "appid"), ["1091500"]);
        assert_eq!(vdf_values(acf, "name"), ["Cyberpunk 2077"]);
        let html = r#"..."videoId":"kurz","videoId":"kXYiU_JCYtU","videoId":"aaaaaaaaaaa""#;
        assert_eq!(first_video(html).as_deref(), Some("kXYiU_JCYtU"));
        assert_eq!(first_video("nichts"), None);
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
        for q in ["Chrome", "Spotify", "Discord", "VS Code", "League", "Explorer", "Editor", "Steam", "Haze", "Notch", "Rocket League", "Fall Guys", "Cyberpunk", "Sekiro", "Battlefield 1"] {
            let h = find(q, &c);
            println!("{q:>10} -> {:?}", h.iter().map(|(s, a)| format!("{} ({s}) [{}]", a.name, a.id)).collect::<Vec<_>>());
        }
        // Saetze aus tools/jarvis/saetze.json: welches Programm gilt als sicher genannt?
        let saetze: serde_json::Value = serde_json::from_str(include_str!("../../tools/jarvis/saetze.json")).unwrap();
        for t in saetze["saetze"].as_array().unwrap().iter().filter_map(|s| s["t"].as_str()) {
            println!("{:>22} | {t}", mention(t, &c).map(|a| a.name.as_str()).unwrap_or("-"));
        }
        println!("Stichwoerter: {:?}", keywords(&c, 100));
        println!("Video: {}", video_url("linkin park numb"));
        println!("Rocket im Satz: {:?}", mention("Starte Rocket League", &c).map(|a| (&a.name, &a.id, &a.exe)));
        assert!(c.len() > 20);
    }
}
