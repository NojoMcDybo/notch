//! Controller-Kuerzel (XInput — funktioniert auch, waehrend ein Spiel vorne ist; Xbox-Controller und
//! alles, was sich als solcher meldet: GameSir, Steam Input …). RB = R1, RT = R2, rechter Stick = R3.
//!
//!   Steuerkreuz links + RB   Notch klein heraus   } nur im Vollbild mit "Im Vollbild: Am Rand";
//!   Steuerkreuz links + RT   Notch aufgeklappt    } nochmal = weg, sonst nach 10 s von selbst
//!   Steuerkreuz links + R3   Sprachassistent: hoert zu, solange gedrueckt (immer)
//!
//! Schickt "pad" ("off" | "compact" | "expanded") und "voice-ptt" (true/false) an die Notch.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use windows::Win32::UI::Input::XboxController::{
    XInputGetState, XINPUT_GAMEPAD_DPAD_LEFT, XINPUT_GAMEPAD_RIGHT_SHOULDER, XINPUT_GAMEPAD_RIGHT_THUMB, XINPUT_STATE,
};

const SHOW_FOR: Duration = Duration::from_secs(10);
/// ab hier zaehlt der Trigger als gedrueckt (0..255)
const TRIGGER: u8 = 100;

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

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut toggle = Toggle::new();
        let mut talking = false;
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

            // Sprachassistent: Druecken und Loslassen weitergeben
            if pressed.talk != talking {
                talking = pressed.talk;
                let _ = app.emit("voice-ptt", talking);
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
}
