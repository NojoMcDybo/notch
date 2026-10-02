//! Musik-Analyse fuer die Musik-Reaktion der Notch: Frequenzbaender, Schlaege, Tempo und Stil.
//! Reine Rechnung ohne Windows-Abhaengigkeit, damit sie mit kuenstlichen Signalen testbar ist.
//!
//! Ablauf: Samples (mono) rein -> alle HOP Samples eine FFT ueber die letzten N Samples ->
//! 24 logarithmische Baender (Bass innen, Hoehen aussen zeichnet das Frontend), Pegel und
//! Schlag ("kick", aus dem spektralen Fluss mit adaptiver Schwelle). Ueber ~8 s ergibt die
//! Autokorrelation der Onset-Huelle das Tempo und wie deutlich der Takt ist; zusammen mit
//! Schlagdichte und Bassanteil waehlt `classify` den Stil.

use std::collections::VecDeque;
use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use serde::Serialize;

pub const BANDS: usize = 24;
/// FFT-Laenge (~43 ms bei 48 kHz)
const N: usize = 2048;
/// neue Samples pro Analyse (~94 Analysen pro Sekunde bei 48 kHz)
const HOP: usize = 512;
const F_LO: f32 = 40.0;
const F_HI: f32 = 16_000.0;
/// bis hier zaehlt es als Bass (Kick, Bassline)
const BASS_HZ: f32 = 150.0;
/// Fenster fuer Tempo, Schlagdichte und Bassanteil
const WINDOW_SECS: f32 = 8.0;
/// so viel Musik braucht die erste Einschaetzung
const MIN_SECS: f32 = 4.0;
/// Dynamikbereich der Baender in dB
const RANGE_DB: f32 = 48.0;
/// so stark muss der Klangfluss mindestens springen, damit es ein Schlag ist (echte Kicks: 40-100,
/// gleichmaessige Toene: unter 1)
const MIN_FLUX: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// harte, schnelle Musik (Hardstyle, DnB, Techno): scharfe Zacken
    Spikes,
    /// ohne deutlichen Takt (Klassik, Ambient): weiche Welle
    Wave,
    /// alles dazwischen (Pop, House, Hip-Hop): die Kante atmet im Takt
    Pulse,
}

/// Eigenschaften der letzten ~8 s Musik.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Character {
    /// geschaetztes Tempo (Schlaege pro Minute)
    pub bpm: f32,
    /// wie deutlich der Takt ist, 0..1 (Autokorrelation beim Tempo)
    pub beat: f32,
    /// erkannte Schlaege pro Sekunde
    pub onsets: f32,
    /// Anteil der Energie unter 150 Hz, 0..1
    pub bass: f32,
}

/// Stil aus den Eigenschaften. Schwellen sind Startwerte und werden mit echter Musik nachgestellt.
pub fn classify(c: &Character) -> Style {
    if c.beat >= 0.3 && (c.bpm >= 135.0 || (c.onsets >= 2.5 && c.bass >= 0.45)) {
        Style::Spikes
    } else if c.beat < 0.2 && c.onsets < 1.2 {
        Style::Wave
    } else {
        Style::Pulse
    }
}

/// Ergebnis einer Analyse (~94 pro Sekunde).
pub struct Frame {
    /// 0..1 je Band, tief -> hoch
    pub bands: [f32; BANDS],
    /// Pegel 0..1 (automatisch ausgesteuert)
    pub energy: f32,
    /// genau hier beginnt ein Schlag
    pub kick: bool,
}

pub struct Analyzer {
    rate: f32,
    fps: f32,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buf: VecDeque<f32>,
    pending: usize,
    scratch: Vec<Complex32>,
    prev: Vec<f32>,
    edges: [usize; BANDS + 1],
    tilt: [f32; BANDS],
    bass_bins: usize,
    peak_db: f32,
    energy_ref: f32,
    flux_hist: VecDeque<f32>,
    env: VecDeque<f32>,
    onsets: VecDeque<f32>,
    bass_hist: VecDeque<(f32, f32)>,
    last_onset: f32,
    t: f32,
}

impl Analyzer {
    pub fn new(rate: u32) -> Self {
        let rate = rate.max(8000) as f32;
        let fft = FftPlanner::<f32>::new().plan_fft_forward(N);
        let window = (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos()).collect();
        let bin = |f: f32| ((f * N as f32 / rate).round() as usize).clamp(1, N / 2);
        let mut edges = [0usize; BANDS + 1];
        let mut tilt = [0f32; BANDS];
        for i in 0..=BANDS {
            let f = F_LO * (F_HI / F_LO).powf(i as f32 / BANDS as f32);
            edges[i] = bin(f);
            if i > 0 && edges[i] <= edges[i - 1] {
                edges[i] = (edges[i - 1] + 1).min(N / 2);
            }
        }
        for (i, t) in tilt.iter_mut().enumerate() {
            // Musik hat oben weniger Energie (~3 dB/Oktave): ausgleichen, sonst bleiben die Hoehen flach
            let fc = F_LO * (F_HI / F_LO).powf((i as f32 + 0.5) / BANDS as f32);
            *t = 3.0 * (fc / 200.0).log2().max(0.0);
        }
        Self {
            rate,
            fps: rate / HOP as f32,
            fft,
            window,
            buf: VecDeque::from(vec![0.0; N]),
            pending: 0,
            scratch: vec![Complex32::default(); N],
            prev: vec![0.0; N / 2],
            edges,
            tilt,
            bass_bins: bin(BASS_HZ),
            peak_db: -90.0,
            energy_ref: 1e-4,
            flux_hist: VecDeque::new(),
            env: VecDeque::new(),
            onsets: VecDeque::new(),
            bass_hist: VecDeque::new(),
            last_onset: -1.0,
            t: 0.0,
        }
    }

    pub fn rate(&self) -> f32 {
        self.rate
    }

    /// Neuer Song: Tempo- und Stilgeschichte vergessen (Aussteuerung bleibt).
    pub fn reset_history(&mut self) {
        self.flux_hist.clear();
        self.env.clear();
        self.onsets.clear();
        self.bass_hist.clear();
    }

    /// Mono-Samples anhaengen; liefert fuer jede volle Schrittweite eine Analyse.
    pub fn push(&mut self, samples: &[f32], mut out: impl FnMut(Frame)) {
        for &s in samples {
            self.buf.pop_front();
            self.buf.push_back(s);
            self.pending += 1;
            if self.pending >= HOP {
                self.pending = 0;
                out(self.analyze());
            }
        }
    }

    fn analyze(&mut self) -> Frame {
        let mut sq = 0.0f32;
        for (i, (&s, w)) in self.buf.iter().zip(&self.window).enumerate() {
            sq += s * s;
            self.scratch[i] = Complex32::new(s * w, 0.0);
        }
        let rms = (sq / N as f32).sqrt();
        self.fft.process(&mut self.scratch);

        let half = N / 2;
        let mut flux = 0.0f32;
        let mut bass_flux = 0.0f32;
        let (mut bass_e, mut total_e) = (0.0f32, 0.0f32);
        for k in 1..half {
            let m = self.scratch[k].norm() / N as f32;
            let e = m * m;
            total_e += e;
            if k <= self.bass_bins {
                bass_e += e;
            }
            // log-komprimierter Fluss: nur deutliche Zunahmen zaehlen (neue Toene, Schlaege) —
            // gleichmaessige Toene schwanken von Fenster zu Fenster minimal, das ist kein Schlag
            // relativ zum Pegel, damit leise gehoerte Musik genauso Schlaege liefert
            let g = 300.0 / self.energy_ref;
            let d = (1.0 + g * m).ln() - (1.0 + g * self.prev[k]).ln() - 0.05;
            if d > 0.0 {
                flux += d;
                if k <= self.bass_bins {
                    bass_flux += d;
                }
            }
            self.prev[k] = m;
        }

        let silent = rms < 1e-4;
        let mut bands = [0f32; BANDS];
        let mut frame_peak = -120.0f32;
        let mut dbs = [0f32; BANDS];
        for i in 0..BANDS {
            let (a, b) = (self.edges[i], self.edges[i + 1].max(self.edges[i] + 1));
            let m = (a..b.min(half)).map(|k| self.scratch[k].norm() / N as f32).fold(0.0, f32::max);
            dbs[i] = 20.0 * (m + 1e-9).log10() + self.tilt[i];
            frame_peak = frame_peak.max(dbs[i]);
        }
        if !silent {
            // Aussteuerung: Spitze folgt sofort nach oben, faellt ~2 dB/s
            self.peak_db = frame_peak.max(self.peak_db - 2.0 / self.fps);
            for i in 0..BANDS {
                bands[i] = ((dbs[i] - (self.peak_db - RANGE_DB)) / RANGE_DB).clamp(0.0, 1.0);
            }
            self.energy_ref = rms.max(self.energy_ref * (1.0 - 0.1 / self.fps)).max(1e-4);
        }
        let energy = if silent { 0.0 } else { (rms / self.energy_ref).min(1.0) };

        // Schlag: deutlich ueber dem Fluss der letzten Sekunde, Bass zaehlt doppelt
        let f = if silent { 0.0 } else { flux + 2.0 * bass_flux };
        let (mean, std) = mean_std(&self.flux_hist);
        let kick = !silent
            && self.flux_hist.len() as f32 > self.fps * 0.5
            && f > mean + 1.4 * std
            && f > 2.0 * mean
            && f > MIN_FLUX
            && self.t - self.last_onset > 0.2;
        if kick {
            self.last_onset = self.t;
            self.onsets.push_back(self.t);
        }
        push_capped(&mut self.flux_hist, f, self.fps as usize);
        // Tempo-Huelle: nur erkannte Schlaege, alle gleich gewichtet (es zaehlt der Zeitpunkt).
        // Der rohe Fluss ist auch bei ruhigen Klaengen regelmaessig und taeuscht sonst einen Takt vor;
        // mit Staerke wuerde jeder zweite Kick bevorzugt (die Schwelle steigt nach einem lauten) -> halbes Tempo
        push_capped(&mut self.env, if kick { 1.0 } else { 0.0 }, (self.fps * WINDOW_SECS) as usize);
        push_capped(&mut self.bass_hist, (bass_e, total_e), (self.fps * WINDOW_SECS) as usize);
        while self.onsets.front().is_some_and(|&o| self.t - o > WINDOW_SECS) {
            self.onsets.pop_front();
        }
        self.t += HOP as f32 / self.rate;
        Frame { bands, energy, kick }
    }

    /// Eigenschaften der letzten ~8 s; None, solange zu wenig Musik da war.
    pub fn character(&self) -> Option<Character> {
        let n = self.env.len();
        if (n as f32) < self.fps * MIN_SECS {
            return None;
        }
        // Schlaege leicht verschmieren (+-2 Analysen), damit kleines Zittern im Timing nicht stoert
        let raw: Vec<f32> = self.env.iter().copied().collect();
        const K: [f32; 5] = [1.0, 2.0, 3.0, 2.0, 1.0];
        let smooth: Vec<f32> = (0..n)
            .map(|i| (0..5).filter_map(|j| (i + j).checked_sub(2).and_then(|p| raw.get(p)).map(|v| v * K[j])).sum::<f32>() / 9.0)
            .collect();
        let mean = smooth.iter().sum::<f32>() / n as f32;
        let x: Vec<f32> = smooth.iter().map(|v| v - mean).collect();
        let r0: f32 = x.iter().map(|v| v * v).sum();
        let (mut beat, mut bpm) = (0.0f32, 0.0f32);
        if r0 > 1e-9 {
            let ac = |lag: usize| -> f32 { x.iter().zip(&x[lag..]).map(|(a, b)| a * b).sum::<f32>() / r0 };
            let lag_of = |bpm: f32| (self.fps * 60.0 / bpm).round() as usize;
            let (lo, hi) = (lag_of(190.0).max(1), lag_of(70.0).min(n / 2));
            let mut best = (0usize, f32::MIN);
            for lag in lo..=hi {
                let r = ac(lag);
                if r > best.1 {
                    best = (lag, r);
                }
            }
            if best.0 > 0 {
                let (lag, r) = best;
                // feiner als ein Raster-Schritt: Parabel durch die Nachbarn
                let (a, c) = (ac(lag - 1), ac(lag + 1));
                let den = a - 2.0 * r + c;
                let shift = if den.abs() > 1e-6 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
                bpm = self.fps * 60.0 / (lag as f32 + shift);
                beat = r.clamp(0.0, 1.0);
                // halbes Tempo erwischt (z. B. Hardstyle als 75): verdoppeln, wenn das halbe Raster auch traegt
                // (das halbe Raster liegt oft zwischen zwei Messpunkten -> Nachbarn mitpruefen)
                let half = (lag / 2).max(2);
                if bpm < 90.0 && (half - 1..=half + 1).map(ac).fold(f32::MIN, f32::max) > 0.6 * r {
                    bpm *= 2.0;
                }
            }
        }
        let secs = n as f32 / self.fps;
        let (b, t) = self.bass_hist.iter().fold((0.0, 0.0), |(b, t), (x, y)| (b + x, t + y));
        Some(Character {
            bpm,
            beat,
            onsets: self.onsets.len() as f32 / secs,
            bass: if t > 1e-12 { b / t } else { 0.0 },
        })
    }
}

fn push_capped<T>(q: &mut VecDeque<T>, v: T, cap: usize) {
    q.push_back(v);
    while q.len() > cap.max(1) {
        q.pop_front();
    }
}

fn mean_std(q: &VecDeque<f32>) -> (f32, f32) {
    if q.is_empty() {
        return (0.0, 0.0);
    }
    let n = q.len() as f32;
    let mean = q.iter().sum::<f32>() / n;
    let var = q.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n;
    (mean, var.sqrt())
}

/// Stilwechsel erst, wenn der neue Stil mehrmals hintereinander gewinnt (kein Flackern);
/// nach einem Songwechsel genuegt die erste Einschaetzung.
pub struct StyleTracker {
    current: Style,
    candidate: Option<(Style, u32)>,
    fresh: bool,
}

impl StyleTracker {
    pub const CONFIRM: u32 = 3;

    pub fn new() -> Self {
        Self { current: Style::Pulse, candidate: None, fresh: true }
    }

    pub fn current(&self) -> Style {
        self.current
    }

    pub fn song_changed(&mut self) {
        self.candidate = None;
        self.fresh = true;
    }

    pub fn update(&mut self, s: Style) -> Style {
        if self.fresh {
            self.fresh = false;
            self.current = s;
            self.candidate = None;
        } else if s == self.current {
            self.candidate = None;
        } else {
            let n = match self.candidate {
                Some((c, n)) if c == s => n + 1,
                _ => 1,
            };
            if n >= Self::CONFIRM {
                self.current = s;
                self.candidate = None;
            } else {
                self.candidate = Some((s, n));
            }
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const RATE: u32 = 48_000;

    /// Signal erzeugen und komplett durch den Analyzer schicken.
    fn run(secs: f32, f: impl Fn(f32) -> f32) -> (Analyzer, Vec<Frame>) {
        let mut a = Analyzer::new(RATE);
        let samples: Vec<f32> = (0..(secs * RATE as f32) as usize).map(|i| f(i as f32 / RATE as f32)).collect();
        let mut frames = Vec::new();
        a.push(&samples, |fr| frames.push(fr));
        (a, frames)
    }

    /// Bassdrum: 55-Hz-Ton mit schnellem Abklingen, alle `60/bpm` Sekunden
    fn kick(t: f32, bpm: f32, gain: f32) -> f32 {
        let p = t % (60.0 / bpm);
        gain * (2.0 * PI * 55.0 * p).sin() * (-p * 25.0).exp()
    }

    /// kleines deterministisches Rauschen (Hi-Hat-artig)
    fn noise(t: f32) -> f32 {
        let x = (t * 12_345.678).sin() * 43_758.547;
        x - x.floor() - 0.5
    }

    #[test]
    fn sinus_landet_im_richtigen_band() {
        let (a, frames) = run(1.0, |t| 0.5 * (2.0 * PI * 1000.0 * t).sin());
        let last = frames.last().unwrap();
        let top = (0..BANDS).max_by(|&i, &j| last.bands[i].total_cmp(&last.bands[j])).unwrap();
        let hz = |i: usize| F_LO * (F_HI / F_LO).powf(i as f32 / BANDS as f32);
        assert!(hz(top) <= 1000.0 && 1000.0 <= hz(top + 1), "Band {top} ({}-{} Hz)", hz(top), hz(top + 1));
        assert!(a.rate() == 48_000.0);
    }

    #[test]
    fn stille_ist_null() {
        let (_, frames) = run(1.0, |_| 0.0);
        assert!(frames.iter().all(|f| f.energy == 0.0 && !f.kick && f.bands.iter().all(|&b| b == 0.0)));
    }

    #[test]
    fn hardstyle_150_bpm_wird_zu_spikes() {
        let (a, frames) = run(10.0, |t| kick(t, 150.0, 0.9) + 0.05 * noise(t));
        let c = a.character().unwrap();
        assert!((c.bpm - 150.0).abs() < 6.0, "{c:?}");
        assert!(c.beat >= 0.3, "{c:?}");
        assert!(frames.iter().filter(|f| f.kick).count() >= 20, "Kicks erkannt");
        assert_eq!(classify(&c), Style::Spikes, "{c:?}");
    }

    #[test]
    fn pop_120_bpm_mit_akkord_wird_zu_puls() {
        let chord = |t: f32| 0.25 * ((2.0 * PI * 262.0 * t).sin() + (2.0 * PI * 330.0 * t).sin() + (2.0 * PI * 392.0 * t).sin());
        let (a, _) = run(10.0, |t| kick(t, 120.0, 0.5) + chord(t));
        let c = a.character().unwrap();
        assert!((c.bpm - 120.0).abs() < 6.0, "{c:?}");
        assert_eq!(classify(&c), Style::Pulse, "{c:?}");
    }

    #[test]
    fn ruhige_klaenge_ohne_takt_werden_zur_welle() {
        let swell = |t: f32| 0.5 + 0.5 * (2.0 * PI * 0.15 * t).sin();
        let (a, _) = run(10.0, |t| {
            swell(t) * 0.2 * ((2.0 * PI * 220.0 * t).sin() + (2.0 * PI * 330.0 * t).sin() + (2.0 * PI * 440.0 * t).sin())
        });
        let c = a.character().unwrap();
        assert!(c.onsets < 1.2, "{c:?}");
        assert_eq!(classify(&c), Style::Wave, "{c:?}");
    }

    #[test]
    fn leise_gehoerter_hardstyle_bleibt_spikes() {
        let (a, _) = run(10.0, |t| 0.08 * (kick(t, 150.0, 0.9) + 0.05 * noise(t)));
        let c = a.character().unwrap();
        assert!((c.bpm - 150.0).abs() < 6.0, "{c:?}");
        assert_eq!(classify(&c), Style::Spikes, "{c:?}");
    }

    #[test]
    fn drum_and_bass_174_bpm() {
        let (a, _) = run(10.0, |t| kick(t, 174.0, 0.8) + 0.08 * noise(t));
        let c = a.character().unwrap();
        assert!((c.bpm - 174.0).abs() < 8.0, "{c:?}");
        assert_eq!(classify(&c), Style::Spikes, "{c:?}");
    }

    #[test]
    fn zu_wenig_musik_noch_keine_einschaetzung() {
        let (a, _) = run(2.0, |t| kick(t, 128.0, 0.8));
        assert!(a.character().is_none());
    }

    #[test]
    fn stil_wechselt_erst_nach_bestaetigung() {
        let mut s = StyleTracker::new();
        assert_eq!(s.update(Style::Wave), Style::Wave, "erste Einschaetzung sofort");
        assert_eq!(s.update(Style::Spikes), Style::Wave);
        assert_eq!(s.update(Style::Spikes), Style::Wave);
        assert_eq!(s.update(Style::Spikes), Style::Spikes, "nach drei Mal");
        assert_eq!(s.update(Style::Pulse), Style::Spikes);
        assert_eq!(s.update(Style::Spikes), Style::Spikes, "Ausreisser setzt zurueck");
        s.song_changed();
        assert_eq!(s.update(Style::Pulse), Style::Pulse, "neuer Song: sofort");
    }
}
