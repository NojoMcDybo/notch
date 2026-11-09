//! Systemton mithoeren (Musik-Reaktion, Pegel) ueber einen Core-Audio-Tap — gibt es ab macOS 14.6; beim ersten
//! Mal fragt macOS nach der Freigabe „Nur Systemaudio aufnehmen“. Nichts wird gespeichert oder gesendet.
//! cpal baut dafuer einen Tap auf den Standardausgang und liest ihn wie ein Mikrofon.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use objc2_foundation::{NSOperatingSystemVersion, NSProcessInfo};

/// so viel puffern wir hoechstens (falls niemand abholt): 1 s bei 48 kHz
const MAX: usize = 48_000;

pub struct Loopback {
    _stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    pub rate: u32,
    pub device: String,
}

/// Process-Taps gibt es erst ab macOS 14.6 (CoreAudio ist schwach gelinkt, build.rs: aeltere Systeme starten trotzdem)
pub fn supported() -> bool {
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion { majorVersion: 14, minorVersion: 6, patchVersion: 0 })
}

/// Name des Standardausgangs (zum Erkennen eines Geraetewechsels)
pub fn output_name() -> Option<String> {
    let d = cpal::default_host().default_output_device()?;
    d.description().ok().map(|x| x.name().to_string())
}

impl Loopback {
    /// shortcut: hat der Standardausgang selbst einen Eingang (manche USB-Headsets als ein Geraet), liest cpal diesen
    /// statt des Taps; dann reagiert die Notch auf das Mikrofon. Abhilfe waere ein eigener Tap ueber CATapDescription.
    pub fn open() -> Result<Self, String> {
        if !supported() {
            return Err("Systemton mithören braucht macOS 14.6 oder neuer.".into());
        }
        let dev = cpal::default_host().default_output_device().ok_or("Kein Ausgabegerät.")?;
        let device = dev.description().map(|d| d.name().to_string()).unwrap_or_default();
        let cfg = dev.default_output_config().map_err(|e| e.to_string())?.config();
        let channels = (cfg.channels as usize).max(1);
        let rate = cfg.sample_rate;
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        let stream = dev
            .build_input_stream::<f32, _, _>(
                cfg,
                move |data: &[f32], _| {
                    let mut b = sink.lock().unwrap();
                    b.extend(data.chunks_exact(channels).map(|c| c.iter().sum::<f32>() / channels as f32));
                    let over = b.len().saturating_sub(MAX);
                    b.drain(..over);
                },
                |e| eprintln!("[notch] Systemton: {e}"),
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self { _stream: stream, buf, rate, device })
    }

    /// Alles seit dem letzten Abholen (Mono); gleiche Form wie die Windows-Aufnahme in spectrum.rs
    pub fn read(&self, out: &mut Vec<f32>) -> Result<(), String> {
        out.append(&mut self.buf.lock().unwrap());
        Ok(())
    }
}
