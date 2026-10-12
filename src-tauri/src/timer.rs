//! Eingebauter Timer. Laeuft als ganz normale Activity (id "notch:timer") —
//! damit benutzt er genau die Schnittstelle, die auch deine Apps benutzen.
//!
//! Zwei Arten (eine zur Zeit):
//! - Timer: zaehlt runter, am Ende Ton + "Timer abgelaufen" (Notch bleibt offen und pulsiert).
//! - Pomodoro: Fokus und Pause im Wechsel. Ist eine Phase um, haelt er nicht an, sondern zaehlt die
//!   Extrazeit hoch ("+1:24"), bis man die naechste Phase selbst startet (Aktion "next").
//!   Die eigene Karte im Frontend liest das Feld `pomo` (Phase, Zeiten, Beginn der Extrazeit).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use tauri::AppHandle;
use windows::core::w;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC};

use crate::activities::{self, Action, Activity};

pub const ID: &str = "notch:timer";

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Work,
    Break,
}

struct Pomo {
    phase: Phase,
    /// Laenge von Fokus und Pause in Sekunden
    work: f64,
    brk: f64,
    /// laufende bzw. zuletzt beendete Fokus-Runde (ab 1)
    round: u32,
    /// Phase ist um: seitdem zaehlt die Extrazeit (Instant fuer Rust, ms seit 1970 fuers Frontend)
    over: Option<(Instant, u64)>,
}

struct Timer {
    total: f64,
    /// Some = laeuft; None = pausiert (dann gilt `left`)
    ends: Option<Instant>,
    left: f64,
    done: bool,
    shown: i64,
    pomo: Option<Pomo>,
}

static T: Mutex<Option<Timer>> = Mutex::new(None);

fn svg_url(svg: &str) -> String {
    format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(svg))
}

fn icon() -> String {
    svg_url(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#ffb340" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="13.5" r="7.5"/><path d="M12 13.5V9.5M10 2.5h4M18.5 6.5l1.5-1.5"/></svg>"##)
}

/// Schlicht wie die Karte: duenner Ring mit Kern — Pfirsich fuer Fokus, Mint fuer Pause
fn pomo_icon(phase: Phase) -> String {
    let c = match phase {
        Phase::Work => "#ffb09b",
        Phase::Break => "#8fdcbc",
    };
    svg_url(&format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8" fill="none" stroke="{c}" stroke-width="2.4" opacity=".35"/><path d="M12 4a8 8 0 0 1 8 8" fill="none" stroke="{c}" stroke-width="2.4" stroke-linecap="round"/><circle cx="12" cy="12" r="2.6" fill="{c}"/></svg>"##
    ))
}

fn fmt(s: f64) -> String {
    clock(s.ceil().max(0.0) as i64)
}

/// Extrazeit zaehlt hoch: abrunden, sonst stuende sofort "+0:01"
fn fmt_up(s: f64) -> String {
    clock(s.floor().max(0.0) as i64)
}

fn clock(s: i64) -> String {
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
    if let Some(p) = &t.pomo {
        publish_pomo(app, t, p, alert);
        return;
    }
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

fn publish_pomo(app: &AppHandle, t: &Timer, p: &Pomo, alert: bool) {
    let l = left(t);
    let running = t.ends.is_some();
    let work = p.phase == Phase::Work;
    let over = p.over.map(|(i, ms)| (i.elapsed().as_secs_f64(), ms));
    let (title, next_label) = match (work, over.is_some()) {
        (true, false) => ("Fokus", "Überspringen"),
        (true, true) => ("Fokus geschafft", "Pause starten"),
        (false, false) => ("Pause", "Überspringen"),
        (false, true) => ("Pause vorbei", "Weiter fokussieren"),
    };
    let subtitle = if over.is_some() {
        "Extrazeit".to_string()
    } else if !running {
        "pausiert".to_string()
    } else if work {
        format!("Runde {}", p.round)
    } else {
        "durchatmen".to_string()
    };
    let mut actions = Vec::new();
    if over.is_none() {
        actions.push(if running { a("pause", "Anhalten", "pause") } else { a("resume", "Weiter", "play") });
    }
    actions.push(a("next", next_label, if over.is_some() { "play" } else { "next" }));
    actions.push(a("stop", "Beenden", "stop"));
    let ends_at = if running { Some(activities::now_ms() + (l * 1000.0) as u64) } else { None };
    activities::upsert(
        app,
        Activity {
            id: ID.into(),
            app: "Pomodoro".into(),
            title: title.into(),
            subtitle: Some(subtitle),
            value: Some(match over {
                Some((s, _)) => format!("+{}", fmt_up(s)),
                None => fmt(l),
            }),
            icon: Some(pomo_icon(if over.is_some() { if work { Phase::Break } else { Phase::Work } } else { p.phase })),
            color: Some(if work { "#ffb09b".into() } else { "#8fdcbc".into() }),
            progress: Some(if over.is_some() { 0.0 } else if t.total > 0.0 { l / t.total } else { 0.0 }),
            ends_at,
            priority: 5,
            alert,
            actions,
            pomo: Some(serde_json::json!({
                "phase": if work { "work" } else { "break" },
                "over_since": over.map(|(_, ms)| ms),
                "total": t.total,
                "left": l,
                "running": running,
                "round": p.round,
                "work": p.work,
                "brk": p.brk,
                "ends_at": ends_at,
            })),
            ..Default::default()
        },
    );
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(200));
        let mut g = T.lock().unwrap();
        let Some(t) = g.as_mut() else { continue };
        // Pomodoro in der Extrazeit: jede neue Sekunde melden
        if let Some((since, _)) = t.pomo.as_ref().and_then(|p| p.over) {
            let s = since.elapsed().as_secs() as i64;
            if s != t.shown {
                t.shown = s;
                publish(&app, t, false);
            }
            continue;
        }
        if t.done || t.ends.is_none() {
            continue;
        }
        let l = left(t);
        if l <= 0.0 {
            t.ends = None;
            t.left = 0.0;
            if let Some(p) = t.pomo.as_mut() {
                // Phase um: nicht anhalten, Extrazeit zaehlen; Klang spielt die Karte (weicher als der Systemton)
                p.over = Some((Instant::now(), activities::now_ms()));
                t.shown = 0;
                publish(&app, t, true);
                continue;
            }
            t.done = true;
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
        pomo: None,
    };
    publish(app, &t, false);
    *T.lock().unwrap() = Some(t);
}

fn phase_timer(phase: Phase, work: f64, brk: f64, round: u32) -> Timer {
    let secs = if phase == Phase::Work { work } else { brk };
    Timer {
        total: secs,
        ends: Some(Instant::now() + Duration::from_secs_f64(secs)),
        left: secs,
        done: false,
        shown: secs.ceil() as i64,
        pomo: Some(Pomo { phase, work, brk, round, over: None }),
    }
}

pub fn start_pomo(app: &AppHandle, work: f64, brk: f64) {
    let t = phase_timer(Phase::Work, work, brk, 1);
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
        "next" => {
            // Pomodoro: naechste Phase jetzt (am Ende der Extrazeit oder vorzeitig)
            let Some(p) = g.as_ref().and_then(|t| t.pomo.as_ref()) else { return };
            let t = match p.phase {
                Phase::Work => phase_timer(Phase::Break, p.work, p.brk, p.round),
                Phase::Break => phase_timer(Phase::Work, p.work, p.brk, p.round + 1),
            };
            publish(app, &t, false);
            *g = Some(t);
            return;
        }
        _ => {}
    }
    let Some(t) = g.as_mut() else { return };
    if t.pomo.as_ref().is_some_and(|p| p.over.is_some()) {
        return; // Extrazeit: nur "next" oder "stop"
    }
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

/// Pomodoro mit eigener Fokus- und Pausenlaenge (Minuten)
#[tauri::command]
pub fn pomo_start(app: AppHandle, work: f64, brk: f64) {
    start_pomo(&app, work.clamp(1.0, 180.0) * 60.0, brk.clamp(1.0, 60.0) * 60.0);
}

#[cfg(test)]
mod tests {
    use super::{fmt, fmt_up};

    #[test]
    fn countdown_rundet_auf_extrazeit_ab() {
        assert_eq!(fmt(59.2), "1:00");
        assert_eq!(fmt_up(0.8), "0:00");
        assert_eq!(fmt_up(84.9), "1:24");
        assert_eq!(fmt_up(3725.0), "1:02:05");
    }
}
