mod activities;
mod audio;
mod convert;
mod media;
mod open;
mod shelf;
mod timer;
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
const WIN_H: f64 = 400.0;

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
        "port": activities::PORT,
        "fullscreen": FULLSCREEN.load(Ordering::Relaxed),
        "hover": HOVER.load(Ordering::Relaxed),
        "dock": dock(),
    })
}

// Fuer den Fall, dass das Frontend (neu) laedt, nachdem ein Event schon raus ist.
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static HOVER: AtomicBool = AtomicBool::new(false);

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

fn load_config(app: &AppHandle) {
    let Some(f) = config_file(app) else { return };
    let Ok(bytes) = std::fs::read(f) else { return };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return };
    if let Some(d) = v.get("dock").and_then(|d| d.as_str()) {
        if matches!(d, "top" | "left" | "right") {
            *DOCK.lock().unwrap() = d.into();
        }
    }
}

fn save_config(app: &AppHandle) {
    let Some(f) = config_file(app) else { return };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(f, serde_json::json!({ "dock": dock() }).to_string());
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
                if last_fg != 0 && !DOCK_DRAG.load(Ordering::Relaxed) {
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
        .invoke_handler(tauri::generate_handler![
            set_hit_rect,
            snapshot,
            dock_set,
            dock_drag_start,
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

            let top = MenuItem::with_id(app, "dock:top", "Oben andocken", true, None::<&str>)?;
            let left = MenuItem::with_id(app, "dock:left", "Links andocken", true, None::<&str>)?;
            let right = MenuItem::with_id(app, "dock:right", "Rechts andocken", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Notch beenden", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&top, &left, &right, &sep, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Notch")
                .menu(&menu)
                .on_menu_event(|app, e| {
                    let id = e.id().as_ref();
                    if id == "quit" {
                        app.exit(0);
                    } else if let Some(d) = id.strip_prefix("dock:") {
                        set_dock(app, d);
                    }
                })
                .build(app)?;

            media::spawn(app.handle().clone());
            activities::spawn(app.handle().clone());
            timer::spawn(app.handle().clone());
            spawn_pointer(w, hwnd);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Notch konnte nicht starten");
}
