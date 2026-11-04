//! macOS-Gegenstueck zu win.rs: gleiche Namen, damit die uebrigen Module unveraendert bleiben.
//! Fensterverhalten (durchklicken, Ebene ueber der Menueleiste) regelt lib.rs ueber Tauri; hier nur, was die
//! Module brauchen. Fokus-Tricks von Windows (Fokus zurueckgeben, Fenster nach vorn zwingen) gibt es auf dem Mac
//! nicht — dort holt `open` eine App nach vorn.

use std::process::Command;

pub const CREATE_NO_WINDOW: u32 = 0;

/// Ersatz fuer std::os::windows::process::CommandExt: auf dem Mac oeffnet ein Kindprozess ohnehin kein Fenster.
pub trait CommandExt {
    fn creation_flags(&mut self, _flags: u32) -> &mut Self;
}
impl CommandExt for Command {
    fn creation_flags(&mut self, _flags: u32) -> &mut Self {
        self
    }
}

pub fn foreground() -> isize {
    0
}

pub fn set_foreground(_h: isize) {}

/// Linke Maustaste gerade gedrueckt? (auch ausserhalb unseres Fensters)
pub fn lbutton_down() -> bool {
    objc2_app_kit::NSEvent::pressedMouseButtons() & 1 != 0
}

/// shortcut: Vollbild-Apps liegen auf dem Mac in einem eigenen Space; die Notch bleibt dort einfach sichtbar.
/// Wer „Im Vollbild ausblenden“ auf dem Mac braucht: NSWorkspace activeSpace / CGWindowList auswerten.
pub fn fullscreen_foreground(_mon: (i32, i32, i32, i32)) -> bool {
    false
}

/// Durchklicken setzt lib.rs per Tauri (set_ignore_cursor_events) — hier nichts zu tun.
pub fn enforce(_raw: isize, _click_through: bool) {}

pub fn show_noactivate(_raw: isize) {}

pub fn focus_exe(_target: &str) -> bool {
    false
}

pub fn focus_title(_text: &str) -> bool {
    false
}

pub fn bring_to_front(_raw: isize) -> bool {
    false
}

pub fn allow_foreground_any() {}

pub fn process_running(name: &str) -> bool {
    Command::new("pgrep").args(["-xq", name.trim_end_matches(".exe")]).status().is_ok_and(|s| s.success())
}

pub fn close(_raw: isize) {}

pub struct Win {
    pub hwnd: isize,
    pub title: String,
    pub exe: String,
}

/// shortcut: Fensterliste braucht auf dem Mac die Bildschirmaufnahme-Freigabe; ohne sie bleibt die Liste leer.
pub fn top_windows() -> Vec<Win> {
    Vec::new()
}

/// Datei im Finder zeigen
pub fn reveal(path: &str) {
    let _ = Command::new("open").arg("-R").arg(path).spawn();
}
