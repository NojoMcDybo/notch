mod activities;
mod media;
mod win;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

/// Fenster ist eine unsichtbare Leinwand; die Notch wird darin gezeichnet.
const WIN_W: f64 = 600.0;
const WIN_H: f64 = 260.0;

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
        "port": activities::PORT,
        "fullscreen": FULLSCREEN.load(Ordering::Relaxed),
        "hover": HOVER.load(Ordering::Relaxed),
    })
}

// Fuer den Fall, dass das Frontend (neu) laedt, nachdem ein Event schon raus ist.
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static HOVER: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug)]
struct Geo {
    wx: i32,
    wy: i32,
    scale: f64,
    /// Monitor x, y, Breite, Hoehe (physisch)
    mon: (i32, i32, i32, i32),
}

/// Oben mittig auf den Hauptmonitor, buendig mit der Kante. Nur bewegen, wenn noetig.
fn place(w: &WebviewWindow, prev: Option<Geo>) -> Option<Geo> {
    let m = w.primary_monitor().ok()??;
    let s = m.scale_factor();
    let pw = (WIN_W * s).round() as i32;
    let ph = (WIN_H * s).round() as i32;
    let (mx, my) = (m.position().x, m.position().y);
    let (mw, mh) = (m.size().width as i32, m.size().height as i32);
    let g = Geo { wx: mx + (mw - pw) / 2, wy: my, scale: s, mon: (mx, my, mw, mh) };
    if prev.map_or(true, |p| p.wx != g.wx || p.wy != g.wy || p.scale != g.scale) {
        let _ = w.set_size(PhysicalSize::new(pw as u32, ph as u32));
        let _ = w.set_position(PhysicalPosition::new(g.wx, g.wy));
    }
    Some(g)
}

/// Maus abfragen statt auf Fenster-Events zu warten: das Fenster ist ja meist durchklickbar
/// und bekommt dann gar keine Mausereignisse. ~60x pro Sekunde, kostet praktisch nichts.
fn spawn_pointer(w: WebviewWindow, hwnd: isize) {
    std::thread::spawn(move || {
        let mut geo = place(&w, None).unwrap_or(Geo { wx: 0, wy: 0, scale: 1.0, mon: (0, 0, 1920, 1080) });
        let mut inside_prev = false;
        let mut fs_prev = false;
        let mut tick: u32 = 0;
        loop {
            win::enforce(hwnd, !inside_prev);
            if tick % 15 == 0 {
                let fs = win::fullscreen_foreground(geo.mon);
                if fs != fs_prev {
                    fs_prev = fs;
                    FULLSCREEN.store(fs, Ordering::Relaxed);
                    let _ = w.emit("fullscreen", fs);
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
                        let t = wy as f64 + r.y * s;
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
        .invoke_handler(tauri::generate_handler![
            set_hit_rect,
            snapshot,
            media::media_control,
            activities::dismiss_activity
        ])
        .setup(|app| {
            let w = app.get_webview_window("notch").expect("Fenster 'notch' fehlt");
            place(&w, None);
            let hwnd = w.hwnd().map(|h| h.0 as isize).unwrap_or(0);
            win::enforce(hwnd, true);
            win::show_noactivate(hwnd);

            let quit = MenuItem::with_id(app, "quit", "Notch beenden", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Notch")
                .menu(&menu)
                .on_menu_event(|app, e| {
                    if e.id().as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .build(app)?;

            media::spawn(app.handle().clone());
            activities::spawn(app.handle().clone());
            spawn_pointer(w, hwnd);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Notch konnte nicht starten");
}
