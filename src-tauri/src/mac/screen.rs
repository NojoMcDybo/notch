//! Bildschirm lesen und bedienen auf dem Mac (Teil von screen.rs): Bedienungshilfen (AXUIElement) fuer die
//! Elemente, CGEvent fuer Maus und Tastatur, `screencapture` fuers Bild, Vision fuer die Texterkennung.
//!
//! Freigaben (Systemeinstellungen › Datenschutz & Sicherheit): „Bedienungshilfen“ fuer Lesen/Klicken/Tippen
//! (macOS fragt beim ersten Mal), „Bildschirmaufnahme“ fuer Bild und Texterkennung. Ohne Freigabe kommt ein
//! klarer Hinweis zurueck, den der Assistent dem Nutzer sagt.

use std::ptr::NonNull;
use std::time::Duration;

use objc2_app_kit::NSRunningApplication;
use objc2_application_services::{kAXTrustedCheckOptionPrompt, AXCopyMultipleAttributeOptions, AXError, AXIsProcessTrustedWithOptions, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFRetained, CFString, CFType, CFURL, CGPoint, CGSize};
use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};
use serde_json::json;

use super::{read_into, url_like, Item, Raw, Reply, Target};
use crate::win;

pub type El = CFRetained<AXUIElement>;
/// Fenster = pid der App (gelesen wird ihr vorderes Fenster)
pub type Win = isize;

pub struct State {
    pub items: Vec<Item<El>>,
    pid: Option<isize>,
}

const NO_AX: &str = "Notch braucht dafür die Freigabe „Bedienungshilfen“: Systemeinstellungen › Datenschutz & Sicherheit › Bedienungshilfen › Notch einschalten. (Nach einem Update ggf. aus- und wieder einschalten.)";

/// Bedienungshilfen freigegeben? Beim ersten Mal zeigt macOS selbst den Hinweis mit Link in die Einstellungen.
fn trusted() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    // nur beim ersten Mal mit Hinweis von macOS, danach still pruefen (sonst stapeln sich die Dialoge)
    static ASKED: AtomicBool = AtomicBool::new(false);
    if ASKED.swap(true, Ordering::Relaxed) {
        return unsafe { objc2_application_services::AXIsProcessTrusted() };
    }
    let key: &NSString = unsafe { &*(kAXTrustedCheckOptionPrompt as *const CFString as *const NSString) };
    let opts = NSDictionary::from_slices(&[key], &[&*NSNumber::new_bool(true)]);
    let opts: &CFDictionary = unsafe { &*(&*opts as *const NSDictionary<NSString, NSNumber> as *const CFDictionary) };
    unsafe { AXIsProcessTrustedWithOptions(Some(opts)) }
}

/// Die App, um die es geht: die vordere, ausser es ist die Notch selbst
pub fn front_window() -> Option<Win> {
    let fg = win::foreground();
    if fg != 0 && fg != std::process::id() as isize {
        return Some(fg);
    }
    win::top_windows().first().map(|w| w.hwnd)
}

/// Laufende App nach Namen ("discord", "spotify")
pub fn app_window(app: &str) -> Option<Win> {
    let want = app.to_lowercase();
    win::top_windows().into_iter().find(|w| w.exe.to_lowercase() == want).map(|w| w.hwnd)
}

pub fn bring(pid: Win) {
    win::bring_to_front(pid);
}

pub fn open_uri(uri: &str) {
    let _ = std::process::Command::new("open").arg(uri).spawn();
}

// ---------- AX-Hilfen ----------

fn cfs(s: &'static str) -> CFRetained<CFString> {
    CFString::from_static_str(s)
}

fn attr(e: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let mut v: *const CFType = std::ptr::null();
    let err = unsafe { e.copy_attribute_value(&cfs(name), NonNull::from(&mut v)) };
    (err == AXError::Success).then(|| NonNull::new(v as *mut CFType)).flatten().map(|p| unsafe { CFRetained::from_raw(p) })
}

fn text_of(v: Option<&CFType>) -> String {
    v.and_then(|v| v.downcast_ref::<CFString>()).map(|s| s.to_string()).unwrap_or_default()
}

fn elements(v: Option<&CFType>) -> Vec<El> {
    match v.and_then(|v| v.downcast_ref::<CFArray>()) {
        // AXChildren ist ein Array von AXUIElement
        Some(a) => unsafe { &*(a as *const CFArray as *const CFArray<AXUIElement>) }.to_vec(),
        None => Vec::new(),
    }
}

/// Rahmen eines Elements in Bildschirmpunkten (oben links)
fn frame(e: &AXUIElement) -> Option<(f64, f64, f64, f64)> {
    let mut p = CGPoint { x: 0.0, y: 0.0 };
    let mut s = CGSize { width: 0.0, height: 0.0 };
    let pv = attr(e, "AXPosition")?.downcast::<AXValue>().ok()?;
    let sv = attr(e, "AXSize")?.downcast::<AXValue>().ok()?;
    unsafe {
        (pv.value(AXValueType::CGPoint, NonNull::from(&mut p).cast()) && sv.value(AXValueType::CGSize, NonNull::from(&mut s).cast())).then_some((p.x, p.y, s.width, s.height))
    }
}

fn role(ax: &str, sub: &str) -> Option<&'static str> {
    Some(match ax {
        "AXButton" | "AXMenuButton" | "AXDisclosureTriangle" => "Knopf",
        "AXLink" => "Link",
        "AXTextField" | "AXTextArea" | "AXSearchField" | "AXSecureTextField" => "Eingabefeld",
        "AXComboBox" | "AXPopUpButton" => "Auswahl",
        "AXCheckBox" if sub == "AXSwitch" => "Kästchen",
        "AXCheckBox" => "Kästchen",
        "AXRadioButton" if sub == "AXTabButton" => "Reiter",
        "AXRadioButton" => "Option",
        "AXMenuItem" | "AXMenuBarItem" => "Menüpunkt",
        "AXTab" => "Reiter",
        "AXRow" | "AXCell" | "AXOutlineRow" => "Eintrag",
        "AXSlider" => "Regler",
        "AXStaticText" => "Text",
        _ => return None,
    })
}

/// Vorderes Fenster einer App
fn window_of(pid: isize) -> Option<El> {
    let app = unsafe { AXUIElement::new_application(pid as i32) };
    unsafe {
        let _ = app.set_messaging_timeout(1.5);
        // Chromium/Electron (Discord, Spotify, Chrome) bauen ihren Baum fuer Bedienungshilfen erst auf Nachfrage
        let _ = app.set_attribute_value(&cfs("AXManualAccessibility"), CFBoolean::new(true));
    }
    let w = attr(&app, "AXFocusedWindow").or_else(|| attr(&app, "AXMainWindow"))?;
    w.downcast::<AXUIElement>().ok().or_else(|| elements(attr(&app, "AXWindows").as_deref()).into_iter().next())
}

/// Alle Elemente unter `root` (Tiefensuche, Reihenfolge wie auf dem Bildschirm), je mit einem Sammelaufruf
fn walk(root: El, mut each: impl FnMut(El, &[Option<CFRetained<CFType>>]) -> bool) {
    const ATTRS: [&str; 7] = ["AXRole", "AXSubrole", "AXTitle", "AXDescription", "AXValue", "AXEnabled", "AXChildren"];
    let names: Vec<CFRetained<CFString>> = ATTRS.iter().map(|a| cfs(a)).collect();
    let names = CFArray::from_retained_objects(&names);
    let mut stack = vec![(root, 0u8)];
    let mut seen = 0usize;
    while let Some((e, depth)) = stack.pop() {
        seen += 1;
        // Riesige Baeume (lange Chats, Tabellen) nicht ganz ablaufen
        if seen > 4000 {
            break;
        }
        let mut out: *const CFArray = std::ptr::null();
        let err = unsafe { e.copy_multiple_attribute_values(names.as_opaque(), AXCopyMultipleAttributeOptions(0), NonNull::from(&mut out)) };
        let Some(vals) = (err == AXError::Success).then(|| NonNull::new(out as *mut CFArray)).flatten().map(|p| unsafe { CFRetained::from_raw(p) }) else { continue };
        // Werte (oder AXValue mit Fehlercode, wenn ein Element ein Attribut nicht hat)
        let vals: &CFArray<CFType> = unsafe { &*(&*vals as *const CFArray as *const CFArray<CFType>) };
        let vals: Vec<Option<CFRetained<CFType>>> = vals.to_vec().into_iter().map(Some).collect();
        let kids = elements(vals.get(6).and_then(|v| v.as_deref()));
        if !each(e, &vals) {
            break;
        }
        if depth < 60 {
            // umgekehrt auf den Stapel, damit das erste Kind zuerst drankommt
            stack.extend(kids.into_iter().rev().map(|k| (k, depth + 1)));
        }
    }
}

impl State {
    pub fn new() -> Result<Self, String> {
        Ok(State { items: Vec::new(), pid: None })
    }

    pub fn read(&mut self, pid: Win) -> Reply {
        if !trusted() {
            return Err(NO_AX.into());
        }
        self.pid = Some(pid);
        let program = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32).and_then(|a| a.localizedName()).map(|n| n.to_string()).unwrap_or_default();
        let win = window_of(pid);
        let title = win.as_deref().map(|w| text_of(attr(w, "AXTitle").as_deref())).unwrap_or_default();
        let mut found = Vec::new();
        if let Some(w) = win.clone() {
            walk(w, |e, v| {
                let t = |i: usize| text_of(v.get(i).and_then(|x| x.as_deref()));
                let (ax, sub) = (t(0), t(1));
                let enabled = v.get(5).and_then(|x| x.as_deref()).and_then(|x| x.downcast_ref::<CFBoolean>()).is_none_or(|b| b.as_bool());
                if let (Some(r), true) = (role(&ax, &sub), enabled) {
                    let password = ax == "AXSecureTextField";
                    let mut name = t(2);
                    if name.is_empty() {
                        name = t(3);
                    }
                    if name.is_empty() && r == "Text" {
                        name = t(4);
                    }
                    let value = if !password && matches!(r, "Eingabefeld" | "Auswahl") { super::short(&t(4), 60) } else { String::new() };
                    found.push(Raw { el: e, role: r, name: super::short(&name, 80), value, password });
                }
                found.len() < super::MAX_ITEMS * 3
            });
        }
        read_into(&mut self.items, found.into_iter(), || win.as_deref().and_then(ocr_lines), &title, &program)
    }

    /// Element ausloesen (AXPress, Zeilen auswaehlen), sonst Mausklick auf die Mitte
    pub fn press(&mut self, n: usize) -> Result<(), String> {
        let it = super::item(&self.items, n)?;
        let point = match &it.target {
            Target::Point(x, y) => (*x as f64, *y as f64),
            Target::El(e) => {
                if unsafe { e.perform_action(&cfs("AXPress")) } == AXError::Success {
                    return Ok(());
                }
                if it.role == "Eintrag" && unsafe { e.set_attribute_value(&cfs("AXSelected"), CFBoolean::new(true)) } == AXError::Success {
                    return Ok(());
                }
                let (x, y, w, h) = frame(e).ok_or("Element hat keine Position.")?;
                (x + w / 2.0, y + h / 2.0)
            }
        };
        self.front();
        let at = CGPoint { x: point.0, y: point.1 };
        for t in [CGEventType::LeftMouseDown, CGEventType::LeftMouseUp] {
            let ev = CGEvent::new_mouse_event(None, t, at, CGMouseButton::Left);
            CGEvent::post(CGEventTapLocation::HIDEventTap, ev.as_deref());
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }

    /// Feld fokussieren und den Text direkt setzen; false = geht nicht (dann tippt screen.rs)
    pub fn set_value(&mut self, n: usize, text: &str) -> bool {
        let Ok(Item { target: Target::El(e), .. }) = super::item(&self.items, n) else { return false };
        unsafe {
            let _ = e.set_attribute_value(&cfs("AXFocused"), CFBoolean::new(true));
            e.set_attribute_value(&cfs("AXValue"), &CFString::from_str(text)) == AXError::Success
        }
    }

    pub fn focus_is_password(&self) -> bool {
        let sys = unsafe { AXUIElement::new_system_wide() };
        attr(&sys, "AXFocusedUIElement")
            .and_then(|f| f.downcast::<AXUIElement>().ok())
            .is_some_and(|f| text_of(attr(&f, "AXRole").as_deref()) == "AXSecureTextField")
    }

    pub fn type_keys(&mut self, text: &str) -> Result<(), String> {
        if !trusted() {
            return Err(NO_AX.into());
        }
        self.front();
        let units: Vec<u16> = text.encode_utf16().collect();
        // mehrere Zeichen je Ereignis sind erlaubt, aber nicht beliebig viele
        for chunk in units.chunks(16) {
            for down in [true, false] {
                let ev = CGEvent::new_keyboard_event(None, 0, down);
                unsafe { CGEvent::keyboard_set_unicode_string(ev.as_deref(), chunk.len() as _, chunk.as_ptr()) };
                CGEvent::post(CGEventTapLocation::HIDEventTap, ev.as_deref());
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        Ok(())
    }

    /// Name der App, um die es geht ("Discord")
    pub fn program(&self) -> Option<String> {
        let pid = self.pid.or_else(front_window)?;
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32).and_then(|a| a.localizedName()).map(|n| n.to_string())
    }

    pub fn send_keys(&mut self, k: &Keys) -> Result<(), String> {
        if !trusted() {
            return Err(NO_AX.into());
        }
        self.front();
        for down in [true, false] {
            let ev = CGEvent::new_keyboard_event(None, k.code, down);
            CGEvent::set_flags(ev.as_deref(), k.flags);
            if let Some(c) = k.ch {
                let u = [c as u16];
                unsafe { CGEvent::keyboard_set_unicode_string(ev.as_deref(), 1, u.as_ptr()) };
            }
            CGEvent::post(CGEventTapLocation::HIDEventTap, ev.as_deref());
        }
        Ok(())
    }

    /// Adresse der Seite im Browser vorn: der Webbereich meldet sie selbst (AXURL), sonst die Adressleiste
    pub fn browser_url(&mut self) -> Reply {
        if !trusted() {
            return Err(NO_AX.into());
        }
        let pid = front_window().ok_or("Kein Fenster im Vordergrund.")?;
        let w = window_of(pid).ok_or("Kein Fenster im Vordergrund.")?;
        let mut url = None;
        walk(w, |e, v| {
            if text_of(v.first().and_then(|x| x.as_deref())) == "AXWebArea" {
                url = attr(&e, "AXURL").and_then(|u| u.downcast::<CFURL>().ok()).map(|u| {
                    let ns: &NSURL = unsafe { &*(&*u as *const CFURL as *const NSURL) };
                    ns.absoluteString().map(|s| s.to_string()).unwrap_or_default()
                });
            }
            url.is_none()
        });
        if let Some(u) = url.filter(|u| u.starts_with("http")) {
            return Ok(json!(u));
        }
        self.read(pid)?;
        for it in self.items.iter().filter(|it| it.role == "Eingabefeld") {
            let Target::El(e) = &it.target else { continue };
            let v = text_of(attr(e, "AXValue").as_deref());
            if url_like(&v) {
                return Ok(json!(v.trim()));
            }
        }
        Err("Im Vordergrund ist keine Webseite offen (Adressleiste nicht gefunden).".into())
    }

    /// Ziel-App nach vorn, bevor Maus oder Tastatur etwas schicken
    fn front(&self) {
        if let Some(p) = self.pid.or_else(front_window) {
            if win::foreground() != p {
                win::bring_to_front(p);
                std::thread::sleep(Duration::from_millis(120));
            }
        }
    }
}

/// Taste fuer CGEvent: virtueller Tastencode (US-Belegung), Zusatztasten, Zeichen (fuer Buchstaben/Ziffern)
#[derive(Debug, PartialEq)]
pub struct Keys {
    code: u16,
    flags: CGEventFlags,
    ch: Option<char>,
}

pub fn is_plain_enter(k: &Keys) -> bool {
    k.code == 36 && k.flags.is_empty()
}

/// Buchstaben und Ziffern -> Tastencode der US-Belegung (kVK_ANSI_*)
const ANSI: &[(char, u16)] = &[
    ('a', 0), ('s', 1), ('d', 2), ('f', 3), ('h', 4), ('g', 5), ('z', 6), ('x', 7), ('c', 8), ('v', 9), ('b', 11), ('q', 12), ('w', 13),
    ('e', 14), ('r', 15), ('y', 16), ('t', 17), ('1', 18), ('2', 19), ('3', 20), ('4', 21), ('6', 22), ('5', 23), ('9', 25), ('7', 26),
    ('8', 28), ('0', 29), ('o', 31), ('u', 32), ('i', 34), ('p', 35), ('l', 37), ('j', 38), ('k', 40), ('n', 45), ('m', 46),
];

/// "Strg+L", "Cmd+T", "Enter", "Umschalt+Tab", "F5" -> Taste. Strg heisst auf dem Mac Befehlstaste (⌘):
/// das Modell denkt in Windows-Kuerzeln (Strg+C, Strg+L), gemeint ist dasselbe. Kein Cmd+Q.
/// shortcut: Buchstaben gehen als Code der US-Belegung plus Zeichen raus; bei deutscher Belegung tauschen Y/Z nur dort,
/// wo eine App den Code statt des Zeichens liest. Abhilfe: Code ueber TISCopyCurrentKeyboardLayoutInputSource suchen.
pub fn parse_keys(spec: &str) -> Result<Keys, String> {
    let parts: Vec<String> = spec.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
    let (last, mods) = parts.split_last().ok_or("Keine Taste angegeben.")?;
    let mut flags = CGEventFlags::empty();
    for p in mods {
        flags |= match p.as_str() {
            "strg" | "ctrl" | "cmd" | "command" | "befehl" | "⌘" => CGEventFlags::MaskCommand,
            "control" | "ctl" => CGEventFlags::MaskControl,
            "alt" | "option" | "wahl" => CGEventFlags::MaskAlternate,
            "umschalt" | "shift" | "hochstell" => CGEventFlags::MaskShift,
            _ => return Err(format!("Unbekannte Zusatztaste „{p}“ (erlaubt: Strg/Cmd, Control, Alt, Umschalt).")),
        };
    }
    let mut ch = None;
    let code: u16 = match last.as_str() {
        "enter" | "eingabe" | "return" => 36,
        "tab" | "tabulator" => 48,
        "esc" | "escape" => 53,
        "leertaste" | "space" | "leer" => 49,
        "rück" | "rueck" | "backspace" | "zurück" => 51,
        "entf" | "delete" | "del" => 117,
        "pos1" | "home" => 115,
        "ende" | "end" => 119,
        "bildauf" | "pageup" => 116,
        "bildab" | "pagedown" => 121,
        "oben" | "up" | "hoch" => 126,
        "unten" | "down" | "runter" => 125,
        "links" | "left" => 123,
        "rechts" | "right" => 124,
        s if s.len() >= 2 && s.starts_with('f') && s[1..].parse::<usize>().is_ok_and(|n| (1..=12).contains(&n)) => {
            [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111][s[1..].parse::<usize>().unwrap() - 1]
        }
        s if s.chars().count() == 1 => {
            let c = s.chars().next().unwrap();
            let code = ANSI.iter().find(|(k, _)| *k == c).map(|x| x.1).ok_or(format!("Unbekannte Taste „{last}“."))?;
            ch = Some(c);
            code
        }
        _ => return Err(format!("Unbekannte Taste „{last}“.")),
    };
    if flags.contains(CGEventFlags::MaskCommand) && code == 12 {
        return Err("Cmd+Q drücke ich nicht — Programme beenden bitte selbst.".into());
    }
    Ok(Keys { code, flags, ch })
}

/// Rahmen des vorderen Fensters einer App (Punkte)
fn window_frame(pid: Win) -> Option<(f64, f64, f64, f64)> {
    let w = window_of(pid)?;
    frame(&w)
}

/// Bildschirmfoto eines Bereichs als RGB (braucht die Freigabe „Bildschirmaufnahme“; ohne sie liefert macOS nur
/// den Schreibtischhintergrund)
fn grab(x: f64, y: f64, w: f64, h: f64) -> Option<(u32, u32, Vec<u8>)> {
    // ohne Freigabe kaeme nur der Schreibtischhintergrund — dann lieber gar kein Bild (fragt beim ersten Mal)
    if !objc2_core_graphics::CGPreflightScreenCaptureAccess() {
        objc2_core_graphics::CGRequestScreenCaptureAccess();
        return None;
    }
    if w < 1.0 || h < 1.0 {
        return None;
    }
    let file = std::env::temp_dir().join(format!("notch-bild-{}.png", std::process::id()));
    let ok = std::process::Command::new("screencapture")
        .args(["-x", "-t", "png", "-R", &format!("{},{},{},{}", x.round(), y.round(), w.round(), h.round())])
        .arg(&file)
        .status()
        .is_ok_and(|s| s.success());
    let img = ok.then(|| image::open(&file).ok()).flatten();
    let _ = std::fs::remove_file(&file);
    let img = img?.to_rgb8();
    let (iw, ih) = img.dimensions();
    Some((iw, ih, img.into_raw()))
}

pub fn capture_rgb(pid: Win) -> Option<(u32, u32, Vec<u8>)> {
    if !trusted() {
        return None;
    }
    let (x, y, w, h) = window_frame(pid)?;
    grab(x, y, w, h)
}

/// Texterkennung (Vision, offline) ueber das Fenster: Zeilen mit Bildschirmpunkt der Zeilenmitte
fn ocr_lines(win: &AXUIElement) -> Option<Vec<(String, i32, i32)>> {
    use objc2::AnyThread;
    use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};
    let (x, y, w, h) = frame(win)?;
    let (iw, ih, rgb) = grab(x, y, w, h)?;
    let file = std::env::temp_dir().join(format!("notch-text-{}.png", std::process::id()));
    image::RgbImage::from_raw(iw, ih, rgb)?.save(&file).ok()?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
    let req = VNRecognizeTextRequest::new();
    req.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    req.setRecognitionLanguages(&NSArray::from_retained_slice(&[NSString::from_str("de-DE"), NSString::from_str("en-US")]));
    let handler = unsafe { VNImageRequestHandler::initWithURL_options(VNImageRequestHandler::alloc(), &url, &NSDictionary::new()) };
    let base: &VNRequest = &req;
    let done = handler.performRequests_error(&NSArray::from_slice(&[base]));
    let _ = std::fs::remove_file(&file);
    done.ok()?;
    let mut out = Vec::new();
    for obs in req.results()?.iter() {
        let Some(best) = obs.topCandidates(1).firstObject() else { continue };
        let text = best.string().to_string();
        if text.trim().chars().count() < 2 {
            continue;
        }
        // Vision: Anteile 0..1, Ursprung unten links
        let b = unsafe { obs.boundingBox() };
        let cx = x + (b.origin.x + b.size.width / 2.0) * w;
        let cy = y + (1.0 - (b.origin.y + b.size.height / 2.0)) * h;
        out.push((text, cx.round() as i32, cy.round() as i32));
    }
    // von oben nach unten, wie auf dem Bildschirm
    out.sort_by_key(|o| (o.2, o.1));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasten() {
        let k = parse_keys("Strg+L").unwrap();
        assert_eq!((k.code, k.flags, k.ch), (37, CGEventFlags::MaskCommand, Some('l')));
        assert!(is_plain_enter(&parse_keys("Enter").unwrap()));
        assert!(!is_plain_enter(&parse_keys("Umschalt+Enter").unwrap()));
        assert_eq!(parse_keys("umschalt + tab").unwrap().code, 48);
        assert_eq!(parse_keys("F5").unwrap().code, 96);
        assert!(parse_keys("Cmd+Q").is_err());
        assert!(parse_keys("Win+R").is_err());
        assert!(parse_keys("").is_err());
        assert!(parse_keys("F13").is_err());
    }
}
