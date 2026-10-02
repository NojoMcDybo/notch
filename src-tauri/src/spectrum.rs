//! Musik-Reaktion: greift per WASAPI-Loopback genau den Ausgang ab, auf dem der Player spielt
//! (z. B. "Sonar - Media"; sonst den Windows-Standardausgang), analysiert ihn (music_style.rs)
//! und schickt ~30x pro Sekunde "spectrum" an die Notch.
//!
//! Laeuft nur, wenn die Einstellung an ist, Musik spielt und kein Vollbild-Programm vorne ist —
//! sonst ist der Abgriff komplett geschlossen und der Faden schlaeft.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDevice, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX,
};
use windows::Win32::System::Com::{CoTaskMemFree, CLSCTX_ALL};

use crate::music_style::{classify, Analyzer, Style, StyleTracker, BANDS};

const EMIT_EVERY: Duration = Duration::from_millis(33);
const STYLE_EVERY: Duration = Duration::from_secs(1);
/// Geraet des Players neu suchen (Player/Geraet gewechselt)
const DEVICE_EVERY: Duration = Duration::from_secs(3);

/// Einstellung compact.music.react: "off" | "auto" | "spikes" | "wave" | "pulse"
fn react_mode() -> String {
    crate::settings_value("/music/react").and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| "off".into())
}

#[derive(Serialize, Clone)]
struct Payload {
    /// Baender 0..255, tief -> hoch
    b: Vec<u8>,
    /// Pegel 0..255
    e: u8,
    /// seit dem letzten Paket kam ein Schlag
    k: bool,
    /// erkannter Stil (fuer "auto")
    s: Style,
    /// geschaetztes Tempo, 0 = noch unbekannt
    bpm: u16,
    /// wie deutlich der Takt ist, 0..100
    beat: u8,
}

struct Capture {
    _client: IAudioClient,
    cap: IAudioCaptureClient,
    channels: usize,
    float: bool,
    rate: u32,
    device: String,
}

impl Capture {
    fn open(dev: &IMMDevice) -> windows::core::Result<Self> {
        unsafe {
            let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
            let fmt = client.GetMixFormat()?;
            let f: WAVEFORMATEX = *fmt;
            // Im geteilten Modus ist das Mischformat praktisch immer 32-bit float
            // (WAVE_FORMAT_IEEE_FLOAT oder EXTENSIBLE mit 32 bit); 16-bit PCM geht auch.
            let float = f.wFormatTag == 3 || (f.wFormatTag == 0xFFFE && f.wBitsPerSample == 32);
            let res = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, fmt, None);
            CoTaskMemFree(Some(fmt as _));
            res?;
            if !float && f.wBitsPerSample != 16 {
                return Err(windows::core::Error::from_hresult(windows::core::HRESULT(0x8889_0008u32 as i32)));
            }
            let cap: IAudioCaptureClient = client.GetService()?;
            client.Start()?;
            Ok(Self {
                _client: client,
                cap,
                channels: f.nChannels.max(1) as usize,
                float,
                rate: f.nSamplesPerSec,
                device: crate::audio::device_name(dev),
            })
        }
    }

    /// Alles Angefallene als Mono lesen.
    fn read(&self, out: &mut Vec<f32>) -> windows::core::Result<()> {
        unsafe {
            loop {
                if self.cap.GetNextPacketSize()? == 0 {
                    return Ok(());
                }
                let mut data = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                self.cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let n = frames as usize;
                if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                    out.extend(std::iter::repeat_n(0.0, n));
                } else if self.float {
                    let s = std::slice::from_raw_parts(data as *const f32, n * self.channels);
                    out.extend(s.chunks_exact(self.channels).map(|c| c.iter().sum::<f32>() / self.channels as f32));
                } else {
                    let s = std::slice::from_raw_parts(data as *const i16, n * self.channels);
                    out.extend(
                        s.chunks_exact(self.channels)
                            .map(|c| c.iter().map(|&v| v as f32 / 32768.0).sum::<f32>() / self.channels as f32),
                    );
                }
                self.cap.ReleaseBuffer(frames)?;
            }
        }
    }
}

/// Ausgang des Players, sonst Standardausgang.
fn target_device() -> Option<IMMDevice> {
    let en = crate::audio::enumerator().ok()?;
    crate::audio::player_exe()
        .and_then(|exe| crate::audio::player_device(&en, &exe))
        .or_else(|| unsafe { en.GetDefaultAudioEndpoint(eRender, eConsole).ok() })
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut cap: Option<Capture> = None;
        let mut analyzer: Option<Analyzer> = None;
        let mut tracker = StyleTracker::new();
        let mut samples = Vec::new();
        let mut acc = [0f32; BANDS];
        let (mut energy, mut kick) = (0f32, false);
        let (mut last_emit, mut last_style, mut last_device) = (Instant::now(), Instant::now(), Instant::now());
        let mut song = String::new();
        let mut character = None;
        loop {
            let media = crate::media::last();
            let playing = media.as_ref().is_some_and(|m| m.playing);
            let active = playing
                && react_mode() != "off"
                && !crate::FULLSCREEN.load(std::sync::atomic::Ordering::Relaxed);
            if !active {
                cap = None;
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            // neuer Song: Stil neu einschaetzen (Aussteuerung bleibt)
            if let Some(m) = &media {
                let id = format!("{}\u{1}{}", m.artist, m.title);
                if id != song {
                    song = id;
                    tracker.song_changed();
                    character = None;
                    if let Some(a) = analyzer.as_mut() {
                        a.reset_history();
                    }
                }
            }
            if cap.is_none() || last_device.elapsed() > DEVICE_EVERY {
                last_device = Instant::now();
                if let Some(dev) = target_device() {
                    let name = crate::audio::device_name(&dev);
                    if cap.as_ref().is_none_or(|c| c.device != name) {
                        cap = Capture::open(&dev).map_err(|e| eprintln!("[notch] Musik-Reaktion: {name}: {e}")).ok();
                        if let Some(c) = &cap {
                            if analyzer.as_ref().is_none_or(|a| a.rate() != c.rate as f32) {
                                analyzer = Some(Analyzer::new(c.rate));
                            }
                        }
                    }
                }
                if cap.is_none() {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
            }
            let (Some(c), Some(a)) = (cap.as_ref(), analyzer.as_mut()) else { continue };
            samples.clear();
            if c.read(&mut samples).is_err() {
                // Geraet weg (abgesteckt, Sonar neu gestartet): beim naechsten Durchgang neu oeffnen
                cap = None;
                continue;
            }
            a.push(&samples, |f| {
                for (x, v) in acc.iter_mut().zip(f.bands) {
                    *x = x.max(v);
                }
                energy = energy.max(f.energy);
                kick |= f.kick;
            });
            if last_style.elapsed() >= STYLE_EVERY {
                last_style = Instant::now();
                if let Some(ch) = a.character() {
                    tracker.update(classify(&ch));
                    character = Some(ch);
                }
            }
            if last_emit.elapsed() >= EMIT_EVERY {
                last_emit = Instant::now();
                let ch = character.unwrap_or_default();
                let _ = app.emit(
                    "spectrum",
                    Payload {
                        b: acc.iter().map(|v| (v * 255.0).round() as u8).collect(),
                        e: (energy * 255.0).round() as u8,
                        k: kick,
                        s: tracker.current(),
                        bpm: ch.bpm.round() as u16,
                        beat: (ch.beat * 100.0).round() as u8,
                    },
                );
                acc = [0.0; BANDS];
                energy = 0.0;
                kick = false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
}
