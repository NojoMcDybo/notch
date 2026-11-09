//! Bildschirm lesen und bedienen unter Windows (Teil von screen.rs): UI Automation, SendInput, Bildschirmfoto per
//! GDI, Texterkennung von Windows (offline). Laeuft im Faden von screen.rs (COM, MTA).

use std::time::Duration;

use serde_json::json;
use windows::core::BSTR;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HGDIOBJ, SRCCOPY,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEINPUT, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, SetCursorPos};

use super::{read_into, url_like, Item, Raw, Reply, Target};
use crate::win;

pub type El = IUIAutomationElement;

pub struct State {
    uia: IUIAutomation,
    pub items: Vec<Item<El>>,
    hwnd: Option<HWND>,
}

/// Das Fenster, um das es geht: das vordere, ausser es ist die Notch selbst oder der Desktop/die Taskleiste
pub fn front_window() -> Option<HWND> {
    unsafe {
        let fg = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, &mut cls) as usize;
        let cls = String::from_utf16_lossy(&cls[..n]);
        if !fg.is_invalid() && pid != std::process::id() && !["Shell_TrayWnd", "Progman", "WorkerW"].contains(&cls.as_str()) {
            return Some(fg);
        }
    }
    win::top_windows().first().map(|w| HWND(w.hwnd as *mut _))
}

/// Hauptfenster eines Programms ("discord" -> discord.exe)
pub fn app_window(app: &str) -> Option<HWND> {
    let exe = format!("{}.exe", app.trim_end_matches(".exe"));
    win::top_windows().into_iter().find(|w| w.exe.eq_ignore_ascii_case(&exe)).map(|w| HWND(w.hwnd as *mut _))
}

pub fn bring(h: HWND) {
    win::bring_to_front(h.0 as isize);
}

pub fn open_uri(uri: &str) {
    let uri = windows::core::HSTRING::from(uri);
    unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(None, windows::core::w!("open"), &uri, None, None, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
    }
}

#[allow(non_upper_case_globals)] // Konstanten aus windows-rs
fn role(ct: UIA_CONTROLTYPE_ID) -> Option<&'static str> {
    Some(match ct {
        UIA_ButtonControlTypeId | UIA_SplitButtonControlTypeId => "Knopf",
        UIA_HyperlinkControlTypeId => "Link",
        UIA_EditControlTypeId => "Eingabefeld",
        UIA_ComboBoxControlTypeId => "Auswahl",
        UIA_CheckBoxControlTypeId => "Kästchen",
        UIA_RadioButtonControlTypeId => "Option",
        UIA_MenuItemControlTypeId => "Menüpunkt",
        UIA_TabItemControlTypeId => "Reiter",
        UIA_ListItemControlTypeId | UIA_TreeItemControlTypeId | UIA_DataItemControlTypeId => "Eintrag",
        UIA_SliderControlTypeId => "Regler",
        UIA_TextControlTypeId => "Text",
        _ => return None,
    })
}

fn value_of(e: &El) -> String {
    unsafe { e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).and_then(|v| v.CurrentValue()).map(|b| b.to_string()).unwrap_or_default() }
}

impl State {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let uia: IUIAutomation =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).map_err(|_| "Windows-Bedienschnittstelle nicht verfügbar.".to_string())?;
            Ok(State { uia, items: Vec::new(), hwnd: None })
        }
    }

    pub fn read(&mut self, hwnd: HWND) -> Reply {
        let title = win::top_windows().into_iter().find(|w| w.hwnd == hwnd.0 as isize);
        self.hwnd = Some(hwnd);
        let mut found = Vec::new();
        unsafe {
            if let Ok(root) = self.uia.ElementFromHandle(hwnd) {
                let req = self.uia.CreateCacheRequest().map_err(|e| e.to_string())?;
                for p in [UIA_NamePropertyId, UIA_ControlTypePropertyId, UIA_IsOffscreenPropertyId, UIA_IsPasswordPropertyId, UIA_IsEnabledPropertyId] {
                    let _ = req.AddProperty(p);
                }
                let cond = self.uia.ControlViewCondition().map_err(|e| e.to_string())?;
                if let Ok(all) = root.FindAllBuildCache(TreeScope_Descendants, &cond, &req) {
                    for i in 0..all.Length().unwrap_or(0) {
                        if let Ok(e) = all.GetElement(i) {
                            found.push(e);
                        }
                    }
                }
            }
        }
        let raw = found.into_iter().filter_map(|e| unsafe {
            let r = e.CachedControlType().ok().and_then(role)?;
            if e.CachedIsOffscreen().is_ok_and(|b| b.as_bool()) || e.CachedIsEnabled().is_ok_and(|b| !b.as_bool()) {
                return None;
            }
            let name = super::short(&e.CachedName().map(|b| b.to_string()).unwrap_or_default(), 80);
            let password = e.CachedIsPassword().is_ok_and(|b| b.as_bool());
            // Inhalt von Feldern (Suchbegriff …), nie von Passwortfeldern
            let value = if !password && matches!(r, "Eingabefeld" | "Auswahl") { super::short(&value_of(&e), 60) } else { String::new() };
            Some(Raw { el: e, role: r, name, value, password })
        });
        read_into(
            &mut self.items,
            raw,
            || unsafe { ocr_lines(hwnd) },
            &title.as_ref().map(|w| w.title.clone()).unwrap_or_default(),
            title.as_ref().map(|w| w.exe.trim_end_matches(".exe")).unwrap_or_default(),
        )
    }

    /// Element ausloesen (Invoke/Umschalten/Auswaehlen/Aufklappen), sonst Mausklick auf die Mitte
    pub fn press(&mut self, n: usize) -> Result<(), String> {
        let it = super::item(&self.items, n)?;
        let point = match &it.target {
            Target::Point(x, y) => Some((*x, *y)),
            Target::El(e) => unsafe {
                let done = e.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId).and_then(|p| p.Invoke()).is_ok()
                    || e.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId).and_then(|p| p.Toggle()).is_ok()
                    || e.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId).and_then(|p| p.Select()).is_ok()
                    || e.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId).and_then(|p| p.Expand()).is_ok();
                if done {
                    None
                } else {
                    let r = e.CurrentBoundingRectangle().map_err(|e| e.to_string())?;
                    Some(((r.left + r.right) / 2, (r.top + r.bottom) / 2))
                }
            },
        };
        if let Some((x, y)) = point {
            self.front();
            unsafe {
                let _ = SetCursorPos(x, y);
                std::thread::sleep(Duration::from_millis(30));
                mouse(MOUSEEVENTF_LEFTDOWN.0 | MOUSEEVENTF_LEFTUP.0);
            }
        }
        Ok(())
    }

    /// Feld fokussieren und den Text direkt setzen; false = geht nicht (dann tippt screen.rs)
    pub fn set_value(&mut self, n: usize, text: &str) -> bool {
        let Ok(Item { target: Target::El(e), .. }) = super::item(&self.items, n) else { return false };
        unsafe {
            let _ = e.SetFocus();
            e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .is_ok_and(|v| v.CurrentIsReadOnly().is_ok_and(|b| !b.as_bool()) && v.SetValue(&BSTR::from(text)).is_ok())
        }
    }

    pub fn focus_is_password(&self) -> bool {
        unsafe { self.uia.GetFocusedElement().and_then(|e| e.CurrentIsPassword()).is_ok_and(|b| b.as_bool()) }
    }

    pub fn type_keys(&mut self, text: &str) -> Result<(), String> {
        self.front();
        let mut inputs = Vec::new();
        for u in text.encode_utf16() {
            for up in [false, true] {
                inputs.push(key_input(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE.0 | if up { KEYEVENTF_KEYUP.0 } else { 0 }));
            }
        }
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        Ok(())
    }

    /// Programmname des Fensters, um das es geht ("discord")
    pub fn program(&self) -> Option<String> {
        let h = self.hwnd.or_else(front_window)?;
        win::top_windows().into_iter().find(|w| w.hwnd == h.0 as isize).map(|w| w.exe.trim_end_matches(".exe").to_string())
    }

    pub fn send_keys(&mut self, (mods, key): &Keys) -> Result<(), String> {
        self.front();
        let mut inputs = Vec::new();
        for m in mods {
            inputs.push(key_input(*m, 0, 0));
        }
        inputs.push(key_input(*key, 0, 0));
        inputs.push(key_input(*key, 0, KEYEVENTF_KEYUP.0));
        for m in mods.iter().rev() {
            inputs.push(key_input(*m, 0, KEYEVENTF_KEYUP.0));
        }
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        Ok(())
    }

    /// Adresse aus der Adressleiste des Browsers im Vordergrund
    pub fn browser_url(&mut self) -> Reply {
        let hwnd = front_window().ok_or("Kein Fenster im Vordergrund.")?;
        self.read(hwnd)?;
        for it in self.items.iter().filter(|it| it.role == "Eingabefeld") {
            let Target::El(e) = &it.target else { continue };
            let v = value_of(e);
            if url_like(&v) {
                return Ok(json!(v.trim()));
            }
        }
        Err("Im Vordergrund ist keine Webseite offen (Adressleiste nicht gefunden).".into())
    }

    /// Ziel-Fenster nach vorn, bevor Maus oder Tastatur etwas schicken
    fn front(&self) {
        if let Some(h) = self.hwnd.or_else(front_window) {
            win::bring_to_front(h.0 as isize);
            std::thread::sleep(Duration::from_millis(60));
        }
    }
}

fn key_input(vk: VIRTUAL_KEY, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: KEYBD_EVENT_FLAGS(flags), time: 0, dwExtraInfo: 0 } },
    }
}

unsafe fn mouse(flags: u32) {
    let mut v = Vec::new();
    for f in [MOUSEEVENTF_LEFTDOWN.0, MOUSEEVENTF_LEFTUP.0] {
        if flags & f != 0 {
            v.push(INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS(f), ..Default::default() } } });
        }
    }
    SendInput(&v, std::mem::size_of::<INPUT>() as i32);
}

pub type Keys = (Vec<VIRTUAL_KEY>, VIRTUAL_KEY);

pub fn is_plain_enter((mods, key): &Keys) -> bool {
    *key == VIRTUAL_KEY(0x0D) && mods.is_empty()
}

/// "Strg+L", "Enter", "Umschalt+Tab", "F5" -> (Modifikatoren, Taste). Keine Windows-Taste, kein Alt+F4.
pub fn parse_keys(spec: &str) -> Result<Keys, String> {
    let parts: Vec<String> = spec.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
    let (last, mods) = parts.split_last().ok_or("Keine Taste angegeben.")?;
    let mut m = Vec::new();
    for p in mods {
        m.push(VIRTUAL_KEY(match p.as_str() {
            "strg" | "ctrl" | "control" => 0x11,
            "alt" => 0x12,
            "umschalt" | "shift" | "hochstell" => 0x10,
            _ => return Err(format!("Unbekannte Zusatztaste „{p}“ (erlaubt: Strg, Alt, Umschalt).")),
        }));
    }
    let k = match last.as_str() {
        "enter" | "eingabe" | "return" => 0x0D,
        "tab" | "tabulator" => 0x09,
        "esc" | "escape" => 0x1B,
        "leertaste" | "space" | "leer" => 0x20,
        "rück" | "rueck" | "backspace" | "zurück" => 0x08,
        "entf" | "delete" | "del" => 0x2E,
        "pos1" | "home" => 0x24,
        "ende" | "end" => 0x23,
        "bildauf" | "pageup" => 0x21,
        "bildab" | "pagedown" => 0x22,
        "oben" | "up" | "hoch" => 0x26,
        "unten" | "down" | "runter" => 0x28,
        "links" | "left" => 0x25,
        "rechts" | "right" => 0x27,
        s if s.len() >= 2 && s.starts_with('f') && s[1..].parse::<u16>().is_ok_and(|n| (1..=12).contains(&n)) => 0x70 + s[1..].parse::<u16>().unwrap() - 1,
        s if s.chars().count() == 1 && s.chars().all(|c| c.is_ascii_alphanumeric()) => s.to_ascii_uppercase().as_bytes()[0] as u16,
        _ => return Err(format!("Unbekannte Taste „{last}“.")),
    };
    if m.contains(&VIRTUAL_KEY(0x12)) && k == 0x73 {
        return Err("Alt+F4 drücke ich nicht — Fenster schließen bitte selbst.".into());
    }
    Ok((m, VIRTUAL_KEY(k)))
}

/// Fensterbereich als RGB (fuers Bild ans Modell)
pub fn capture_rgb(hwnd: HWND) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h, bgra) = unsafe { capture(hwnd)? };
    Some((w, h, bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0]]).collect()))
}

/// Fensterbereich als BGRA (vom Bildschirm, also so, wie man ihn sieht)
unsafe fn capture(hwnd: HWND) -> Option<(u32, u32, Vec<u8>)> {
    let mut r = RECT::default();
    GetWindowRect(hwnd, &mut r).ok()?;
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= 0 {
        return None;
    }
    let screen = GetDC(None);
    let mem = CreateCompatibleDC(Some(screen));
    let bmp = CreateCompatibleBitmap(screen, w, h);
    let old = SelectObject(mem, HGDIOBJ(bmp.0));
    let ok = BitBlt(mem, 0, 0, w, h, Some(screen), r.left, r.top, SRCCOPY | CAPTUREBLT).is_ok();
    let mut px = vec![0u8; (w * h * 4) as usize];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let got = ok && GetDIBits(mem, bmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS) == h;
    SelectObject(mem, old);
    let _ = DeleteObject(HGDIOBJ(bmp.0));
    let _ = DeleteDC(mem);
    ReleaseDC(None, screen);
    got.then_some((w as u32, h as u32, px))
}

/// Texterkennung (Windows, offline) ueber das Fenster: Zeilen mit Bildschirmpunkt der Zeilenmitte
unsafe fn ocr_lines(hwnd: HWND) -> Option<Vec<(String, i32, i32)>> {
    let mut r = RECT::default();
    GetWindowRect(hwnd, &mut r).ok()?;
    let (w, h, mut bgra) = capture(hwnd)?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().ok()?;
    // zu grosse Bilder verkleinern (Grenze der Texterkennung), Punkte danach zurueckrechnen
    let max = OcrEngine::MaxImageDimension().unwrap_or(2600);
    let mut scale = 1.0f32;
    let (mut bw, mut bh) = (w, h);
    if w.max(h) > max {
        scale = max as f32 / w.max(h) as f32;
        let img = image::RgbaImage::from_raw(w, h, bgra)?;
        let small = image::imageops::resize(&img, (w as f32 * scale) as u32, (h as f32 * scale) as u32, image::imageops::FilterType::Triangle);
        (bw, bh) = small.dimensions();
        bgra = small.into_raw();
    }
    for p in bgra.chunks_exact_mut(4) {
        p[3] = 255;
    }
    let dw = DataWriter::new().ok()?;
    dw.WriteBytes(&bgra).ok()?;
    let bmp = SoftwareBitmap::CreateCopyFromBuffer(&dw.DetachBuffer().ok()?, BitmapPixelFormat::Bgra8, bw as i32, bh as i32).ok()?;
    let res = engine.RecognizeAsync(&bmp).ok()?.join().ok()?;
    let mut out = Vec::new();
    for line in res.Lines().ok()? {
        let text = line.Text().map(|t| t.to_string()).unwrap_or_default();
        let words: Vec<_> = line.Words().ok()?.into_iter().filter_map(|w| w.BoundingRect().ok()).collect();
        if text.trim().chars().count() < 2 || words.is_empty() {
            continue;
        }
        let x0 = words.iter().map(|b| b.X).fold(f32::MAX, f32::min);
        let x1 = words.iter().map(|b| b.X + b.Width).fold(0.0, f32::max);
        let y0 = words.iter().map(|b| b.Y).fold(f32::MAX, f32::min);
        let y1 = words.iter().map(|b| b.Y + b.Height).fold(0.0, f32::max);
        out.push((text, r.left + ((x0 + x1) / 2.0 / scale) as i32, r.top + ((y0 + y1) / 2.0 / scale) as i32));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasten() {
        assert_eq!(parse_keys("Strg+L").unwrap(), (vec![VIRTUAL_KEY(0x11)], VIRTUAL_KEY(b'L' as u16)));
        assert_eq!(parse_keys("Enter").unwrap().1, VIRTUAL_KEY(0x0D));
        assert_eq!(parse_keys("umschalt + tab").unwrap(), (vec![VIRTUAL_KEY(0x10)], VIRTUAL_KEY(0x09)));
        assert_eq!(parse_keys("F5").unwrap().1, VIRTUAL_KEY(0x74));
        assert!(parse_keys("Alt+F4").is_err());
        assert!(parse_keys("Win+R").is_err());
        assert!(parse_keys("").is_err());
        assert!(parse_keys("F13").is_err());
    }

    /// Texterkennung allein auf das vordere Fenster: cargo test --lib screen_ocr -- --ignored --nocapture
    #[test]
    #[ignore]
    fn screen_ocr() {
        let t = std::time::Instant::now();
        let lines = unsafe { ocr_lines(front_window().unwrap()) }.expect("Texterkennung");
        println!("{} Zeilen in {:?}", lines.len(), t.elapsed());
        for l in lines.iter().take(12) {
            println!("{l:?}");
        }
        assert!(!lines.is_empty());
    }
}
