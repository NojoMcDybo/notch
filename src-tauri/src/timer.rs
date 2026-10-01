//! Eingebauter Timer. Laeuft als ganz normale Activity (id "notch:timer") —
//! damit benutzt er genau die Schnittstelle, die auch deine Apps benutzen.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use tauri::AppHandle;
use windows::core::w;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC};

use crate::activities::{self, Action, Activity};

pub const ID: &str = "notch:timer";

struct Timer {
    total: f64,
    /// Some = laeuft; None = pausiert (dann gilt `left`)
    ends: Option<Instant>,
    left: f64,
    done: bool,
    shown: i64,
}

static T: Mutex<Option<Timer>> = Mutex::new(None);

fn icon() -> String {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#ffb340" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="13.5" r="7.5"/><path d="M12 13.5V9.5M10 2.5h4M18.5 6.5l1.5-1.5"/></svg>"##;
    format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(svg))
}

fn fmt(s: f64) -> String {
    let s = s.ceil().max(0.0) as i64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn left(t: &Timer) -> f64 {
    match t.ends {
        Some(e) => e.saturating_duration_since(Instant::now()).as_secs_f64(),
        None => t.left,
    }
}

fn a(id: &str, label: &str, icon: &str) -> Action {
    Action { id: id.into(), label: label.into(), icon: Some(icon.into()), ..Default::default() }
}

fn publish(app: &AppHandle, t: &Timer, alert: bool) {
    let l = left(t);
    let running = t.ends.is_some();
    let actions = if t.done {
        vec![a("restart", "Nochmal", "restart"), a("stop", "OK", "check")]
    } else {
        vec![
            if running { a("pause", "Pause", "pause") } else { a("resume", "Weiter", "play") },
            a("plus", "+1 min", ""),
            a("stop", "Stopp", "stop"),
        ]
    };
    activities::upsert(
        app,
        Activity {
            id: ID.into(),
            app: "Timer".into(),
            title: if t.done { "Timer abgelaufen".into() } else { "Timer".into() },
            subtitle: Some(if t.done { "".into() } else if running { "".into() } else { "pausiert".into() })
                .filter(|s: &String| !s.is_empty()),
            value: Some(fmt(l)),
            icon: Some(icon()),
            color: Some(if t.done { "#ff453a".into() } else { "#ffb340".into() }),
            progress: Some(if t.total > 0.0 { l / t.total } else { 0.0 }),
            // Frontend laesst den Balken damit pro Bildschirmbild weiterlaufen statt pro Sekunde
            ends_at: if running { Some(activities::now_ms() + (l * 1000.0) as u64) } else { None },
            priority: 5,
            alert,
            actions,
            ..Default::default()
        },
    );
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(200));
        let mut g = T.lock().unwrap();
        let Some(t) = g.as_mut() else { continue };
        if t.done || t.ends.is_none() {
            continue;
        }
        let l = left(t);
        if l <= 0.0 {
            t.done = true;
            t.ends = None;
            t.left = 0.0;
            publish(&app, t, true);
            unsafe {
                let _ = PlaySoundW(w!("SystemAsterisk"), None, SND_ALIAS | SND_ASYNC);
            }
        } else if l.ceil() as i64 != t.shown {
            t.shown = l.ceil() as i64;
            publish(&app, t, false);
        }
    });
}

pub fn start(app: &AppHandle, secs: f64) {
    let t = Timer {
        total: secs,
        ends: Some(Instant::now() + Duration::from_secs_f64(secs)),
        left: secs,
        done: false,
        shown: secs.ceil() as i64,
    };
    publish(app, &t, false);
    *T.lock().unwrap() = Some(t);
}

pub fn action(app: &AppHandle, action: &str) {
    let mut g = T.lock().unwrap();
    match action {
        "stop" => {
            *g = None;
            drop(g);
            activities::remove(app, ID);
            return;
        }
        "restart" => {
            let total = g.as_ref().map(|t| t.total).unwrap_or(60.0);
            drop(g);
            start(app, total);
            return;
        }
        _ => {}
    }
    let Some(t) = g.as_mut() else { return };
    match action {
        "pause" => {
            t.left = left(t);
            t.ends = None;
        }
        "resume" => {
            t.ends = Some(Instant::now() + Duration::from_secs_f64(t.left));
        }
        "plus" => {
            t.total += 60.0;
            match t.ends {
                Some(e) => t.ends = Some(e + Duration::from_secs(60)),
                None => t.left += 60.0,
            }
        }
        _ => return,
    }
    publish(app, t, false);
}

#[tauri::command]
pub fn timer_start(app: AppHandle, seconds: f64) {
    start(&app, seconds.clamp(1.0, 24.0 * 3600.0));
}
