//! Vorschaubilder fuer Dateien (Ablage, Zwischenablage) ueber die Windows-Shell — dieselben, die der
//! Explorer zeigt: Fotos (auch HEIC, wenn die HEIF-Erweiterung installiert ist), PDF-Deckblaetter,
//! Videobilder, Office-Dokumente; ohne Vorschau das Programmsymbol. Ergebnis als PNG-data:-URL.
//!
//! Ein eigener Faden mit STA (manche Vorschau-Anbieter verlangen das); Ergebnisse bleiben im Speicher,
//! solange sich die Datei nicht aendert.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::mpsc::{channel, Sender};
use std::sync::{LazyLock, Mutex, OnceLock};

use base64::Engine;
use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT};

type Job = (String, u32, Sender<Option<String>>);

static TX: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
/// (Pfad, Groesse) -> (Aenderungszeit, data:-URL)
static CACHE: LazyLock<Mutex<HashMap<(String, u32), (u64, String)>>> = LazyLock::new(Default::default);

fn mtime(path: &str) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn spawn() {
    let (tx, rx) = channel::<Job>();
    let _ = TX.set(Mutex::new(tx));
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        for (path, size, back) in rx {
            let _ = back.send(render(&path, size));
        }
    });
}

/// Shell-Vorschau als PNG-data:-URL (None, wenn die Datei fehlt oder Windows nichts liefert)
fn render(path: &str, size: u32) -> Option<String> {
    let rgba = unsafe { shell_bitmap(path, size)? };
    let (w, h, px) = rgba;
    let img = image::RgbaImage::from_raw(w, h, px)?;
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(out.into_inner())))
}

unsafe fn shell_bitmap(path: &str, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    let item: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
    let hbm = item.GetImage(SIZE { cx: size as i32, cy: size as i32 }, SIIGBF_RESIZETOFIT).ok()?;
    let mut bm = BITMAP::default();
    let got = GetObjectW(HGDIOBJ(hbm.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    let mut px = vec![0u8; (w.max(0) * h.max(0) * 4) as usize];
    let mut ok = false;
    if got > 0 && w > 0 && h > 0 {
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // von oben nach unten
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = CreateCompatibleDC(None);
        ok = GetDIBits(dc, hbm, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS) == h;
        let _ = DeleteDC(dc);
    }
    let _ = DeleteObject(HGDIOBJ(hbm.0));
    if !ok {
        return None;
    }
    // BGRA -> RGBA; Fotos kommen oft ohne Alphakanal (alles 0) -> deckend
    let opaque = px.chunks_exact(4).all(|p| p[3] == 0);
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2);
        if opaque {
            p[3] = 255;
        }
    }
    Some((w as u32, h as u32, px))
}

/// Wie `thumb`, aber blockierend (aus einem Faden, der warten darf), ohne Zwischenspeicher
pub fn render_now(path: &str, size: u32) -> Option<String> {
    let tx = TX.get()?.lock().unwrap().clone();
    let (back, rx) = channel();
    tx.send((path.to_string(), size, back)).ok()?;
    rx.recv_timeout(std::time::Duration::from_secs(3)).ok().flatten()
}

#[tauri::command]
pub async fn thumb(path: String, size: Option<u32>) -> Option<String> {
    let size = size.unwrap_or(128).clamp(24, 256);
    let m = mtime(&path);
    if m == 0 {
        return None;
    }
    let key = (path.clone(), size);
    if let Some((t, url)) = CACHE.lock().unwrap().get(&key) {
        if *t == m {
            return Some(url.clone());
        }
    }
    let tx = TX.get()?.lock().unwrap().clone();
    let url = tauri::async_runtime::spawn_blocking(move || {
        let (back, rx) = channel();
        tx.send((path, size, back)).ok()?;
        rx.recv().ok().flatten()
    })
    .await
    .ok()
    .flatten()?;
    let mut c = CACHE.lock().unwrap();
    if c.len() > 300 {
        c.clear();
    }
    c.insert(key, (m, url.clone()));
    Some(url)
}

#[cfg(test)]
mod tests {
    /// Echte Dateien auf diesem PC: `cargo test --lib thumb -- --ignored` mit NOTCH_THUMB_TEST=<Pfad;Pfad>
    #[test]
    #[ignore]
    fn shell_vorschau_fuer_echte_dateien() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let list = std::env::var("NOTCH_THUMB_TEST").unwrap_or_else(|_| r"C:\Windows\explorer.exe".into());
        for p in list.split(';') {
            let url = super::render(p, 128).unwrap_or_else(|| panic!("keine Vorschau fuer {p}"));
            assert!(url.starts_with("data:image/png;base64,"), "{p}");
            println!("{p}: {} Bytes", url.len());
            if let Ok(dir) = std::env::var("NOTCH_THUMB_OUT") {
                use base64::Engine;
                let png = base64::engine::general_purpose::STANDARD.decode(&url["data:image/png;base64,".len()..]).unwrap();
                let name = std::path::Path::new(p).file_name().unwrap().to_string_lossy().to_string();
                std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), png).unwrap();
            }
        }
    }
}
