//! iPhone-Anrufe und -Nachrichten per Bluetooth LE: Apple Notification Center Service (ANCS), ohne App.
//!
//! Das iPhone bietet gekoppelten Zubehoergeraeten seine Benachrichtigungen an (so zeigen Uhren sie an). Der PC
//! verhaelt sich wie ein solches Zubehoer: einmal in den Windows-Bluetooth-Einstellungen koppeln, am iPhone
//! „Benachrichtigungen anzeigen“ erlauben. Die Notch abonniert dann die drei ANCS-Merkmale
//! (Notification Source, Data Source, Control Point), holt zu jeder neuen Meldung App, Titel und Text und zeigt
//! Anrufe (mit Annehmen/Ablehnen) und Nachrichten bekannter Messenger. Gesprochen wird am iPhone: Annehmen
//! loest dort den Anruf aus, der Ton laeuft nicht ueber den PC.
//!
//! Aus, bis der Schalter in Einstellungen › iPhone an ist (Nachrichtentexte erscheinen sonst auch in
//! Bildschirmfreigaben). Nur Windows; der Mac bekommt die gleichen Meldungen nicht auf diesem Weg.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

// ---------- Protokoll (plattformunabhaengig, getestet) ----------

const FLAG_SILENT: u8 = 1;
const FLAG_PREEXISTING: u8 = 4;
const FLAG_POSITIVE: u8 = 8;
const FLAG_NEGATIVE: u8 = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum EventKind {
    Added,
    Modified,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Event {
    pub kind: EventKind,
    pub flags: u8,
    pub category: u8,
    pub uid: u32,
}

/// Notification Source: Ereignis, Flags, Kategorie, Anzahl, UID (4 Byte, little endian)
pub(crate) fn parse_event(b: &[u8]) -> Option<Event> {
    if b.len() < 8 {
        return None;
    }
    let kind = match b[0] {
        0 => EventKind::Added,
        1 => EventKind::Modified,
        2 => EventKind::Removed,
        _ => return None,
    };
    Some(Event { kind, flags: b[1], category: b[2], uid: u32::from_le_bytes(b[4..8].try_into().ok()?) })
}

/// Control Point: App, Titel (64), Untertitel (64), Text (200) einer Meldung
pub(crate) fn attr_request(uid: u32) -> Vec<u8> {
    let mut v = vec![0];
    v.extend(uid.to_le_bytes());
    v.extend([0, 1, 64, 0, 2, 64, 0, 3, 200, 0]);
    v
}

/// Control Point: positive (Annehmen) oder negative (Ablehnen) Aktion einer Meldung
pub(crate) fn action_request(uid: u32, positive: bool) -> Vec<u8> {
    let mut v = vec![2];
    v.extend(uid.to_le_bytes());
    v.push(if positive { 0 } else { 1 });
    v
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Attrs {
    pub app: String,
    pub title: String,
    pub subtitle: String,
    pub message: String,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Parsed {
    /// UID, Attribute, verbrauchte Bytes
    Done(u32, Attrs, usize),
    /// Antwort kommt in mehreren Paketen: weiter sammeln
    More,
    Bad,
}

/// Data Source: Befehl 0, UID, dann je Attribut Kennung, Laenge (2 Byte) und Inhalt; wir fragen genau vier an
pub(crate) fn parse_attrs(b: &[u8]) -> Parsed {
    if b.len() < 5 {
        return Parsed::More;
    }
    if b[0] != 0 {
        return Parsed::Bad;
    }
    let uid = u32::from_le_bytes([b[1], b[2], b[3], b[4]]);
    let (mut at, mut a) = (5, Attrs::default());
    for _ in 0..4 {
        if b.len() < at + 3 {
            return Parsed::More;
        }
        let len = u16::from_le_bytes([b[at + 1], b[at + 2]]) as usize;
        if b.len() < at + 3 + len {
            return Parsed::More;
        }
        // die Laengen zaehlen Bytes: ein abgeschnittenes Zeichen am Ende wird zu U+FFFD und faellt weg
        let s = String::from_utf8_lossy(&b[at + 3..at + 3 + len]).trim_end_matches('\u{fffd}').to_string();
        match b[at] {
            0 => a.app = s,
            1 => a.title = s,
            2 => a.subtitle = s,
            3 => a.message = s,
            _ => {}
        }
        at += 3 + len;
    }
    Parsed::Done(uid, a, at)
}

#[derive(Debug, PartialEq)]
pub(crate) enum What {
    Call,
    Missed,
    Message(String),
    Ignore,
}

/// Messenger, deren Meldungen die Notch zeigt (Bundle-Kennung -> Name)
const MESSENGERS: &[(&str, &str)] = &[
    ("com.apple.MobileSMS", "Nachrichten"),
    ("net.whatsapp.WhatsApp", "WhatsApp"),
    ("ph.telegra.Telegraph", "Telegram"),
    ("org.whispersystems.signal", "Signal"),
    ("com.facebook.Messenger", "Messenger"),
    ("com.burbn.instagram", "Instagram"),
    ("com.hammerandchisel.discord", "Discord"),
    ("com.tinyspeck.chatlyio", "Slack"),
    ("com.apple.facetime", "FaceTime"),
];

/// Anrufe und verpasste Anrufe immer, sonst nur Messenger (alles andere waere ein Strom von Werbung)
pub(crate) fn classify(category: u8, app: &str) -> What {
    match category {
        1 => What::Call,
        2 | 3 => What::Missed,
        _ => match MESSENGERS.iter().find(|(id, _)| *id == app) {
            Some((_, name)) => What::Message((*name).to_string()),
            None => What::Ignore,
        },
    }
}

// ---------- Zustand ----------

static ENABLED: AtomicBool = AtomicBool::new(false);
/// "aus" | "suche" | "verbunden"
static STATE: Mutex<(&str, String)> = Mutex::new(("aus", String::new()));
/// UID des Anrufs, der gerade klingelt
static CALL: Mutex<Option<u32>> = Mutex::new(None);

fn flag_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("iphone-ancs.txt"))
}

fn set_state(app: &AppHandle, s: &'static str, device: &str) {
    let changed = {
        let mut st = STATE.lock().unwrap();
        let changed = st.0 != s || st.1 != device;
        *st = (s, device.to_string());
        changed
    };
    if changed {
        crate::share::status_event(app);
    }
}

/// fuer share::status (Einstellungen › iPhone)
pub fn status_json() -> Value {
    let st = STATE.lock().unwrap();
    json!({ "on": ENABLED.load(Ordering::Relaxed), "state": st.0, "device": st.1, "supported": cfg!(windows) })
}

/// Schalter in Einstellungen › iPhone
#[tauri::command]
pub fn ancs_set(app: AppHandle, on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if let Some(f) = flag_file(&app) {
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, if on { "1" } else { "0" });
    }
    if !on {
        set_state(&app, "aus", "");
    }
    crate::share::status_event(&app);
}

/// Knoepfe an der Anrufkarte
pub fn call_action(app: &AppHandle, action: &str) {
    let uid = *CALL.lock().unwrap();
    if let Some(uid) = uid.filter(|_| matches!(action, "answer" | "decline")) {
        if let Err(e) = perform(uid, action == "answer") {
            // Karte bleibt stehen (sie verschwindet nach der Klingeldauer von selbst), sonst klingelt es unbemerkt weiter
            eprintln!("[notch] ancs: {e}");
            crate::iphone::show_message(app, "Telefon", "Anruf", &format!("Aktion fehlgeschlagen: {e}"));
            return;
        }
    }
    *CALL.lock().unwrap() = None;
    crate::iphone::end_call(app);
}

#[cfg(not(windows))]
fn perform(_uid: u32, _positive: bool) -> Result<(), String> {
    Err("nur unter Windows".into())
}

#[cfg(not(windows))]
pub fn spawn(_app: AppHandle) {}

// ---------- Bluetooth LE (Windows) ----------

#[cfg(windows)]
static CONTROL: Mutex<Option<ble::Control>> = Mutex::new(None);

#[cfg(windows)]
fn perform(uid: u32, positive: bool) -> Result<(), String> {
    let c = CONTROL.lock().unwrap().clone().ok_or("iPhone nicht verbunden")?;
    ble::write(&c.0, &action_request(uid, positive))
}

#[cfg(windows)]
pub fn spawn(app: AppHandle) {
    let saved = flag_file(&app).and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|s| s.trim() == "1");
    ENABLED.store(saved, Ordering::Relaxed);
    std::thread::spawn(move || loop {
        if !ENABLED.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(3));
            continue;
        }
        set_state(&app, "suche", "");
        match ble::connect() {
            Ok(s) => {
                set_state(&app, "verbunden", &s.name);
                *CONTROL.lock().unwrap() = Some(s.control.clone());
                run(&app, &s);
                *CONTROL.lock().unwrap() = None;
                *CALL.lock().unwrap() = None;
                crate::iphone::end_call(&app);
                // Verbindung ging verloren: nicht sofort wieder anfunken
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_secs(10)),
        }
    });
}

#[cfg(windows)]
fn run(app: &AppHandle, s: &ble::Session) {
    use std::collections::HashMap;
    use std::sync::mpsc::RecvTimeoutError;
    let mut pending: HashMap<u32, Event> = HashMap::new();
    let mut buf: Vec<u8> = Vec::new();
    while ENABLED.load(Ordering::Relaxed) && s.connected() {
        match s.rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(ble::Msg::Source(b)) => {
                if let Some(ev) = parse_event(&b) {
                    on_event(app, s, ev, &mut pending);
                }
            }
            Ok(ble::Msg::Data(b)) => {
                buf.extend(b);
                loop {
                    match parse_attrs(&buf) {
                        Parsed::Done(uid, a, n) => {
                            buf.drain(..n);
                            if let Some(ev) = pending.remove(&uid) {
                                show(app, ev, a);
                            }
                        }
                        Parsed::More => break,
                        Parsed::Bad => {
                            buf.clear();
                            break;
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[cfg(windows)]
fn on_event(app: &AppHandle, s: &ble::Session, ev: Event, pending: &mut std::collections::HashMap<u32, Event>) {
    match ev.kind {
        EventKind::Removed => {
            pending.remove(&ev.uid);
            if CALL.lock().unwrap().is_some_and(|u| u == ev.uid) {
                *CALL.lock().unwrap() = None;
                crate::iphone::end_call(app);
            }
        }
        // „geaendert“ (z. B. Anrufdauer) und beim Verbinden Vorhandenes sind keine neuen Meldungen
        EventKind::Modified => {}
        EventKind::Added => {
            let call = ev.category == 1;
            if ev.flags & FLAG_PREEXISTING != 0 || (ev.flags & FLAG_SILENT != 0 && !call) {
                return;
            }
            if pending.len() > 32 {
                pending.clear();
            }
            pending.insert(ev.uid, ev);
            if let Err(e) = ble::write(&s.control.0, &attr_request(ev.uid)) {
                eprintln!("[notch] ancs: {e}");
            }
        }
    }
}

#[cfg(windows)]
fn show(app: &AppHandle, ev: Event, a: Attrs) {
    let label = MESSENGERS.iter().find(|(id, _)| *id == a.app).map(|(_, n)| *n).unwrap_or("Telefon");
    match classify(ev.category, &a.app) {
        What::Call => {
            *CALL.lock().unwrap() = Some(ev.uid);
            let who = if a.title.is_empty() { "Unbekannt".to_string() } else { a.title };
            crate::iphone::show_call(app, &who, label, ev.flags & FLAG_POSITIVE != 0, ev.flags & FLAG_NEGATIVE != 0);
        }
        What::Missed => crate::iphone::show_message(app, label, &a.title, "Verpasster Anruf"),
        What::Message(name) => {
            let from = if a.subtitle.is_empty() { a.title } else { format!("{} · {}", a.title, a.subtitle) };
            crate::iphone::show_message(app, &name, &from, &a.message);
        }
        What::Ignore => {}
    }
}

#[cfg(windows)]
mod ble {
    use std::sync::mpsc::{channel, Receiver, Sender};

    use windows::core::{Ref, GUID, HSTRING};
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus, GattDeviceService,
        GattSession, GattValueChangedEventArgs, GattWriteOption,
    };
    use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothConnectionStatus, BluetoothLEDevice};
    use windows::Devices::Enumeration::DeviceInformation;
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::{DataReader, DataWriter};

    const SERVICE: GUID = GUID::from_u128(0x7905F431_B5CE_4E99_A40F_4B1E122D00D0);
    const SOURCE: GUID = GUID::from_u128(0x9FBF120D_6301_42D9_8C58_25E699A21DBD);
    const CONTROL: GUID = GUID::from_u128(0x69D1D8F3_45E1_49A8_9821_9BBDFDAAD9D9);
    const DATA: GUID = GUID::from_u128(0x22EAC6E9_24D6_4BB5_BE44_B36ACE7C7BFB);

    pub enum Msg {
        Source(Vec<u8>),
        Data(Vec<u8>),
    }

    #[derive(Clone)]
    pub struct Control(pub GattCharacteristic);

    pub struct Session {
        pub name: String,
        pub control: Control,
        pub rx: Receiver<Msg>,
        dev: BluetoothLEDevice,
        // Dienst und Merkmale muessen leben, solange abonniert ist
        _keep: (GattDeviceService, GattCharacteristic, GattCharacteristic, GattSession),
    }

    impl Session {
        pub fn connected(&self) -> bool {
            self.dev.ConnectionStatus().is_ok_and(|c| c == BluetoothConnectionStatus::Connected)
        }
    }

    fn e(err: windows::core::Error) -> String {
        err.message()
    }

    pub fn write(c: &GattCharacteristic, bytes: &[u8]) -> Result<(), String> {
        let w = DataWriter::new().map_err(e)?;
        w.WriteBytes(bytes).map_err(e)?;
        let buf = w.DetachBuffer().map_err(e)?;
        let r = c.WriteValueWithOptionAsync(&buf, GattWriteOption::WriteWithResponse).map_err(e)?.join().map_err(e)?;
        if r == GattCommunicationStatus::Success { Ok(()) } else { Err(format!("Schreiben fehlgeschlagen ({r:?})")) }
    }

    fn characteristic(svc: &GattDeviceService, id: GUID) -> Result<GattCharacteristic, String> {
        let r = svc.GetCharacteristicsForUuidAsync(id).map_err(e)?.join().map_err(e)?;
        if r.Status().map_err(e)? != GattCommunicationStatus::Success {
            return Err("Merkmal nicht verfuegbar".into());
        }
        r.Characteristics().map_err(e)?.GetAt(0).map_err(e)
    }

    fn subscribe(ch: &GattCharacteristic, tx: Sender<Msg>, wrap: fn(Vec<u8>) -> Msg) -> Result<(), String> {
        ch.ValueChanged(&TypedEventHandler::new(move |_: Ref<GattCharacteristic>, args: Ref<GattValueChangedEventArgs>| {
            if let Some(a) = args.as_ref() {
                if let Ok(r) = a.CharacteristicValue().and_then(|b| DataReader::FromBuffer(&b)) {
                    let mut v = vec![0u8; r.UnconsumedBufferLength().unwrap_or(0) as usize];
                    if r.ReadBytes(&mut v).is_ok() {
                        let _ = tx.send(wrap(v));
                    }
                }
            }
            Ok(())
        }))
        .map_err(e)?;
        // beim ersten Mal fragt das iPhone hier, ob der PC Benachrichtigungen bekommen darf
        let st = ch
            .WriteClientCharacteristicConfigurationDescriptorAsync(GattClientCharacteristicConfigurationDescriptorValue::Notify)
            .map_err(e)?
            .join()
            .map_err(e)?;
        if st == GattCommunicationStatus::Success { Ok(()) } else { Err(format!("Abonnieren fehlgeschlagen ({st:?})")) }
    }

    /// Gekoppelte Bluetooth-LE-Geraete durchgehen und das nehmen, das ANCS anbietet (iPhone/iPad zuerst)
    pub fn connect() -> Result<Session, String> {
        let sel = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true).map_err(e)?;
        let found = DeviceInformation::FindAllAsyncAqsFilter(&sel).map_err(e)?.join().map_err(e)?;
        let mut list: Vec<(String, String)> = found
            .into_iter()
            .filter_map(|d| Some((d.Id().ok()?.to_string(), d.Name().ok()?.to_string())))
            .collect();
        let apple = |n: &str| {
            let n = n.to_lowercase();
            n.contains("iphone") || n.contains("ipad")
        };
        // sonst wuerde jedes gekoppelte Zubehoer (Lautsprecher, Controller) alle 10 s angefunkt
        let any_apple = list.iter().any(|(_, n)| apple(n));
        list.retain(|(_, n)| !any_apple || apple(n));
        for (id, name) in list {
            if let Ok(s) = open(&id, name) {
                return Ok(s);
            }
        }
        Err("kein iPhone mit ANCS gefunden".into())
    }

    fn open(id: &str, name: String) -> Result<Session, String> {
        let dev = BluetoothLEDevice::FromIdAsync(&HSTRING::from(id)).map_err(e)?.join().map_err(e)?;
        let r = dev.GetGattServicesForUuidWithCacheModeAsync(SERVICE, BluetoothCacheMode::Uncached).map_err(e)?.join().map_err(e)?;
        if r.Status().map_err(e)? != GattCommunicationStatus::Success || r.Services().map_err(e)?.Size().map_err(e)? == 0 {
            return Err("kein ANCS".into());
        }
        let svc = r.Services().map_err(e)?.GetAt(0).map_err(e)?;
        let (data, source, control) = (characteristic(&svc, DATA)?, characteristic(&svc, SOURCE)?, characteristic(&svc, CONTROL)?);
        // ohne Sitzung haelt Windows die LE-Verbindung nicht und ConnectionStatus meldet „getrennt“
        let session = GattSession::FromDeviceIdAsync(&dev.BluetoothDeviceId().map_err(e)?).map_err(e)?.join().map_err(e)?;
        session.SetMaintainConnection(true).map_err(e)?;
        let (tx, rx) = channel();
        // Data Source zuerst: die Antworten auf Anfragen duerfen nicht verloren gehen
        subscribe(&data, tx.clone(), Msg::Data)?;
        subscribe(&source, tx, Msg::Source)?;
        Ok(Session { name, control: Control(control), rx, dev, _keep: (svc, data, source, session) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(id: u8, s: &str) -> Vec<u8> {
        let mut v = vec![id];
        v.extend((s.len() as u16).to_le_bytes());
        v.extend(s.as_bytes());
        v
    }

    #[test]
    fn ereignis() {
        // hinzugefuegt, wichtig+positive Aktion, Anruf, 1 Meldung, UID 0x01020304
        let e = parse_event(&[0, 2 | FLAG_POSITIVE, 1, 1, 4, 3, 2, 1]).unwrap();
        assert_eq!((e.kind, e.category, e.uid), (EventKind::Added, 1, 0x0102_0304));
        assert!(e.flags & FLAG_POSITIVE != 0 && e.flags & FLAG_NEGATIVE == 0);
        assert_eq!(parse_event(&[2, 0, 4, 1, 9, 0, 0, 0]).unwrap().kind, EventKind::Removed);
        assert!(parse_event(&[0, 0, 1]).is_none());
        assert!(parse_event(&[7, 0, 1, 1, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn anfragen() {
        assert_eq!(&attr_request(0x0102_0304)[..6], &[0, 4, 3, 2, 1, 0]);
        assert_eq!(action_request(5, true), vec![2, 5, 0, 0, 0, 0]);
        assert_eq!(action_request(5, false), vec![2, 5, 0, 0, 0, 1]);
    }

    #[test]
    fn antwort_in_stuecken() {
        let mut full = vec![0, 9, 0, 0, 0];
        for (id, s) in [(0, "net.whatsapp.WhatsApp"), (1, "Max"), (2, ""), (3, "Hallo ä")] {
            full.extend(attr(id, s));
        }
        // an jeder Stelle abgeschnitten: weiter sammeln, nie falsch lesen
        for cut in 0..full.len() {
            assert_eq!(parse_attrs(&full[..cut]), Parsed::More, "bei {cut}");
        }
        let Parsed::Done(uid, a, n) = parse_attrs(&full) else { panic!() };
        assert_eq!((uid, n), (9, full.len()));
        assert_eq!((a.app.as_str(), a.title.as_str(), a.subtitle.as_str(), a.message.as_str()), ("net.whatsapp.WhatsApp", "Max", "", "Hallo ä"));
        // zwei Antworten hintereinander: nur die erste wird verbraucht
        let mut two = full.clone();
        two.extend(&full);
        assert!(matches!(parse_attrs(&two), Parsed::Done(_, _, n) if n == full.len()));
        assert_eq!(parse_attrs(&[1, 0, 0, 0, 0, 0]), Parsed::Bad);
    }

    #[test]
    fn abgeschnittenes_zeichen() {
        let mut v = vec![0, 1, 0, 0, 0];
        v.extend(attr(0, "a"));
        v.extend(attr(1, ""));
        v.extend(attr(2, ""));
        // „ä“ (2 Byte) nach 1 Byte abgeschnitten, wie bei 200 Byte Grenze
        v.extend([3, 2, 0, b'x', 0xC3]);
        let Parsed::Done(_, a, _) = parse_attrs(&v) else { panic!() };
        assert_eq!(a.message, "x");
    }

    #[test]
    fn auswahl() {
        assert_eq!(classify(1, "com.apple.mobilephone"), What::Call);
        assert_eq!(classify(2, ""), What::Missed);
        assert_eq!(classify(4, "net.whatsapp.WhatsApp"), What::Message("WhatsApp".into()));
        assert_eq!(classify(0, "com.apple.MobileSMS"), What::Message("Nachrichten".into()));
        assert_eq!(classify(4, "com.zalando.app"), What::Ignore);
        assert_eq!(classify(0, ""), What::Ignore);
    }
}
