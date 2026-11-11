//! Virtuelle Windows-Kamera „Notch iPhone“ (Media Foundation, Windows 11): Programme wie Teams, Zoom, Chrome und die
//! Kamera-App sehen sie als normale Webcam. Die eigentliche Quelle (vcam/, eigenes Paket) laeuft im Windows-Frame-Server
//! und liest die Bilder aus `%PUBLIC%\Notch\vcam.frame`; hier wird sie abgelegt, eingetragen, gestartet und mit
//! Bildern gefuettert (NV12, 1280x720). Die Kamera lebt nur, solange die Notch laeuft (Lebensdauer „Sitzung“).

use std::fs::File;
use std::os::windows::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const W: usize = 1280;
pub const H: usize = 720;
pub const FRAME: usize = W * H * 3 / 2;
const HEADER: usize = 64;
const CLSID: &str = "{7C0C9B3E-5A1D-4F6B-9E47-2D6A1B8C3F10}";
const NAME: &str = "Notch iPhone";
const DLL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/notch_vcam.dll"));

fn public_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("PUBLIC").unwrap_or_else(|| r"C:\Users\Public".into())).join("Notch")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---------- Bilder an die Kamera geben ----------

/// Schreibt Bilder in die gemeinsame Datei (Kopf siehe vcam/src/lib.rs); ungerade seq = „wird gerade geschrieben“
pub struct Writer {
    file: File,
    seq: u64,
}

impl Writer {
    pub fn open() -> std::io::Result<Self> {
        let dir = public_dir();
        std::fs::create_dir_all(&dir)?;
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("vcam.frame"))?;
        file.set_len((HEADER + FRAME) as u64)?;
        Ok(Writer { file, seq: 0 })
    }

    fn header(&self, seq: u64) -> [u8; HEADER] {
        let mut h = [0u8; HEADER];
        h[0..4].copy_from_slice(b"NCAM");
        h[8..16].copy_from_slice(&seq.to_le_bytes());
        h[16..20].copy_from_slice(&(W as u32).to_le_bytes());
        h[20..24].copy_from_slice(&(H as u32).to_le_bytes());
        h[24..32].copy_from_slice(&now_ms().to_le_bytes());
        h
    }

    pub fn write(&mut self, nv12: &[u8]) -> std::io::Result<()> {
        assert_eq!(nv12.len(), FRAME);
        self.seq += 1;
        self.file.seek_write(&self.header(self.seq), 0)?;
        self.file.seek_write(nv12, HEADER as u64)?;
        self.seq += 1;
        self.file.seek_write(&self.header(self.seq), 0).map(|_| ())
    }
}

/// RGB (3 Byte je Pixel, W x H) -> NV12, BT.601 mit begrenztem Bereich (so erwarten es Videoprogramme)
pub fn rgb_to_nv12(rgb: &[u8], out: &mut [u8]) {
    let px = |x: usize, y: usize| -> (i32, i32, i32) {
        let i = (y * W + x) * 3;
        (rgb[i] as i32, rgb[i + 1] as i32, rgb[i + 2] as i32)
    };
    for y in 0..H {
        for x in 0..W {
            let (r, g, b) = px(x, y);
            out[y * W + x] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
        }
    }
    for cy in 0..H / 2 {
        for cx in 0..W / 2 {
            let (mut r, mut g, mut b) = (0, 0, 0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (pr, pg, pb) = px(cx * 2 + dx, cy * 2 + dy);
                (r, g, b) = (r + pr, g + pg, b + pb);
            }
            let (r, g, b) = (r / 4, g / 4, b / 4);
            let o = W * H + cy * W + cx * 2;
            out[o] = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
            out[o + 1] = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
        }
    }
}

// ---------- Ablegen, eintragen, starten ----------

/// Kennung der eingebetteten Quelle (vcam/): Pruefsumme der DLL, von build.rs berechnet. Sie steckt im Dateinamen, damit
/// eine neue Fassung keine vom Frame-Server geladene ueberschreibt, und die Notch fragt nur dann erneut nach Administratorrechten.
const VCAM_REV: &str = include_str!(concat!(env!("OUT_DIR"), "/vcam_rev.txt"));

/// Ablageort der Quelle: unter Programme, weil sie vom Frame-Server (Dienst) geladen wird; ein fuer Benutzer
/// beschreibbarer Ort waere eine Hintertuer in den Dienst.
fn dll_path() -> PathBuf {
    PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into()))
        .join("Notch")
        .join("vcam")
        .join(format!("notch_vcam-r{VCAM_REV}.dll"))
}

fn run_hidden(prog: &str, args: &[&str]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let o = std::process::Command::new(prog).args(args).creation_flags(0x0800_0000).output().map_err(|e| e.to_string())?;
    if o.status.success() { Ok(()) } else { Err(format!("{prog}: {}", String::from_utf8_lossy(&o.stdout).trim())) }
}

static INSTALLED_CACHE: Mutex<Option<(std::time::Instant, bool)>> = Mutex::new(None);

const KEY: &str = r"HKLM\Software\Classes\CLSID\{7C0C9B3E-5A1D-4F6B-9E47-2D6A1B8C3F10}\InprocServer32";

/// Ist die aktuelle Quelle systemweit eingetragen? (Dann braucht das Starten keine Administratorrechte.)
/// Ein „ja“ bleibt; ein „nein“ wird 3 Sekunden gemerkt (die Einstellungen fragen oft nach, `reg` ist ein eigener Prozess).
pub fn installed() -> bool {
    use std::os::windows::process::CommandExt;
    if let Some((at, ok)) = *INSTALLED_CACHE.lock().unwrap() {
        if ok || at.elapsed() < std::time::Duration::from_secs(3) {
            return ok;
        }
    }
    let want = dll_path();
    let o = std::process::Command::new("reg").args(["query", KEY, "/ve"]).creation_flags(0x0800_0000).output();
    let ok = o.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&*want.to_string_lossy())) && want.exists();
    *INSTALLED_CACHE.lock().unwrap() = Some((std::time::Instant::now(), ok));
    ok
}

/// Einmalig mit Administratorrechten (`notch.exe --vcam-setup`): Quelle ablegen und fuer den Frame-Server eintragen
pub fn system_install() -> Result<(), String> {
    let path = dll_path();
    let dir = path.parent().ok_or("Pfad")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, DLL).map_err(|e| e.to_string())?;
    // gemeinsamer Ordner fuer die Bilder (und das kleine Protokoll der Quelle); Frame-Server (LocalService) darf schreiben
    let shared = public_dir();
    std::fs::create_dir_all(&shared).map_err(|e| e.to_string())?;
    run_hidden("icacls", &[&shared.to_string_lossy(), "/grant", "*S-1-5-19:(OI)(CI)(M)", "*S-1-5-32-545:(OI)(CI)(M)"])?;
    run_hidden("reg", &["add", KEY, "/ve", "/d", &path.to_string_lossy(), "/f"])?;
    run_hidden("reg", &["add", KEY, "/v", "ThreadingModel", "/d", "Both", "/f"])?;
    // alte Versionen aufraeumen (geladene lassen sich nicht loeschen; das ist in Ordnung)
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path() != path && e.path().extension().is_some_and(|x| x == "dll") {
            let _ = std::fs::remove_file(e.path());
        }
    }
    Ok(())
}

/// Windows fragt nach Administratorrechten; wartet, bis die Einrichtung fertig ist
pub fn elevate_setup(exe: &Path) -> Result<(), String> {
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let file = HSTRING::from(exe.as_os_str());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: windows::core::PCWSTR(file.as_ptr()),
        lpParameters: w!("--vcam-setup"),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info).map_err(|_| "Einrichtung abgebrochen (Administrator-Abfrage abgelehnt?)".to_string())?;
        WaitForSingleObject(info.hProcess, INFINITE);
        let _ = CloseHandle(info.hProcess);
    }
    // ein vor der Abfrage gemerktes „nein“ darf das Ergebnis nicht verfaelschen
    *INSTALLED_CACHE.lock().unwrap() = None;
    if installed() { Ok(()) } else { Err("Einrichtung fehlgeschlagen".into()) }
}

struct Keep(windows::Win32::Media::MediaFoundation::IMFVirtualCamera);
// COM-Zeiger der Kamera; wird nur angelegt und beim Beenden freigegeben
unsafe impl Send for Keep {}

static CAM: Mutex<Option<Keep>> = Mutex::new(None);

pub fn running() -> bool {
    CAM.lock().unwrap().is_some()
}

/// Kamera starten (wiederholbar); setzt die einmalige Einrichtung (`installed`) voraus
pub fn start() -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::Media::MediaFoundation::*;
    let mut cam = CAM.lock().unwrap();
    if cam.is_some() {
        return Ok(());
    }
    if !installed() {
        return Err("Kamera ist noch nicht eingerichtet".into());
    }
    unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_LITE).map_err(|e| e.message())?;
        let c = MFCreateVirtualCamera(
            MFVirtualCameraType_SoftwareCameraSource,
            MFVirtualCameraLifetime_Session,
            MFVirtualCameraAccess_CurrentUser,
            &HSTRING::from(NAME),
            &HSTRING::from(CLSID),
            None,
        )
        .map_err(|e| format!("MFCreateVirtualCamera: {}", e.message()))?;
        c.Start(None).map_err(|e| format!("Start: {}", e.message()))?;
        *cam = Some(Keep(c));
    }
    Ok(())
}

pub fn stop() {
    if let Some(Keep(c)) = CAM.lock().unwrap().take() {
        unsafe {
            let _ = c.Stop();
            let _ = c.Remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn farben() {
        let mut out = vec![0u8; FRAME];
        let color = |r: u8, g: u8, b: u8, out: &mut Vec<u8>| {
            let rgb: Vec<u8> = (0..W * H).flat_map(|_| [r, g, b]).collect();
            rgb_to_nv12(&rgb, out);
            (out[0], out[W * H], out[W * H + 1])
        };
        assert_eq!(color(255, 255, 255, &mut out), (235, 128, 128));
        assert_eq!(color(0, 0, 0, &mut out), (16, 128, 128));
        let (y, u, v) = color(255, 0, 0, &mut out); // rot: Y niedrig, V hoch, U niedrig
        assert!((80..=84).contains(&y) && u < 100 && v > 200, "{y} {u} {v}");
    }

    /// Bild der Kamera „Notch iPhone“ ueber Media Foundation lesen; Rueckgabe (Y, U, V) aus der Bildmitte
    fn read_center() -> (u8, u8, u8) {
        use windows::Win32::Media::MediaFoundation::*;
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_LITE).unwrap();
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 1).unwrap();
            let attrs = attrs.unwrap();
            attrs.SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID).unwrap();
            let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut n = 0u32;
            MFEnumDeviceSources(&attrs, &mut list, &mut n).unwrap();
            let acts = std::slice::from_raw_parts(list, n as usize);
            let mut found = None;
            for a in acts.iter().flatten() {
                let mut name = windows::core::PWSTR::null();
                let mut len = 0;
                a.GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut name, &mut len).unwrap();
                let name_s = name.to_string().unwrap_or_default();
                println!("Kamera: {name_s}");
                if name_s.starts_with(NAME) {
                    found = Some(a.clone());
                }
            }
            let a = found.expect("Notch iPhone nicht in der Kameraliste");
            let src: IMFMediaSource = a.ActivateObject().expect("Quelle aktivieren");
            let reader = MFCreateSourceReaderFromMediaSource(&src, None).expect("Reader");
            let mut last = (0, 0, 0);
            for _ in 0..40 {
                let (mut idx, mut flags, mut ts) = (0u32, 0u32, 0i64);
                let mut sample = None;
                reader.ReadSample(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, 0, Some(&mut idx), Some(&mut flags), Some(&mut ts), Some(&mut sample)).unwrap();
                if let Some(s) = sample {
                    let b = s.ConvertToContiguousBuffer().unwrap();
                    let (mut p, mut max, mut cur) = (std::ptr::null_mut(), 0u32, 0u32);
                    b.Lock(&mut p, Some(&mut max), Some(&mut cur)).unwrap();
                    let data = std::slice::from_raw_parts(p, cur as usize);
                    let uv = W * H + (H / 4) * W + (W / 4) * 2;
                    last = (data[W * H / 2 + W / 2], data[uv], data[uv + 1]);
                    println!("Bild: {cur} Byte, Mitte Y/U/V = {last:?}");
                    b.Unlock().unwrap();
                    if last.0 > 60 {
                        break;
                    }
                }
            }
            last
        }
    }

    /// Nur lesen: die laufende Notch hat die Kamera gestartet und bekommt Bilder (z. B. per curl an die HTTPS-Seite).
    /// Erwartet das gruene Testbild (RGB 0,200,0 -> Y~117, U~70, V~55).
    #[test]
    #[ignore]
    fn kamera_gruen() {
        let (y, u, v) = read_center();
        assert!((110..=125).contains(&y) && (62..=78).contains(&u) && (47..=63).contains(&v), "kein gruenes Bild: {y} {u} {v}");
    }

    /// Ende-zu-Ende am Rechner: Kamera einrichten, Bilder schreiben, ueber Media Foundation lesen.
    /// `cargo test --lib vcam_ende -- --ignored --nocapture` (NOTCH_EXE = gebaute notch.exe fuer die Einrichtung)
    #[test]
    #[ignore]
    fn vcam_ende() {
        if !installed() {
            // das App-Programm (notch.exe) versteht --vcam-setup; Pfad ueber NOTCH_EXE
            let exe = std::env::var("NOTCH_EXE").expect("NOTCH_EXE = Pfad zur gebauten notch.exe");
            elevate_setup(Path::new(&exe)).expect("Einrichtung");
        }
        start().expect("Kamera starten");
        println!("Kamera gestartet");
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s2 = stop.clone();
        let feeder = std::thread::spawn(move || {
            let mut w = Writer::open().unwrap();
            // roter Bildschirm: Y ~82
            let rgb: Vec<u8> = (0..W * H).flat_map(|_| [255u8, 0, 0]).collect();
            let mut f = vec![0u8; FRAME];
            rgb_to_nv12(&rgb, &mut f);
            while !s2.load(std::sync::atomic::Ordering::Relaxed) {
                w.write(&f).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(33));
            }
        });
        std::thread::sleep(std::time::Duration::from_secs(3));
        let (y_seen, _, _) = read_center();
        assert!((78..=86).contains(&y_seen), "Bild kam nicht vom Feeder (Y={y_seen})");
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        feeder.join().unwrap();
        super::stop();
    }
}
