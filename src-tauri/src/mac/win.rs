//! macOS-Gegenstueck zu win.rs: gleiche Namen, damit die uebrigen Module unveraendert bleiben.
//! Ein „Fenster-Handle“ ist hier die Prozessnummer (pid) der App: nach vorn holen, schliessen und Fokus
//! zurueckgeben gehen auf dem Mac ohnehin pro App. Fensterliste und Vollbild kommen aus CGWindowList
//! (Eigentuemer, Ebene, Groesse ohne Freigabe; Fenstertitel nur mit der Freigabe „Bildschirmaufnahme“).

use std::process::Command;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use objc2_core_foundation::CFString;
use objc2_core_graphics::{
    kCGWindowBounds, kCGWindowLayer, kCGWindowName, kCGWindowOwnerName, kCGWindowOwnerPID, CGDisplayBounds, CGMainDisplayID,
    CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

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

fn app_of(pid: isize) -> Option<Retained<NSRunningApplication>> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)
}

/// pid der App im Vordergrund (0 = keine)
pub fn foreground() -> isize {
    NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier() as isize).unwrap_or(0)
}

/// App nach vorn holen (auch die Notch selbst, z. B. zum Tippen)
pub fn set_foreground(pid: isize) {
    if let Some(a) = app_of(pid) {
        a.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}

/// Linke Maustaste gerade gedrueckt? (auch ausserhalb unseres Fensters)
pub fn lbutton_down() -> bool {
    objc2_app_kit::NSEvent::pressedMouseButtons() & 1 != 0
}

/// Ein Fenster aus CGWindowList (Koordinaten in Punkten, Ursprung oben links am Hauptbildschirm)
pub struct Info {
    pub pid: isize,
    pub owner: String,
    pub title: String,
    pub layer: i64,
    pub rect: (f64, f64, f64, f64),
}

fn key(k: &CFString) -> &NSString {
    // CFString und NSString sind gebuehrenfrei austauschbar
    unsafe { &*(k as *const CFString as *const NSString) }
}

fn num(d: &NSDictionary<NSString, AnyObject>, k: &NSString) -> Option<f64> {
    d.objectForKey(k)?.downcast::<NSNumber>().ok().map(|n| n.doubleValue())
}

fn text(d: &NSDictionary<NSString, AnyObject>, k: &NSString) -> String {
    d.objectForKey(k).and_then(|o| o.downcast::<NSString>().ok()).map(|s| s.to_string()).unwrap_or_default()
}

/// Sichtbare Fenster, vorderstes zuerst
pub fn window_list() -> Vec<Info> {
    let opts = CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(arr) = CGWindowListCopyWindowInfo(opts, 0) else { return Vec::new() };
    let arr: &NSArray<NSDictionary<NSString, AnyObject>> = unsafe { &*(&*arr as *const _ as *const NSArray<NSDictionary<NSString, AnyObject>>) };
    let (kb, kw, kh, kx, ky) = (unsafe { key(kCGWindowBounds) }, NSString::from_str("Width"), NSString::from_str("Height"), NSString::from_str("X"), NSString::from_str("Y"));
    arr.iter()
        .map(|d| {
            let b = d.objectForKey(kb).and_then(|o| o.downcast::<NSDictionary>().ok());
            let b: Option<&NSDictionary<NSString, AnyObject>> = b.as_deref().map(|b| unsafe { &*(b as *const NSDictionary as *const NSDictionary<NSString, AnyObject>) });
            let r = |k: &NSString| b.and_then(|b| num(b, k)).unwrap_or(0.0);
            Info {
                pid: num(&d, unsafe { key(kCGWindowOwnerPID) }).unwrap_or(0.0) as isize,
                owner: text(&d, unsafe { key(kCGWindowOwnerName) }),
                title: text(&d, unsafe { key(kCGWindowName) }),
                layer: num(&d, unsafe { key(kCGWindowLayer) }).unwrap_or(0.0) as i64,
                rect: (r(&kx), r(&ky), r(&kw), r(&kh)),
            }
        })
        .collect()
}

/// Liegt vorne eine App, deren vorderstes Fenster den ganzen Hauptbildschirm bedeckt (Vollbild-Space, Spiel)?
/// shortcut: prueft nur den Hauptbildschirm (dort sitzt die Notch); fuer Notch auf Zweitbildschirm `mon` umrechnen.
pub fn fullscreen_foreground(_mon: (i32, i32, i32, i32)) -> bool {
    let fg = foreground();
    if fg == 0 || fg == std::process::id() as isize {
        return false;
    }
    let screen = CGDisplayBounds(CGMainDisplayID());
    window_list()
        .into_iter()
        .find(|w| w.pid == fg && w.layer == 0)
        .is_some_and(|w| w.rect.0 <= screen.origin.x && w.rect.1 <= screen.origin.y && w.rect.2 >= screen.size.width && w.rect.3 >= screen.size.height)
}

/// Durchklicken setzt lib.rs per Tauri (set_ignore_cursor_events) — hier nichts zu tun.
pub fn enforce(_raw: isize, _click_through: bool) {}

pub fn show_noactivate(_raw: isize) {}

/// "Discord", "discord.exe", "/Applications/Discord.app" -> "discord"
fn plain(name: &str) -> String {
    let n = name.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(name).to_lowercase();
    n.trim_end_matches(".exe").trim_end_matches(".app").to_string()
}

/// Laufende App mit diesem Namen (Anzeigename oder Name der .app) nach vorn holen
pub fn focus_exe(target: &str) -> bool {
    let want = plain(target);
    let hit = NSWorkspace::sharedWorkspace().runningApplications().iter().find(|a| {
        a.localizedName().is_some_and(|n| plain(&n.to_string()) == want)
            || a.bundleURL().and_then(|u| u.path()).is_some_and(|p| plain(&p.to_string()) == want)
    });
    hit.is_some_and(|a| a.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows))
}

/// Fenster, dessen Titel `text` enthaelt, nach vorn (braucht die Freigabe „Bildschirmaufnahme“, sonst keine Titel)
pub fn focus_title(text: &str) -> bool {
    let t = text.to_lowercase();
    window_list().into_iter().find(|w| w.layer == 0 && w.title.to_lowercase().contains(&t)).is_some_and(|w| bring_to_front(w.pid))
}

pub fn bring_to_front(pid: isize) -> bool {
    app_of(pid).is_some_and(|a| a.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows))
}

pub fn allow_foreground_any() {}

/// Prozess- oder App-Name ("EpicGamesLauncher.exe" passt auch zu „Epic Games Launcher“)
pub fn process_running(name: &str) -> bool {
    let want = plain(name).replace(' ', "");
    NSWorkspace::sharedWorkspace().runningApplications().iter().any(|a| a.localizedName().is_some_and(|n| plain(&n.to_string()).replace(' ', "") == want))
        || Command::new("pgrep").args(["-xq", name.trim_end_matches(".exe")]).status().is_ok_and(|s| s.success())
}

/// App bitten, sich zu beenden (wie Cmd+Q: sie darf nachfragen)
pub fn close(pid: isize) {
    if let Some(a) = app_of(pid) {
        a.terminate();
    }
}

pub struct Win {
    pub hwnd: isize,
    pub title: String,
    pub exe: String,
}

/// Normale App-Fenster, vorderstes zuerst; ohne Notch selbst. `exe` ist der App-Name („Discord“).
pub fn top_windows() -> Vec<Win> {
    let me = std::process::id() as isize;
    window_list()
        .into_iter()
        .filter(|w| w.layer == 0 && w.pid != me && w.rect.2 > 50.0 && w.rect.3 > 50.0)
        .map(|w| Win { hwnd: w.pid, title: w.title, exe: w.owner })
        .collect()
}

/// Datei im Finder zeigen
pub fn reveal(path: &str) {
    let _ = Command::new("open").arg("-R").arg(path).spawn();
}

#[cfg(test)]
mod tests {
    #[test]
    fn namen() {
        assert_eq!(super::plain("/Applications/Discord.app"), "discord");
        assert_eq!(super::plain("Spotify.exe"), "spotify");
        assert_eq!(super::plain("Google Chrome"), "google chrome");
    }
}
