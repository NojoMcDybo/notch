//! Ton der iPhone-Bildschirmuebertragung (cam.rs, `POST /cam/<schluessel>/audio`) auf dem Standardausgang abspielen.
//! Das iPhone schickt 16-bit-PCM in kleinen Stuecken; WASAPI (geteilter Modus) wandelt Rate und Kanaele selbst um.
//! Die Wiedergabe laeuft in einem eigenen Faden und endet von selbst, wenn 5 s lang nichts mehr ankommt.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioClient, IAudioRenderClient, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX,
};
use windows::Win32::System::Com::CLSCTX_ALL;

struct Shared {
    q: Mutex<VecDeque<i16>>,
    stop: AtomicBool,
    last: Mutex<Instant>,
    rate: u32,
    ch: u16,
}

static PLAYER: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

/// 16-bit-PCM (little endian, verschachtelt) abgeben; startet die Wiedergabe beim ersten Stueck oder bei neuem Format
pub fn feed(rate: u32, ch: u16, pcm: &[u8]) {
    let mut g = PLAYER.lock().unwrap();
    if g.as_ref().is_some_and(|p| p.rate != rate || p.ch != ch || p.stop.load(Ordering::Relaxed)) {
        if let Some(p) = g.take() {
            p.stop.store(true, Ordering::Relaxed);
        }
    }
    let p = g
        .get_or_insert_with(|| {
            let p = Arc::new(Shared {
                q: Mutex::new(VecDeque::new()),
                stop: AtomicBool::new(false),
                last: Mutex::new(Instant::now()),
                rate,
                ch,
            });
            let p2 = p.clone();
            std::thread::spawn(move || {
                if let Err(e) = play(&p2) {
                    eprintln!("[notch] Bildschirmton: {e}");
                }
                p2.stop.store(true, Ordering::Relaxed);
            });
            p
        })
        .clone();
    drop(g);
    *p.last.lock().unwrap() = Instant::now();
    let mut q = p.q.lock().unwrap();
    q.extend(pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])));
    // hoechstens 0,4 s Rueckstand: lieber alte Klaenge verwerfen als der Ton hinkt dem Bild hinterher
    let cap = (rate as usize * ch as usize) * 2 / 5;
    if q.len() > cap {
        let drop_n = q.len() - cap;
        q.drain(..drop_n);
    }
}

fn play(p: &Shared) -> Result<(), String> {
    let e = |x: windows::core::Error| x.to_string();
    unsafe {
        let en = crate::audio::enumerator().map_err(e)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).map_err(e)?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None).map_err(e)?;
        let align = p.ch * 2;
        let wf = WAVEFORMATEX {
            wFormatTag: 1, // PCM
            nChannels: p.ch,
            nSamplesPerSec: p.rate,
            nAvgBytesPerSec: p.rate * align as u32,
            nBlockAlign: align,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                2_000_000,
                0,
                &wf,
                None,
            )
            .map_err(e)?;
        let size = client.GetBufferSize().map_err(e)?;
        let out: IAudioRenderClient = client.GetService().map_err(e)?;
        client.Start().map_err(e)?;
        let prime = p.rate as usize * p.ch as usize * 8 / 100; // 80 ms Vorlauf
        let mut primed = false;
        while !p.stop.load(Ordering::Relaxed) && p.last.lock().unwrap().elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
            let free = size.saturating_sub(client.GetCurrentPadding().map_err(e)?);
            if free == 0 {
                continue;
            }
            let mut q = p.q.lock().unwrap();
            if !primed && q.len() >= prime {
                primed = true;
            }
            let frames = if primed { (q.len() / p.ch as usize).min(free as usize) as u32 } else { 0 };
            if frames == 0 {
                primed = false;
                continue;
            }
            let buf = out.GetBuffer(frames).map_err(e)?;
            let dst = std::slice::from_raw_parts_mut(buf as *mut i16, frames as usize * p.ch as usize);
            for d in dst.iter_mut() {
                *d = q.pop_front().unwrap_or(0);
            }
            out.ReleaseBuffer(frames, 0).map_err(e)?;
        }
        let _ = client.Stop();
    }
    Ok(())
}
