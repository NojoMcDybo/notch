//! Controller-Kuerzel (XInput — funktioniert auch, waehrend ein Spiel vorne ist; Xbox-Controller und
//! alles, was sich als solcher meldet: GameSir, Steam Input …). RB = R1, RT = R2, rechter Stick = R3.
//!
//!   Steuerkreuz links + RB   Notch klein heraus   } nur im Vollbild mit "Im Vollbild: Am Rand";
//!   Steuerkreuz links + RT   Notch aufgeklappt    } nochmal = weg, sonst nach 10 s von selbst
//!   Steuerkreuz links + R3   Sprachassistent: nach 1,5 s Halten hoert er zu, solange gedrueckt (immer)
//!
//! Schickt "pad" ("off" | "compact" | "expanded"), "voice-arm" (ms bis zum Zuhoeren, 0 = abgebrochen)
//! und "voice-ptt" (true/false) an die Notch.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use windows::Win32::UI::Input::XboxController::{
    XInputGetState, XINPUT_GAMEPAD_DPAD_LEFT, XINPUT_GAMEPAD_RIGHT_SHOULDER, XINPUT_GAMEPAD_RIGHT_THUMB, XINPUT_STATE,
};

const SHOW_FOR: Duration = Duration::from_secs(10);
/// ab hier zaehlt der Trigger als gedrueckt (0..255)
const TRIGGER: u8 = 100;
/// so lange muss Steuerkreuz links + R3 gehalten werden, bevor der Assistent zuhoert
/// (ein kurzer Druck im Spiel startet ihn nicht)
pub const TALK_AFTER: Duration = Duration::from_millis(1500);
/// ab hier zeigt die Notch den Ladering — ganz kurze Druecke bleiben unsichtbar
const ARM_AFTER: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Compact,
    Expanded,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Compact => "compact",
            Mode::Expanded => "expanded",
        }
    }
}

/// Was ist gerade gedrueckt? (Steuerkreuz links muss jeweils dabei sein)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pressed {
    pub peek: Mode,
    pub talk: bool,
}

pub fn read(buttons: u16, right_trigger: u8) -> Pressed {
    let left = buttons & XINPUT_GAMEPAD_DPAD_LEFT.0 != 0;
    let peek = if !left {
        Mode::Off
    } else if right_trigger >= TRIGGER {
        Mode::Expanded
    } else if buttons & XINPUT_GAMEPAD_RIGHT_SHOULDER.0 != 0 {
        Mode::Compact
    } else {
        Mode::Off
    };
    Pressed { peek, talk: left && buttons & XINPUT_GAMEPAD_RIGHT_THUMB.0 != 0 }
}

/// Umschalten bei jedem neuen Druck einer Kombination, Ablauf nach SHOW_FOR.
pub struct Toggle {
    mode: Mode,
    until: Option<Instant>,
    held: Mode,
}

impl Toggle {
    pub fn new() -> Self {
        Self { mode: Mode::Off, until: None, held: Mode::Off }
    }

    #[cfg(test)]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Aktuell gedrueckte Kombination; liefert den neuen Modus, falls er sich aendert.
    pub fn update(&mut self, pressed: Mode, now: Instant) -> Option<Mode> {
        let before = self.mode;
        // nur der Moment des Drueckens zaehlt (Festhalten schaltet nicht dauernd um);
        // RB -> RT nachdruecken wechselt von klein auf aufgeklappt
        if pressed != Mode::Off && pressed != self.held {
            self.mode = if self.mode == pressed { Mode::Off } else { pressed };
            self.until = (self.mode != Mode::Off).then(|| now + SHOW_FOR);
        }
        self.held = pressed;
        if self.until.is_some_and(|u| now >= u) {
            self.mode = Mode::Off;
            self.until = None;
        }
        (self.mode != before).then_some(self.mode)
    }

    pub fn reset(&mut self) -> Option<Mode> {
        let before = self.mode;
        *self = Self::new();
        (before != Mode::Off).then_some(Mode::Off)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Talk {
    /// Ladering zeigen: noch so viele ms bis zum Zuhoeren (0 = weg)
    Arm(u64),
    /// Zuhoeren an/aus
    Ptt(bool),
}

/// Halten zum Sprechen mit Vorlauf: erst nach TALK_AFTER geht es los, Loslassen davor bricht ab.
pub struct TalkGate {
    since: Option<Instant>,
    armed: bool,
    on: bool,
}

impl TalkGate {
    pub fn new() -> Self {
        Self { since: None, armed: false, on: false }
    }

    pub fn update(&mut self, held: bool, now: Instant) -> Vec<Talk> {
        let mut out = Vec::new();
        if !held {
            if self.armed {
                out.push(Talk::Arm(0));
            }
            if self.on {
                out.push(Talk::Ptt(false));
            }
            *self = Self::new();
            return out;
        }
        let t = now.saturating_duration_since(*self.since.get_or_insert(now));
        if self.on {
            return out;
        }
        if t >= TALK_AFTER {
            if self.armed {
                self.armed = false;
                out.push(Talk::Arm(0));
            }
            self.on = true;
            out.push(Talk::Ptt(true));
        } else if t >= ARM_AFTER && !self.armed {
            self.armed = true;
            out.push(Talk::Arm((TALK_AFTER - t).as_millis() as u64));
        }
        out
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut toggle = Toggle::new();
        let mut talk = TalkGate::new();
        let mut connected = [false; 4];
        let mut last_scan: Option<Instant> = None;
        loop {
            // nicht verbundene Plaetze nur alle 2 s abfragen (XInput ist dort langsam)
            let scan = last_scan.is_none_or(|t| t.elapsed() > Duration::from_secs(2));
            if scan {
                last_scan = Some(Instant::now());
            }
            let mut pressed = Pressed { peek: Mode::Off, talk: false };
            for (i, c) in connected.iter_mut().enumerate() {
                if !*c && !scan {
                    continue;
                }
                let mut st = XINPUT_STATE::default();
                *c = unsafe { XInputGetState(i as u32, &mut st) } == 0;
                if *c {
                    let p = read(st.Gamepad.wButtons.0, st.Gamepad.bRightTrigger);
                    if p.peek != Mode::Off {
                        pressed.peek = p.peek;
                    }
                    pressed.talk |= p.talk;
                }
            }

            // Sprachassistent: erst nach 1,5 s Halten zuhoeren, Loslassen weitergeben
            for t in talk.update(pressed.talk, Instant::now()) {
                let _ = match t {
                    Talk::Arm(ms) => app.emit("voice-arm", ms),
                    Talk::Ptt(on) => app.emit("voice-ptt", on),
                };
            }

            // Notch herausholen: nur im Vollbild mit "Am Rand"
            let peek_mode = crate::FULLSCREEN.load(Ordering::Relaxed) && crate::peek_enabled();
            let change = if peek_mode { toggle.update(pressed.peek, Instant::now()) } else { toggle.reset() };
            if let Some(m) = change {
                let _ = app.emit("pad", m.name());
            }

            // ohne Controller selten nachsehen
            let any = connected.iter().any(|&c| c);
            std::thread::sleep(Duration::from_millis(if any { 16 } else { 250 }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: u16 = XINPUT_GAMEPAD_DPAD_LEFT.0;
    const RB: u16 = XINPUT_GAMEPAD_RIGHT_SHOULDER.0;
    const R3: u16 = XINPUT_GAMEPAD_RIGHT_THUMB.0;

    #[test]
    fn kombinationen() {
        assert_eq!(read(LEFT | RB, 0).peek, Mode::Compact);
        assert_eq!(read(LEFT, 255).peek, Mode::Expanded);
        assert_eq!(read(LEFT | RB, 200).peek, Mode::Expanded, "RT gewinnt");
        assert_eq!(read(RB, 255).peek, Mode::Off, "ohne Steuerkreuz links nichts");
        assert_eq!(read(LEFT, 50).peek, Mode::Off, "Trigger nur angetippt");
        assert!(read(LEFT | R3, 0).talk);
        assert!(!read(R3, 0).talk, "R3 allein ist im Spiel oft belegt");
        assert_eq!(read(LEFT | R3, 0).peek, Mode::Off, "Sprechen holt nicht zusaetzlich die Notch");
    }

    #[test]
    fn druecken_schaltet_festhalten_nicht() {
        let t0 = Instant::now();
        let mut t = Toggle::new();
        assert_eq!(t.update(Mode::Compact, t0), Some(Mode::Compact));
        assert_eq!(t.update(Mode::Compact, t0 + Duration::from_millis(500)), None, "gehalten");
        assert_eq!(t.update(Mode::Off, t0 + Duration::from_secs(1)), None, "losgelassen");
        assert_eq!(t.update(Mode::Compact, t0 + Duration::from_secs(2)), Some(Mode::Off), "nochmal = weg");
    }

    #[test]
    fn klein_dann_aufgeklappt_und_ablauf() {
        let t0 = Instant::now();
        let mut t = Toggle::new();
        t.update(Mode::Compact, t0);
        assert_eq!(t.update(Mode::Expanded, t0 + Duration::from_millis(300)), Some(Mode::Expanded), "RT nachdruecken");
        t.update(Mode::Off, t0 + Duration::from_secs(1));
        assert_eq!(t.update(Mode::Off, t0 + Duration::from_secs(5)), None);
        assert_eq!(t.update(Mode::Off, t0 + Duration::from_millis(300) + SHOW_FOR), Some(Mode::Off), "nach 10 s weg");
        assert_eq!(t.mode(), Mode::Off);
        assert_eq!(t.reset(), None);
    }

    #[test]
    fn sprechen_erst_nach_anderthalb_sekunden() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut g = TalkGate::new();
        assert!(g.update(true, t0).is_empty(), "kurzer Druck: noch nichts");
        assert!(g.update(true, ms(100)).is_empty());
        assert_eq!(g.update(true, ms(300)), vec![Talk::Arm(1200)], "Ladering mit Restzeit");
        assert!(g.update(true, ms(800)).is_empty(), "Ring nur einmal");
        assert_eq!(g.update(true, ms(1500)), vec![Talk::Arm(0), Talk::Ptt(true)]);
        assert!(g.update(true, ms(4000)).is_empty(), "Halten bleibt an");
        assert_eq!(g.update(false, ms(4100)), vec![Talk::Ptt(false)]);
    }

    #[test]
    fn zu_frueh_losgelassen_bricht_ab() {
        let t0 = Instant::now();
        let mut g = TalkGate::new();
        assert!(g.update(true, t0).is_empty());
        assert!(g.update(false, t0 + Duration::from_millis(120)).is_empty(), "Antippen: gar nichts");
        g.update(true, t0 + Duration::from_millis(200));
        assert_eq!(g.update(true, t0 + Duration::from_millis(700)), vec![Talk::Arm(1000)]);
        assert_eq!(g.update(false, t0 + Duration::from_millis(900)), vec![Talk::Arm(0)], "Ring weg, kein Zuhoeren");
        // neuer Anlauf zaehlt von vorn
        g.update(true, t0 + Duration::from_millis(1000));
        assert!(!g.update(true, t0 + Duration::from_millis(2000)).contains(&Talk::Ptt(true)));
        assert!(g.update(true, t0 + Duration::from_millis(2500)).contains(&Talk::Ptt(true)));
    }
}
