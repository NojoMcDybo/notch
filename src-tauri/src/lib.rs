mod activities;
mod audio;
mod clipboard;
mod convert;
mod media;
mod open;
mod shelf;
mod timer;
mod voice;
mod win;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

/// Fenster ist eine unsichtbare Leinwand; die Notch wird darin gezeichnet.
const WIN_W: f64 = 640.0;
const WIN_H: f64 = 560.0; // Platz fuer Player + Blutzucker-Graph + weitere Zeilen

/// Bereich der Notch im Fenster (CSS-Pixel), meldet das Frontend.
/// Nur dort faengt das Fenster die Maus ab, ueberall sonst klickt man durch.
#[derive(Clone, Copy, Deserialize)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}
static HIT: Mutex<Rect> = Mutex::new(Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 });

#[tauri::command]
fn set_hit_rect(rect: Rect) {
    *HIT.lock().unwrap() = rect;
}

#[tauri::command]
fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "media": media::last(),
        "cover": media::last_cover(),
        "activities": activities::list(),
        "shelf": shelf::shelf_list(),
        "clips": clipboard::list(),
        "port": activities::PORT,
        "fullscreen": FULLSCREEN.load(Ordering::Relaxed),
        "hover": HOVER.load(Ordering::Relaxed),
        "dock": dock(),
        "voice_key": VOICE_KEY.lock().unwrap().clone(),
    })
}

/// Tastenkuerzel fuer den Sprachassistenten, das sich registrieren liess
static VOICE_KEY: Mutex<String> = Mutex::new(String::new());

// Fuer den Fall, dass das Frontend (neu) laedt, nachdem ein Event schon raus ist.
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static HOVER: AtomicBool = AtomicBool::new(false);
/// Ein Eingabefeld in der Notch hat den Fokus -> Fenster darf vorne bleiben (Tastatur)
static KEYBOARD: AtomicBool = AtomicBool::new(false);
static HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Bis zu diesem Zeitpunkt (ms) gibt das Polling den Fokus nicht zurueck — damit eine App,
/// die per Doppelklick nach vorn geholt wird, nicht sofort wieder weggedrueckt wird.
static FOCUS_HOLD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Doppelklick auf einen Verlauf (z. B. Haze-Graph): die App soll nach vorn kommen.
/// Bewusste Ausnahme zu "ein Klick klaut nie den Fokus". Die Notch erlaubt dem App-Prozess,
/// sich selbst in den Vordergrund zu holen, und legt ein "open"-Ereignis in /events.
/// Die App pollt /events und macht Restore/Show/Activate selbst (klappt auch aus Tray/minimiert).
#[tauri::command]
fn activity_focus_app(id: String) {
    use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
    let pid = activities::get(&id).and_then(|a| a.pid).unwrap_or(u32::MAX); // MAX = ASFW_ANY
    unsafe {
        let _ = AllowSetForegroundWindow(pid);
    }
    FOCUS_HOLD.store(activities::now_ms() + 1200, Ordering::Relaxed);
    activities::push_event(&id, "open");
}

/// Tippen in der Notch: solange `on`, gibt das Polling den Fokus nicht zurueck.
#[tauri::command]
fn keyboard(on: bool) {
    KEYBOARD.store(on, Ordering::Relaxed);
    let h = HWND.load(Ordering::Relaxed);
    if on && h != 0 && win::foreground() != h {
        win::set_foreground(h);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Geo {
    wx: i32,
    wy: i32,
    ww: i32,
    wh: i32,
    scale: f64,
    /// Monitor x, y, Breite, Hoehe (physisch)
    mon: (i32, i32, i32, i32),
}

// ---------- Andocken: oben (Standard), links, rechts ----------

/// Fensterbreite beim seitlichen Andocken (logische Pixel); die Hoehe ist dann die ganze Monitorhoehe.
const SIDE_W: f64 = 440.0;
static DOCK: Mutex<String> = Mutex::new(String::new());
/// Fenster neu platzieren (nach Wechsel der Andockseite)
static REPLACE: AtomicBool = AtomicBool::new(false);
static DOCK_DRAG: AtomicBool = AtomicBool::new(false);
static MON: Mutex<(i32, i32, i32, i32)> = Mutex::new((0, 0, 1920, 1080));

fn dock() -> String {
    let d = DOCK.lock().unwrap().clone();
    if d.is_empty() { "top".into() } else { d }
}

fn config_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("config.json"))
}

/// Einstellungen der kompakten Anzeige (Rangliste, Extras). Das Schema gehoert dem Frontend
/// (src/settings-model.ts) — Rust speichert es nur und verteilt Aenderungen an alle Fenster.
static SETTINGS: Mutex<serde_json::Value> = Mutex::new(serde_json::Value::Null);

fn load_config(app: &AppHandle) {
    let Some(f) = config_file(app) else { return };
    let Ok(bytes) = std::fs::read(f) else { return };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return };
    if let Some(d) = v.get("dock").and_then(|d| d.as_str()) {
        if matches!(d, "top" | "left" | "right") {
            *DOCK.lock().unwrap() = d.into();
        }
    }
    if let Some(c) = v.get("compact").filter(|c| c.is_object()) {
        *SETTINGS.lock().unwrap() = c.clone();
    }
}

fn save_config(app: &AppHandle) {
    let Some(f) = config_file(app) else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let compact = SETTINGS.lock().unwrap().clone();
    let _ = std::fs::write(f, serde_json::json!({ "dock": dock(), "compact": compact }).to_string());
}

#[tauri::command]
fn settings_get() -> serde_json::Value {
    SETTINGS.lock().unwrap().clone()
}

/// Speichern und an alle Fenster schicken (Notch + Einstellungsfenster)
#[tauri::command]
fn settings_set(app: AppHandle, settings: serde_json::Value) {
    if !settings.is_object() {
        return;
    }
    *SETTINGS.lock().unwrap() = settings.clone();
    save_config(&app);
    let _ = app.emit("settings", settings);
}

/// Einstellungen in einem eigenen, normalen Fenster (nicht in der Notch).
/// async, weil Fenster aus synchronen Befehlen unter Windows haengen bleiben koennen.
#[tauri::command]
async fn open_settings(app: AppHandle) -> Result<(), String> {
    // Klick kam aus der Notch: Polling soll den Fokus nicht ans vorige Programm zurueckgeben,
    // waehrend das neue Fenster nach vorn kommt
    FOCUS_HOLD.store(activities::now_ms() + 1500, Ordering::Relaxed);
    show_settings(&app).map_err(|e| e.to_string())
}

fn show_settings(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    // gleiche Browser-Argumente wie das Notch-Fenster — sonst lehnt WebView2 die zweite
    // Umgebung ab (alle Fenster teilen sich einen Datenordner)
    let w = tauri::WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App("settings.html".into()))
        .title("Notch – Einstellungen")
        .inner_size(560.0, 780.0)
        .min_inner_size(460.0, 560.0)
        .center()
        .resizable(true)
        .maximizable(false)
        // keine Windows-Titelleiste: Schließen-Knopf und Ziehen übernimmt der Kopf der Seite
        // (data-tauri-drag-region); shadow gibt unter Windows 11 Schatten + runde Ecken zurück
        .decorations(false)
        .shadow(true)
        .background_color(tauri::window::Color(14, 20, 20, 255))
        .additional_browser_args(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required",
        )
        .build()?;
    let _ = w.set_focus();
    Ok(())
}

fn set_dock(app: &AppHandle, d: &str) {
    if !matches!(d, "top" | "left" | "right") {
        return;
    }
    *DOCK.lock().unwrap() = d.into();
    save_config(app);
    REPLACE.store(true, Ordering::Relaxed);
    let _ = app.emit("dock", d);
}

#[tauri::command]
fn dock_set(app: AppHandle, dock: String) {
    set_dock(&app, &dock);
}

/// Notch mit gedrueckter Maus ziehen: solange die Taste unten ist, meldet Rust,
/// welche Kante gerade am naechsten ist; beim Loslassen dockt die Notch dort an.
#[tauri::command]
fn dock_drag_start(app: AppHandle) {
    if DOCK_DRAG.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let (mx, _, mw, _) = *MON.lock().unwrap();
        let mut edge = dock();
        let _ = app.emit("dock-preview", Some(edge.clone()));
        loop {
            std::thread::sleep(Duration::from_millis(16));
            if let Some((cx, _)) = win::cursor() {
                let rel = (cx - mx) as f64 / mw.max(1) as f64;
                let e = if rel < 0.22 { "left" } else if rel > 0.78 { "right" } else { "top" };
                if e != edge {
                    edge = e.into();
                    let _ = app.emit("dock-preview", Some(edge.clone()));
                }
            }
            if !win::lbutton_down() {
                break;
            }
        }
        let _ = app.emit("dock-preview", None::<String>);
        if edge != dock() {
            set_dock(&app, &edge);
        }
        DOCK_DRAG.store(false, Ordering::Relaxed);
    });
}

/// Fenster an die gewaehlte Kante des Hauptmonitors legen. Nur bewegen, wenn noetig.
fn place(w: &WebviewWindow, prev: Option<Geo>) -> Option<Geo> {
    let m = w.primary_monitor().ok()??;
    let s = m.scale_factor();
    let (mx, my) = (m.position().x, m.position().y);
    let (mw, mh) = (m.size().width as i32, m.size().height as i32);
    *MON.lock().unwrap() = (mx, my, mw, mh);
    let d = dock();
    let (ww, wh) = if d == "top" {
        ((WIN_W * s).round() as i32, (WIN_H * s).round() as i32)
    } else {
        ((SIDE_W * s).round() as i32, mh)
    };
    let wx = match d.as_str() {
        "left" => mx,
        "right" => mx + mw - ww,
        _ => mx + (mw - ww) / 2,
    };
    let g = Geo { wx, wy: my, ww, wh, scale: s, mon: (mx, my, mw, mh) };
    if prev != Some(g) {
        let _ = w.set_size(PhysicalSize::new(ww as u32, wh as u32));
        let _ = w.set_position(PhysicalPosition::new(g.wx, g.wy));
    }
    Some(g)
}

/// Maus abfragen statt auf Fenster-Events zu warten: das Fenster ist ja meist durchklickbar
/// und bekommt dann gar keine Mausereignisse. ~60x pro Sekunde, kostet praktisch nichts.
fn spawn_pointer(w: WebviewWindow, hwnd: isize) {
    HWND.store(hwnd, Ordering::Relaxed);
    std::thread::spawn(move || {
        let mut geo = place(&w, None)
            .unwrap_or(Geo { wx: 0, wy: 0, ww: 640, wh: 400, scale: 1.0, mon: (0, 0, 1920, 1080) });
        let mut inside_prev = false;
        let mut fs_prev = false;
        let mut tick: u32 = 0;
        let mut last_fg: isize = 0;
        loop {
            win::enforce(hwnd, !inside_prev);
            // Klick auf die Notch soll dem aktuellen Programm nicht den Fokus klauen
            let fg = win::foreground();
            if fg == hwnd {
                let hold = activities::now_ms() < FOCUS_HOLD.load(Ordering::Relaxed);
                if last_fg != 0 && !DOCK_DRAG.load(Ordering::Relaxed) && !KEYBOARD.load(Ordering::Relaxed) && !hold {
                    win::set_foreground(last_fg);
                }
            } else if fg != 0 {
                last_fg = fg;
            }
            if tick % 15 == 0 {
                let fs = win::fullscreen_foreground(geo.mon);
                if fs != fs_prev {
                    fs_prev = fs;
                    FULLSCREEN.store(fs, Ordering::Relaxed);
                    let _ = w.emit("fullscreen", fs);
                }
            }
            if REPLACE.swap(false, Ordering::Relaxed) {
                if let Some(g) = place(&w, None) {
                    geo = g;
                }
            }
            if tick % 120 == 0 && tick > 0 {
                // Aufloesung / Monitore koennen sich aendern
                if let Some(g) = place(&w, Some(geo)) {
                    geo = g;
                }
            }
            tick = tick.wrapping_add(1);

            let (wx, wy, s) = (geo.wx, geo.wy, geo.scale);
            let inside = !fs_prev
                && match win::cursor() {
                    Some((cx, cy)) => {
                        let r = *HIT.lock().unwrap();
                        let pad = 6.0;
                        let l = wx as f64 + (r.x - pad) * s;
                        let rr = wx as f64 + (r.x + r.w + pad) * s;
                        let t = wy as f64 + (r.y - pad) * s;
                        let b = wy as f64 + (r.y + r.h + pad) * s;
                        let (cx, cy) = (cx as f64, cy as f64);
                        r.w > 0.0 && cx >= l && cx <= rr && cy >= t && cy <= b
                    }
                    None => false,
                };
            if inside != inside_prev {
                inside_prev = inside;
                win::enforce(hwnd, !inside);
                HOVER.store(inside, Ordering::Relaxed);
                let _ = w.emit("hover", inside);
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(
            // Strg+Alt+Leertaste: Sprachassistent an/aus — egal, welches Programm vorne ist
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, ev| {
                    if ev.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let _ = app.emit("voice-toggle", ());
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            set_hit_rect,
            snapshot,
            dock_set,
            dock_drag_start,
            settings_get,
            settings_set,
            open_settings,
            voice::voice_ready,
            voice::voice_setup,
            voice::voice_token,
            media::media_control,
            media::media_seek,
            media::media_focus,
            media::media_next_source,
            audio::volume_get,
            audio::volume_set,
            audio::volume_mute,
            activities::dismiss_activity,
            activities::activity_open,
            activities::activity_action,
            activities::activity_input,
            activity_focus_app,
            keyboard,
            clipboard::clip_list,
            clipboard::clip_copy,
            clipboard::clip_remove,
            clipboard::clip_clear,
            clipboard::clip_to_shelf,
            open::open,
            timer::timer_start,
            shelf::shelf_list,
            shelf::shelf_add,
            shelf::shelf_remove,
            shelf::shelf_clear,
            shelf::reveal,
            shelf::drag_icon,
            convert::convert_targets,
            convert::convert,
            convert::ffmpeg_available
        ])
        .setup(|app| {
            shelf::load(app.handle());
            load_config(app.handle());
            let w = app.get_webview_window("notch").expect("Fenster 'notch' fehlt");
            place(&w, None);
            let hwnd = w.hwnd().map(|h| h.0 as isize).unwrap_or(0);
            win::enforce(hwnd, true);
            win::show_noactivate(hwnd);
            let _ = w.with_webview(|wv| voice::allow_microphone(wv.controller()));
            {
                // Erstes freies Kuerzel nehmen (Strg+Alt+Leertaste belegt z. B. die Claude-App)
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                for (key, label) in [
                    ("ctrl+alt+space", "Strg+Alt+Leertaste"),
                    ("ctrl+shift+alt+space", "Strg+Umschalt+Alt+Leertaste"),
                    ("ctrl+alt+n", "Strg+Alt+N"),
                ] {
                    match app.global_shortcut().register(key) {
                        Ok(()) => {
                            *VOICE_KEY.lock().unwrap() = label.into();
                            break;
                        }
                        Err(e) => eprintln!("[notch] Kuerzel {label} belegt: {e}"),
                    }
                }
            }

            let settings = MenuItem::with_id(app, "settings", "Einstellungen …", true, None::<&str>)?;
            let sep0 = PredefinedMenuItem::separator(app)?;
            let top = MenuItem::with_id(app, "dock:top", "Oben andocken", true, None::<&str>)?;
            let left = MenuItem::with_id(app, "dock:left", "Links andocken", true, None::<&str>)?;
            let right = MenuItem::with_id(app, "dock:right", "Rechts andocken", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Notch beenden", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings, &sep0, &top, &left, &right, &sep, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Notch")
                .menu(&menu)
                .on_menu_event(|app, e| {
                    let id = e.id().as_ref();
                    if id == "quit" {
                        app.exit(0);
                    } else if id == "settings" {
                        // nicht direkt im Menue-Handler bauen (WebView2 haengt sonst, wry#583)
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = show_settings(&app);
                        });
                    } else if let Some(d) = id.strip_prefix("dock:") {
                        set_dock(app, d);
                    }
                })
                .build(app)?;

            media::spawn(app.handle().clone());
            activities::spawn(app.handle().clone());
            timer::spawn(app.handle().clone());
            audio::spawn_meter(app.handle().clone());
            spawn_pointer(w, hwnd);
            clipboard::spawn(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Notch konnte nicht starten");
}
