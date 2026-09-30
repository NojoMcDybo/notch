//! Duenne Win32-Helfer: Cursor, Vollbild-Erkennung, Overlay-Fensterstil, Apps nach vorn holen.

use std::os::windows::process::CommandExt;
use std::process::Command;

use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindow, GetWindowLongPtrW,
    GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed,
    SetForegroundWindow, SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_RESTORE,
    SW_SHOWNOACTIVATE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

struct Search {
    target: String,
    by_path: bool,
    found: Option<HWND>,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
    let s = &mut *(lp.0 as *mut Search);
    if !IsWindowVisible(hwnd).as_bool()
        || GetWindowTextLengthW(hwnd) == 0
        || GetWindow(hwnd, GW_OWNER).map(|o| !o.0.is_null()).unwrap_or(false)
    {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == std::process::id() {
        return true.into();
    }
    if let Some(p) = process_path(pid) {
        let p = p.to_lowercase();
        let hit = if s.by_path { p == s.target } else { p.rsplit('\\').next() == Some(s.target.as_str()) };
        if hit {
            s.found = Some(hwnd);
            return false.into();
        }
    }
    true.into()
}

/// Sichtbares Hauptfenster eines laufenden Programms nach vorn holen.
/// `target` ist ein voller Pfad (…\Haze.exe) oder nur ein Dateiname (spotify.exe).
/// Das klappt, weil der Klick auf die Notch die letzte Eingabe war -> Windows erlaubt uns den Fokuswechsel.
pub fn focus_exe(target: &str) -> bool {
    let t = target.to_lowercase();
    let mut s = Search { by_path: t.contains('\\'), target: t, found: None };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut s as *mut Search as isize));
        if let Some(h) = s.found {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            return SetForegroundWindow(h).as_bool();
        }
    }
    false
}

/// Medien-Quelle nach vorn holen. Store-Apps (AUMID mit "!") ueber shell:AppsFolder starten —
/// laufende Einzelinstanz-Apps wie Spotify kommen dabei nach vorn. Sonst ueber den Prozessnamen.
pub fn activate_app(source: &str) {
    if source.is_empty() {
        return;
    }
    if source.contains('!') {
        let exe = source.rsplit('!').next().unwrap_or("").to_lowercase() + ".exe";
        if focus_exe(&exe) {
            return;
        }
        let _ = Command::new("explorer.exe")
            .arg(format!("shell:AppsFolder\\{source}"))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        return;
    }
    let exe = if source.to_lowercase().ends_with(".exe") { source.to_string() } else { format!("{source}.exe") };
    focus_exe(&exe);
}

pub fn reveal(path: &str) {
    let _ = Command::new("explorer.exe").raw_arg(format!("/select,\"{path}\"")).spawn();
}
