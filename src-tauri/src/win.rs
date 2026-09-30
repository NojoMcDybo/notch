//! Duenne Win32-Helfer: Cursorposition, Vollbild-Erkennung, Overlay-Fensterstil.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsZoomed, SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, SW_SHOWNOACTIVATE,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

/// Cursor in physischen Bildschirmpixeln.
pub fn cursor() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok()? };
    Some((p.x, p.y))
}

/// true, wenn das Vordergrundfenster den ganzen Monitor bedeckt (Spiel, Video, F11-Browser).
/// Bewusst NICHT SHQueryUserNotificationState: das meldet auch bei Wallpaper Engine & Co.
/// dauerhaft "busy". Maximierte Fenster zaehlen nicht (die lassen die Taskleiste frei).
pub fn fullscreen_foreground(mon: (i32, i32, i32, i32)) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() || IsZoomed(fg).as_bool() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == std::process::id() {
            return false;
        }
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, &mut cls) as usize;
        let cls = String::from_utf16_lossy(&cls[..n]);
        if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        let (mx, my, mw, mh) = mon;
        r.left <= mx && r.top <= my && r.right >= mx + mw && r.bottom >= my + mh
    }
}

/// Erweiterte Fensterstile erzwingen:
/// - NOACTIVATE: Klick auf Play klaut dem aktuellen Programm nicht den Fokus
/// - TOOLWINDOW: nicht in Alt-Tab
/// - TRANSPARENT (+LAYERED): Maus faellt durch, solange sie nicht auf der Notch ist
/// Tauri/tao schreibt den Stil bei manchen Ereignissen neu, deshalb wird das in jeder
/// Runde geprueft und nur bei Abweichung gesetzt (ein billiger Systemaufruf).
pub fn enforce(raw: isize, click_through: bool) {
    let hwnd = HWND(raw as *mut core::ffi::c_void);
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        // wie tao selbst: LAYERED + TRANSPARENT zusammen an/aus
        let base = (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        let t = (WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0) as isize;
        let want = if click_through { ex | base | t } else { (ex | base) & !t };
        if want != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
        }
    }
}

pub fn show_noactivate(raw: isize) {
    unsafe {
        let _ = ShowWindow(HWND(raw as *mut core::ffi::c_void), SW_SHOWNOACTIVATE);
    }
}
