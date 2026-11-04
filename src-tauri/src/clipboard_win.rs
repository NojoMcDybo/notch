//! Zwischenablage unter Windows (Teil von clipboard.rs): lesen, was drin ist, und wieder hineinlegen.

use std::sync::atomic::Ordering;
use std::time::Duration;

use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;

/// Aendert sich bei jedem neuen Inhalt
pub fn seq() -> u64 {
    unsafe { GetClipboardSequenceNumber() as u64 }
}

/// Dateien, Text oder "Bild vorhanden" — None, wenn belegt oder vom Verlauf ausgeschlossen
pub fn peek() -> Option<(Option<Vec<String>>, Option<String>, bool)> {
    let _open = Open::new(None)?;
    if excluded() {
        return None;
    }
    let files = read_files().filter(|f| !f.is_empty());
    let text = if files.is_none() { read_text().filter(|t| !t.trim().is_empty()) } else { None };
    let image = files.is_none() && text.is_none() && has_image();
    Some((files, text, image))
}

fn owner() -> Option<HWND> {
    let h = crate::HWND.load(Ordering::Relaxed);
    (h != 0).then(|| HWND(h as *mut _))
}

/// Zwischenablage oeffnen (andere Programme halten sie manchmal kurz fest) und beim Verlassen schliessen.
struct Open;
impl Open {
    fn new(owner: Option<HWND>) -> Option<Self> {
        for _ in 0..8 {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Some(Open);
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        None
    }
}
impl Drop for Open {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

fn available(fmt: u32) -> bool {
    unsafe { IsClipboardFormatAvailable(fmt) }.is_ok()
}

/// Passwortmanager & Co. markieren ihre Inhalte als "nicht in den Verlauf".
fn excluded() -> bool {
    unsafe {
        let ex = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let ign = RegisterClipboardFormatW(w!("Clipboard Viewer Ignore"));
        if available(ex) || available(ign) {
            return true;
        }
        let can = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if available(can) {
            if let Ok(h) = GetClipboardData(can) {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u32;
                if !p.is_null() {
                    let v = *p;
                    let _ = GlobalUnlock(g);
                    if v == 0 {
                        return true;
                    }
                }
            }
        }
        false
    }
}

fn read_text() -> Option<String> {
    if !available(CF_UNICODETEXT) {
        return None;
    }
    unsafe {
        let h = GetClipboardData(CF_UNICODETEXT).ok()?;
        let g = HGLOBAL(h.0);
        let p = GlobalLock(g) as *const u16;
        if p.is_null() {
            return None;
        }
        let n = GlobalSize(g) / 2;
        let s = std::slice::from_raw_parts(p, n);
        let end = s.iter().position(|&c| c == 0).unwrap_or(n);
        let t = String::from_utf16_lossy(&s[..end]);
        let _ = GlobalUnlock(g);
        Some(t)
    }
}

fn read_files() -> Option<Vec<String>> {
    if !available(CF_HDROP) {
        return None;
    }
    unsafe {
        let h = GetClipboardData(CF_HDROP).ok()?;
        let drop = HDROP(h.0);
        let n = DragQueryFileW(drop, u32::MAX, None);
        let mut v = Vec::new();
        for i in 0..n {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(drop, i, Some(&mut buf));
            v.push(String::from_utf16_lossy(&buf[..len]));
        }
        Some(v)
    }
}

fn has_image() -> bool {
    let png = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    available(CF_DIB) || available(CF_DIBV5) || available(png)
}

// ---------- Zurueck in die Zwischenablage ----------

fn put(fmt: u32, bytes: &[u8]) -> bool {
    unsafe {
        let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return false };
        let p = GlobalLock(g) as *mut u8;
        if p.is_null() {
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(g);
        // Bei Erfolg gehoert der Speicher jetzt Windows
        SetClipboardData(fmt, Some(HANDLE(g.0))).is_ok()
    }
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect()
}

pub fn set_text(t: &str) -> Result<(), String> {
    let _open = Open::new(owner()).ok_or("Zwischenablage belegt")?;
    unsafe { EmptyClipboard() }.map_err(|e| e.to_string())?;
    if put(CF_UNICODETEXT, &utf16z(t)) { Ok(()) } else { Err("Text konnte nicht kopiert werden".into()) }
}

pub fn set_files(files: &[String]) -> Result<(), String> {
    // DROPFILES: pFiles=20, pt=(0,0), fNC=0, fWide=1, danach die Pfade (UTF-16, je 0-terminiert, am Ende 0)
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(&20u32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&1i32.to_le_bytes());
    for f in files {
        b.extend(utf16z(f));
    }
    b.extend_from_slice(&0u16.to_le_bytes());
    let _open = Open::new(owner()).ok_or("Zwischenablage belegt")?;
    unsafe { EmptyClipboard() }.map_err(|e| e.to_string())?;
    if !put(CF_HDROP, &b) {
        return Err("Dateien konnten nicht kopiert werden".into());
    }
    // Einfuegen soll kopieren, nicht verschieben
    let effect = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
    put(effect, &1u32.to_le_bytes());
    Ok(())
}
