//! Virtuelle Kamera „Notch iPhone“: eine Media-Foundation-Quelle, die der Windows-Frame-Server laedt
//! (MFCreateVirtualCamera, Windows 11). Sie liest fertige NV12-Bilder (1280x720) aus der Datei
//! `%PUBLIC%\Notch\vcam.frame`, in die die Notch die Bilder vom iPhone schreibt, und zeigt ohne frisches Bild
//! ein dunkles Platzhalterbild mit laufendem Balken.
//!
//! Der Frame-Server laeuft als LocalService: deshalb keine Pipes oder benannten Objekte, nur eine Datei, die jeder
//! lesen darf. Dateikopf (64 Byte): "NCAM", seq (u64 @8, ungerade = wird gerade geschrieben), Breite (u32 @16),
//! Hoehe (u32 @20), Zeit in ms seit 1970 (u64 @24); danach das Bild.

#![allow(non_snake_case)]

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{implement, IUnknownImpl, Interface, PWSTR, PCWSTR, Ref, Result, BOOL, GUID, HRESULT, IUnknown};
use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_NOTIMPL, S_FALSE, S_OK};
use windows::Win32::Media::KernelStreaming::{IKsControl, IKsControl_Impl, KSIDENTIFIER, KSPROPSETID_Pin, PINNAME_VIDEO_CAPTURE};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;

pub const W: u32 = 1280;
pub const H: u32 = 720;
const FPS: u32 = 30;
const FRAME: usize = (W * H * 3 / 2) as usize;
const HEADER: usize = 64;
/// Bilder, die aelter sind, gelten als „iPhone nicht verbunden“
const STALE_MS: u64 = 1500;

/// CLSID der Quelle; die Notch traegt sie unter HKCU/HKLM\Software\Classes\CLSID ein
const CLSID_NOTCH_VCAM: GUID = GUID::from_u128(0x7c0c9b3e_5a1d_4f6b_9e47_2d6a1b8c3f10);

const MF_E_INVALID_STATE: HRESULT = HRESULT(0xC00D3E82u32 as i32); // MF_E_INVALID_STATE_TRANSITION
const E_SET_NOT_FOUND: HRESULT = HRESULT(0x8007_0492u32 as i32); // HRESULT_FROM_WIN32(ERROR_SET_NOT_FOUND)
const MF_E_UNSUPPORTED_SERVICE_HR: HRESULT = HRESULT(0xC00D36B4u32 as i32);

fn data_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("PUBLIC").unwrap_or_else(|| "C:\\Users\\Public".into())).join("Notch")
}

/// Fehlersuche (nur mit `--features log`): im Frame-Server sieht man sonst nichts. Im Auslieferungsstand aus, weil der
/// Dienst sonst in einen Ordner schriebe, den jeder Benutzer vorbereiten (Verknuepfung auf eine andere Datei) koennte.
#[cfg(feature = "log")]
fn log(line: &str) {
    use std::io::Write;
    let p = data_dir().join("vcam.log");
    if std::fs::metadata(&p).is_ok_and(|m| m.len() > 64 * 1024) {
        let _ = std::fs::remove_file(&p);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{} {line}", now_ms());
    }
}

#[cfg(not(feature = "log"))]
fn log(_line: &str) {}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---------- Bilder lesen ----------

struct Feed {
    file: Option<std::fs::File>,
    buf: Vec<u8>,
}

impl Feed {
    fn new() -> Self {
        Feed { file: None, buf: vec![0; FRAME] }
    }

    /// Das neueste ganze Bild, `None` ohne frisches Bild (dann Platzhalter). Ein beim Lesen ueberschriebenes Bild
    /// (seq geaendert) wird verworfen.
    fn read(&mut self) -> Option<&[u8]> {
        use std::os::windows::fs::FileExt;
        if self.file.is_none() {
            // keine Verknuepfung als Bilddatei: der Frame-Server (LocalService) duerfte sonst fremde Dateien als Bild lesen
            let p = data_dir().join("vcam.frame");
            if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
                return None;
            }
            self.file = std::fs::File::open(p).ok();
        }
        let f = self.file.as_ref()?;
        let mut h = [0u8; HEADER];
        let ok = (|| {
            f.seek_read(&mut h, 0).ok().filter(|&n| n == HEADER)?;
            let seq = u64::from_le_bytes(h[8..16].try_into().ok()?);
            let (w, hh) = (u32::from_le_bytes(h[16..20].try_into().ok()?), u32::from_le_bytes(h[20..24].try_into().ok()?));
            let stamp = u64::from_le_bytes(h[24..32].try_into().ok()?);
            if &h[0..4] != b"NCAM" || seq % 2 == 1 || (w, hh) != (W, H) || now_ms().saturating_sub(stamp) > STALE_MS {
                return None;
            }
            f.seek_read(&mut self.buf, HEADER as u64).ok().filter(|&n| n == FRAME)?;
            f.seek_read(&mut h, 0).ok().filter(|&n| n == HEADER)?;
            (u64::from_le_bytes(h[8..16].try_into().ok()?) == seq).then_some(())
        })();
        if ok.is_none() && std::fs::metadata(data_dir().join("vcam.frame")).is_err() {
            self.file = None; // Datei weg: beim naechsten Mal neu oeffnen
        }
        ok.map(|_| &self.buf[..])
    }
}

/// Dunkles Bild mit laufendem hellem Balken
fn placeholder(tick: u32, out: &mut [u8]) {
    let (w, h) = (W as usize, H as usize);
    out[..w * h].fill(40);
    out[w * h..].fill(128);
    let x0 = (tick as usize * 8) % (w - 160);
    for row in (h / 2 - 4)..(h / 2 + 4) {
        out[row * w + x0..row * w + x0 + 160].fill(120);
    }
}

// ---------- Strom ----------

struct SendIt<T>(T);
unsafe impl<T> Send for SendIt<T> {}

#[derive(Default)]
struct Shared {
    /// ausstehende RequestSample-Aufrufe (mit Token)
    requests: Mutex<VecDeque<Option<SendIt<IUnknown>>>>,
    running: AtomicBool,
}

#[implement(IMFMediaStream2)]
struct Stream {
    source: Mutex<Option<IMFMediaSource>>,
    queue: IMFMediaEventQueue,
    desc: IMFStreamDescriptor,
    shared: Arc<Shared>,
}

impl IMFMediaEventGenerator_Impl for Stream_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        unsafe { self.queue.GetEvent(flags.0) }
    }
    fn BeginGetEvent(&self, cb: Ref<IMFAsyncCallback>, state: Ref<IUnknown>) -> Result<()> {
        unsafe { self.queue.BeginGetEvent(cb.as_ref(), state.as_ref()) }
    }
    fn EndGetEvent(&self, result: Ref<IMFAsyncResult>) -> Result<IMFMediaEvent> {
        unsafe { self.queue.EndGetEvent(result.as_ref()) }
    }
    fn QueueEvent(&self, met: u32, ext: *const GUID, hr: HRESULT, value: *const PROPVARIANT) -> Result<()> {
        unsafe { self.queue.QueueEventParamVar(met, ext, hr, value) }
    }
}

impl IMFMediaStream_Impl for Stream_Impl {
    fn GetMediaSource(&self) -> Result<IMFMediaSource> {
        self.source.lock().unwrap().clone().ok_or_else(|| MF_E_SHUTDOWN_HR.into())
    }
    fn GetStreamDescriptor(&self) -> Result<IMFStreamDescriptor> {
        Ok(self.desc.clone())
    }
    fn RequestSample(&self, token: Ref<IUnknown>) -> Result<()> {
        let mut q = self.shared.requests.lock().unwrap();
        if q.len() < 16 {
            q.push_back(token.as_ref().cloned().map(SendIt));
        }
        Ok(())
    }
}

const MF_E_SHUTDOWN_HR: HRESULT = HRESULT(0xC00D3E85u32 as i32);

impl IMFMediaStream2_Impl for Stream_Impl {
    fn SetStreamState(&self, _state: MF_STREAM_STATE) -> Result<()> {
        Ok(())
    }
    fn GetStreamState(&self) -> Result<MF_STREAM_STATE> {
        Ok(MF_STREAM_STATE_RUNNING)
    }
}

/// Liefert im Takt der Bildrate je ein Bild pro offener Anfrage
fn deliver(queue: SendIt<IMFMediaEventQueue>, shared: Arc<Shared>) {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let queue = queue.0;
    let mut feed = Feed::new();
    let mut frame = vec![0u8; FRAME];
    let t0 = Instant::now();
    let mut n: u32 = 0;
    while shared.running.load(Ordering::Relaxed) {
        n += 1;
        let due = t0 + Duration::from_micros(n as u64 * 1_000_000 / FPS as u64);
        std::thread::sleep(due.saturating_duration_since(Instant::now()));
        let token = shared.requests.lock().unwrap().pop_front();
        let Some(token) = token else { continue };
        match feed.read() {
            Some(f) => frame.copy_from_slice(f),
            None => placeholder(n, &mut frame),
        }
        let r = (|| -> Result<()> {
            unsafe {
                let buf = MFCreateMemoryBuffer(FRAME as u32)?;
                let mut p = std::ptr::null_mut();
                buf.Lock(&mut p, None, None)?;
                std::ptr::copy_nonoverlapping(frame.as_ptr(), p, FRAME);
                buf.Unlock()?;
                buf.SetCurrentLength(FRAME as u32)?;
                let s = MFCreateSample()?;
                s.AddBuffer(&buf)?;
                s.SetSampleTime(MFGetSystemTime())?;
                s.SetSampleDuration(10_000_000 / FPS as i64)?;
                if let Some(t) = &token {
                    s.SetUnknown(&MFSampleExtension_Token, &t.0)?;
                }
                queue.QueueEventParamUnk(MEMediaSample.0 as u32, &GUID::zeroed(), S_OK, &s)
            }
        })();
        if let Err(e) = r {
            log(&format!("Bild nicht geliefert: {e}"));
        }
    }
}

// ---------- Quelle ----------

#[implement(IMFMediaSourceEx, IMFGetService, IKsControl)]
struct Source {
    queue: IMFMediaEventQueue,
    attrs: IMFAttributes,
    pd: IMFPresentationDescriptor,
    sd: IMFStreamDescriptor,
    stream: Mutex<Option<(IMFMediaStream2, IMFMediaEventQueue)>>,
    shared: Arc<Shared>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

fn pack(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

fn create_source() -> Result<IMFMediaSourceEx> {
    unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_LITE)?;
        let mt = MFCreateMediaType()?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        mt.SetUINT64(&MF_MT_FRAME_SIZE, pack(W, H))?;
        mt.SetUINT64(&MF_MT_FRAME_RATE, pack(FPS, 1))?;
        mt.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
        mt.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        mt.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        mt.SetUINT32(&MF_MT_DEFAULT_STRIDE, W)?;
        mt.SetUINT32(&MF_MT_SAMPLE_SIZE, FRAME as u32)?;
        mt.SetUINT32(&MF_MT_AVG_BITRATE, (FRAME as u32) * 8 * FPS)?;

        let sd = MFCreateStreamDescriptor(0, &[Some(mt.clone())])?;
        sd.GetMediaTypeHandler()?.SetCurrentMediaType(&mt)?;
        sd.SetGUID(&MF_DEVICESTREAM_STREAM_CATEGORY, &PINNAME_VIDEO_CAPTURE)?;
        sd.SetUINT32(&MF_DEVICESTREAM_STREAM_ID, 0)?;
        sd.SetUINT32(&MF_DEVICESTREAM_FRAMESERVER_SHARED, 1)?;
        sd.SetUINT32(&MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES, MFFrameSourceTypes_Color.0 as u32)?;

        let pd = MFCreatePresentationDescriptor(Some(&[Some(sd.clone())]))?;
        pd.SelectStream(0)?;

        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 2)?;
        let attrs = attrs.ok_or(E_FAIL)?;
        let queue = MFCreateEventQueue()?;
        let src = Source { queue, attrs, pd, sd, stream: Mutex::new(None), shared: Arc::new(Shared::default()), thread: Mutex::new(None) };
        Ok(src.into())
    }
}

impl IMFMediaEventGenerator_Impl for Source_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        unsafe { self.queue.GetEvent(flags.0) }
    }
    fn BeginGetEvent(&self, cb: Ref<IMFAsyncCallback>, state: Ref<IUnknown>) -> Result<()> {
        unsafe { self.queue.BeginGetEvent(cb.as_ref(), state.as_ref()) }
    }
    fn EndGetEvent(&self, result: Ref<IMFAsyncResult>) -> Result<IMFMediaEvent> {
        unsafe { self.queue.EndGetEvent(result.as_ref()) }
    }
    fn QueueEvent(&self, met: u32, ext: *const GUID, hr: HRESULT, value: *const PROPVARIANT) -> Result<()> {
        unsafe { self.queue.QueueEventParamVar(met, ext, hr, value) }
    }
}

impl IMFMediaSource_Impl for Source_Impl {
    fn GetCharacteristics(&self) -> Result<u32> {
        Ok(MFMEDIASOURCE_IS_LIVE.0 as u32)
    }
    fn CreatePresentationDescriptor(&self) -> Result<IMFPresentationDescriptor> {
        unsafe { self.pd.Clone() }
    }
    fn Start(&self, _pd: Ref<IMFPresentationDescriptor>, _fmt: *const GUID, _pos: *const PROPVARIANT) -> Result<()> {
        log("Start");
        unsafe {
            let mut st = self.stream.lock().unwrap();
            let first = st.is_none();
            if first {
                let q = MFCreateEventQueue()?;
                let me: IMFMediaSource = self.to_interface::<IMFMediaSourceEx>().cast()?;
                let stream: IMFMediaStream2 = Stream { source: Mutex::new(Some(me)), queue: q.clone(), desc: self.sd.clone(), shared: self.shared.clone() }.into();
                *st = Some((stream, q));
            }
            let (stream, q) = st.as_ref().unwrap();
            let empty = PROPVARIANT::default();
            self.queue.QueueEventParamUnk(if first { MENewStream.0 } else { MEUpdatedStream.0 } as u32, &GUID::zeroed(), S_OK, stream)?;
            self.queue.QueueEventParamVar(MESourceStarted.0 as u32, &GUID::zeroed(), S_OK, &empty)?;
            q.QueueEventParamVar(MEStreamStarted.0 as u32, &GUID::zeroed(), S_OK, &empty)?;
            if !self.shared.running.swap(true, Ordering::SeqCst) {
                let (sh, q2) = (self.shared.clone(), SendIt(q.clone()));
                *self.thread.lock().unwrap() = Some(std::thread::spawn(move || deliver(q2, sh)));
            }
        }
        Ok(())
    }
    fn Stop(&self) -> Result<()> {
        log("Stop");
        self.shared.running.store(false, Ordering::SeqCst);
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
        self.shared.requests.lock().unwrap().clear();
        unsafe {
            let empty = PROPVARIANT::default();
            if let Some((_, q)) = self.stream.lock().unwrap().as_ref() {
                q.QueueEventParamVar(MEStreamStopped.0 as u32, &GUID::zeroed(), S_OK, &empty)?;
            }
            self.queue.QueueEventParamVar(MESourceStopped.0 as u32, &GUID::zeroed(), S_OK, &empty)
        }
    }
    fn Pause(&self) -> Result<()> {
        Err(MF_E_INVALID_STATE.into())
    }
    fn Shutdown(&self) -> Result<()> {
        log("Shutdown");
        self.shared.running.store(false, Ordering::SeqCst);
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
        if let Some((_, q)) = self.stream.lock().unwrap().take() {
            unsafe { let _ = q.Shutdown(); }
        }
        unsafe { self.queue.Shutdown() }
    }
}

impl IMFMediaSourceEx_Impl for Source_Impl {
    fn GetSourceAttributes(&self) -> Result<IMFAttributes> {
        Ok(self.attrs.clone())
    }
    fn GetStreamAttributes(&self, _id: u32) -> Result<IMFAttributes> {
        self.sd.cast()
    }
    fn SetD3DManager(&self, _m: Ref<IUnknown>) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

impl IMFGetService_Impl for Source_Impl {
    fn GetService(&self, _service: *const GUID, _riid: *const GUID, _out: *mut *mut c_void) -> Result<()> {
        Err(MF_E_UNSUPPORTED_SERVICE_HR.into())
    }
}

impl IKsControl_Impl for Source_Impl {
    fn KsProperty(&self, p: *const KSIDENTIFIER, _pl: u32, d: *mut c_void, dl: u32, ret: *mut u32) -> Result<()> {
        let id = unsafe { (*p).Anonymous.Anonymous };
        // Pin-Eigenschaften „wie viele Instanzen“ (KSPROPERTY_PIN_CINSTANCES = 0, GLOBALCINSTANCES = 8): PossibleCount, CurrentCount
        if id.Set == KSPROPSETID_Pin && (id.Id == 0 || id.Id == 8) && id.Flags & 1 != 0 {
            if dl < 8 || d.is_null() {
                if !ret.is_null() {
                    unsafe { *ret = 8 };
                }
                return Err(HRESULT(0x8007_00EAu32 as i32).into()); // ERROR_MORE_DATA
            }
            unsafe {
                (d as *mut u32).write_unaligned(1);
                (d as *mut u32).add(1).write_unaligned(0);
                if !ret.is_null() {
                    *ret = 8;
                }
            }
            return Ok(());
        }
        // Kamerasteuerung (Datenschutzblende u. a.): nicht vorhanden, aber ohne Fehlermeldung
        if id.Set == GUID::from_u128(0xC6E13370_30AC_11D0_A18C_00A0C9118956) && !d.is_null() {
            unsafe { std::ptr::write_bytes(d as *mut u8, 0, dl as usize) };
            if !ret.is_null() {
                unsafe { *ret = dl };
            }
            return Ok(());
        }
        Err(E_SET_NOT_FOUND.into())
    }
    fn KsMethod(&self, _m: *const KSIDENTIFIER, _ml: u32, _d: *mut c_void, _dl: u32, _ret: *mut u32) -> Result<()> {
        Err(E_SET_NOT_FOUND.into())
    }
    fn KsEvent(&self, _e: *const KSIDENTIFIER, _el: u32, _d: *mut c_void, _dl: u32, _ret: *mut u32) -> Result<()> {
        Err(E_SET_NOT_FOUND.into())
    }
}


// ---------- IMFActivate: so verlangt es MFCreateVirtualCamera (CLSID = Aktivierungsobjekt, es erzeugt die Quelle) ----------

#[implement(IMFActivate)]
struct Activate {
    inner: IMFAttributes,
}

impl IMFActivate_Impl for Activate_Impl {
    fn ActivateObject(&self, riid: *const GUID, out: *mut *mut c_void) -> Result<()> {
        let src = create_source().inspect_err(|e| log(&format!("Quelle nicht erzeugt: {e}")))?;
        let r = unsafe { src.query(riid, out) };
        log(&format!("ActivateObject riid={:?} -> {:#x}", unsafe { *riid }, r.0));
        r.ok()
    }
    fn ShutdownObject(&self) -> Result<()> {
        Ok(())
    }
    fn DetachObject(&self) -> Result<()> {
        Ok(())
    }
}

impl IMFAttributes_Impl for Activate_Impl {
    fn GetItem(&self, guidkey: *const GUID, pvalue: *mut PROPVARIANT) -> Result<()> {
        unsafe { self.inner.GetItem(guidkey, Some(pvalue)) }
    }
    fn GetItemType(&self, guidkey: *const GUID) -> Result<MF_ATTRIBUTE_TYPE> {
        unsafe { self.inner.GetItemType(guidkey) }
    }
    fn CompareItem(&self, guidkey: *const GUID, value: *const PROPVARIANT) -> Result<BOOL> {
        unsafe { self.inner.CompareItem(guidkey, value) }
    }
    fn Compare(&self, ptheirs: Ref<IMFAttributes>, matchtype: MF_ATTRIBUTES_MATCH_TYPE) -> Result<BOOL> {
        unsafe { self.inner.Compare(ptheirs.as_ref(), matchtype) }
    }
    fn GetUINT32(&self, guidkey: *const GUID) -> Result<u32> {
        unsafe { self.inner.GetUINT32(guidkey) }
    }
    fn GetUINT64(&self, guidkey: *const GUID) -> Result<u64> {
        unsafe { self.inner.GetUINT64(guidkey) }
    }
    fn GetDouble(&self, guidkey: *const GUID) -> Result<f64> {
        unsafe { self.inner.GetDouble(guidkey) }
    }
    fn GetGUID(&self, guidkey: *const GUID) -> Result<GUID> {
        unsafe { self.inner.GetGUID(guidkey) }
    }
    fn GetStringLength(&self, guidkey: *const GUID) -> Result<u32> {
        unsafe { self.inner.GetStringLength(guidkey) }
    }
    fn GetString(&self, guidkey: *const GUID, pwszvalue: PWSTR, cchbufsize: u32, pcchlength: *mut u32) -> Result<()> {
        unsafe { self.inner.GetString(guidkey, std::slice::from_raw_parts_mut(pwszvalue.0, cchbufsize as usize), (!pcchlength.is_null()).then_some(pcchlength)) }
    }
    fn GetAllocatedString(&self, guidkey: *const GUID, ppwszvalue: *mut PWSTR, pcchlength: *mut u32) -> Result<()> {
        unsafe { self.inner.GetAllocatedString(guidkey, ppwszvalue, pcchlength) }
    }
    fn GetBlobSize(&self, guidkey: *const GUID) -> Result<u32> {
        unsafe { self.inner.GetBlobSize(guidkey) }
    }
    fn GetBlob(&self, guidkey: *const GUID, pbuf: *mut u8, cbbufsize: u32, pcbblobsize: *mut u32) -> Result<()> {
        unsafe { self.inner.GetBlob(guidkey, std::slice::from_raw_parts_mut(pbuf, cbbufsize as usize), (!pcbblobsize.is_null()).then_some(pcbblobsize)) }
    }
    fn GetAllocatedBlob(&self, guidkey: *const GUID, ppbuf: *mut *mut u8, pcbsize: *mut u32) -> Result<()> {
        unsafe { self.inner.GetAllocatedBlob(guidkey, ppbuf, pcbsize) }
    }
    fn GetUnknown(&self, guidkey: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> Result<()> {
        unsafe { (Interface::vtable(&self.inner).GetUnknown)(Interface::as_raw(&self.inner), guidkey, riid, ppv).ok() }
    }
    fn SetItem(&self, guidkey: *const GUID, value: *const PROPVARIANT) -> Result<()> {
        unsafe { self.inner.SetItem(guidkey, value) }
    }
    fn DeleteItem(&self, guidkey: *const GUID) -> Result<()> {
        unsafe { self.inner.DeleteItem(guidkey) }
    }
    fn DeleteAllItems(&self) -> Result<()> {
        unsafe { self.inner.DeleteAllItems() }
    }
    fn SetUINT32(&self, guidkey: *const GUID, unvalue: u32) -> Result<()> {
        unsafe { self.inner.SetUINT32(guidkey, unvalue) }
    }
    fn SetUINT64(&self, guidkey: *const GUID, unvalue: u64) -> Result<()> {
        unsafe { self.inner.SetUINT64(guidkey, unvalue) }
    }
    fn SetDouble(&self, guidkey: *const GUID, fvalue: f64) -> Result<()> {
        unsafe { self.inner.SetDouble(guidkey, fvalue) }
    }
    fn SetGUID(&self, guidkey: *const GUID, guidvalue: *const GUID) -> Result<()> {
        unsafe { self.inner.SetGUID(guidkey, guidvalue) }
    }
    fn SetString(&self, guidkey: *const GUID, wszvalue: &PCWSTR) -> Result<()> {
        unsafe { self.inner.SetString(guidkey, *wszvalue) }
    }
    fn SetBlob(&self, guidkey: *const GUID, pbuf: *const u8, cbbufsize: u32) -> Result<()> {
        unsafe { self.inner.SetBlob(guidkey, std::slice::from_raw_parts(pbuf, cbbufsize as usize)) }
    }
    fn SetUnknown(&self, guidkey: *const GUID, punknown: Ref<IUnknown>) -> Result<()> {
        unsafe { self.inner.SetUnknown(guidkey, punknown.as_ref()) }
    }
    fn LockStore(&self) -> Result<()> {
        unsafe { self.inner.LockStore() }
    }
    fn UnlockStore(&self) -> Result<()> {
        unsafe { self.inner.UnlockStore() }
    }
    fn GetCount(&self) -> Result<u32> {
        unsafe { self.inner.GetCount() }
    }
    fn GetItemByIndex(&self, unindex: u32, pguidkey: *mut GUID, pvalue: *mut PROPVARIANT) -> Result<()> {
        unsafe { self.inner.GetItemByIndex(unindex, pguidkey, Some(pvalue)) }
    }
    fn CopyAllItems(&self, pdest: Ref<IMFAttributes>) -> Result<()> {
        unsafe { self.inner.CopyAllItems(pdest.as_ref()) }
    }
}

// ---------- COM-Einstieg ----------

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, riid: *const GUID, out: *mut *mut c_void) -> Result<()> {
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        log("CreateInstance");
        let mut attrs = None;
        unsafe { MFCreateAttributes(&mut attrs, 4)? };
        let act: IMFActivate = Activate { inner: attrs.ok_or(E_FAIL)? }.into();
        let r = unsafe { act.query(riid, out) };
        log(&format!("CreateInstance riid={:?} -> {:#x}", unsafe { *riid }, r.0));
        r.ok()
    }
    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

#[no_mangle]
pub extern "system" fn DllGetClassObject(clsid: *const GUID, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if unsafe { *clsid } != CLSID_NOTCH_VCAM {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let f: IClassFactory = Factory.into();
    unsafe { f.query(riid, out) }
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    // der Frame-Server haelt die Quelle; entladen bringt nichts
    S_FALSE
}
