//! Apples Health-Export (`export.zip` aus Health › Profil › Alle Gesundheitsdaten exportieren) einlesen und in
//! dieselben Tagesdateien legen wie der Abgleich der iPhone-App (health.rs, Format: docs/health-export.md).
//! Das ist der Weg ohne Entwicklerkonto: Export am iPhone teilen › Notch, der PC liest ihn von selbst ein.
//!
//! Die Datei kann mehrere GB gross sein: die XML wird gestreamt, je 5000 Werte geschrieben.

use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde_json::{json, Value};

use crate::health::ingest_into;

/// (Health-Kennung ohne `HKQuantityTypeIdentifier`, Schluessel im Export, Einheit im Export). Gleiche Tabelle wie in der App.
const QUANTITIES: &[(&str, &str, &str)] = &[
    ("StepCount", "stepCount", "count"),
    ("DistanceWalkingRunning", "distanceWalkingRunning", "m"),
    ("DistanceCycling", "distanceCycling", "m"),
    ("DistanceSwimming", "distanceSwimming", "m"),
    ("FlightsClimbed", "flightsClimbed", "count"),
    ("ActiveEnergyBurned", "activeEnergyBurned", "kcal"),
    ("BasalEnergyBurned", "basalEnergyBurned", "kcal"),
    ("AppleExerciseTime", "exerciseTime", "min"),
    ("AppleStandTime", "standTime", "min"),
    ("HeartRate", "heartRate", "count/min"),
    ("RestingHeartRate", "restingHeartRate", "count/min"),
    ("WalkingHeartRateAverage", "walkingHeartRateAverage", "count/min"),
    ("HeartRateVariabilitySDNN", "heartRateVariabilitySDNN", "ms"),
    ("HeartRateRecoveryOneMinute", "heartRateRecoveryOneMinute", "count/min"),
    ("OxygenSaturation", "oxygenSaturation", "%"),
    ("RespiratoryRate", "respiratoryRate", "count/min"),
    ("BodyTemperature", "bodyTemperature", "degC"),
    ("AppleSleepingWristTemperature", "wristTemperature", "degC"),
    ("VO2Max", "vo2Max", "ml/kg/min"),
    ("BloodPressureSystolic", "bloodPressureSystolic", "mmHg"),
    ("BloodPressureDiastolic", "bloodPressureDiastolic", "mmHg"),
    ("BloodGlucose", "bloodGlucose", "mg/dL"),
    ("InsulinDelivery", "insulinDelivery", "IU"),
    ("BodyMass", "bodyMass", "kg"),
    ("Height", "height", "m"),
    ("BodyMassIndex", "bodyMassIndex", "kg/m2"),
    ("BodyFatPercentage", "bodyFatPercentage", "%"),
    ("LeanBodyMass", "leanBodyMass", "kg"),
    ("WaistCircumference", "waistCircumference", "m"),
    ("WalkingSpeed", "walkingSpeed", "m/s"),
    ("WalkingStepLength", "walkingStepLength", "m"),
    ("WalkingAsymmetryPercentage", "walkingAsymmetry", "%"),
    ("WalkingDoubleSupportPercentage", "walkingDoubleSupport", "%"),
    ("AppleWalkingSteadiness", "walkingSteadiness", "%"),
    ("SixMinuteWalkTestDistance", "sixMinuteWalkDistance", "m"),
    ("EnvironmentalAudioExposure", "environmentalAudioExposure", "dBASPL"),
    ("HeadphoneAudioExposure", "headphoneAudioExposure", "dBASPL"),
    ("DietaryEnergyConsumed", "dietaryEnergy", "kcal"),
    ("DietaryProtein", "dietaryProtein", "g"),
    ("DietaryCarbohydrates", "dietaryCarbohydrates", "g"),
    ("DietaryFatTotal", "dietaryFat", "g"),
    ("DietaryFatSaturated", "dietaryFatSaturated", "g"),
    ("DietaryFatMonounsaturated", "dietaryFatMonounsaturated", "g"),
    ("DietaryFatPolyunsaturated", "dietaryFatPolyunsaturated", "g"),
    ("DietarySugar", "dietarySugar", "g"),
    ("DietaryFiber", "dietaryFiber", "g"),
    ("DietaryCholesterol", "dietaryCholesterol", "mg"),
    ("DietarySodium", "dietarySodium", "mg"),
    ("DietaryPotassium", "dietaryPotassium", "mg"),
    ("DietaryCalcium", "dietaryCalcium", "mg"),
    ("DietaryIron", "dietaryIron", "mg"),
    ("DietaryVitaminC", "dietaryVitaminC", "mg"),
    ("DietaryVitaminD", "dietaryVitaminD", "mcg"),
    ("DietaryWater", "dietaryWater", "mL"),
    ("DietaryCaffeine", "dietaryCaffeine", "mg"),
    ("NumberOfAlcoholicBeverages", "alcoholicBeverages", "count"),
];

#[derive(Default, Debug)]
pub struct Import {
    /// neu abgelegt
    pub added: usize,
    /// Werte in der Datei (auch schon bekannte)
    pub seen: usize,
    /// Wertarten, die nicht uebernommen wurden (unbekannte Art oder Einheit)
    pub skipped: BTreeSet<String>,
}

/// "2026-11-18 08:00:00 +0100" -> "2026-11-18T08:00:00+01:00" (wie die App es schreibt)
fn iso(s: &str) -> Option<String> {
    let mut p = s.split(' ');
    let (d, t, z) = (p.next()?, p.next()?, p.next()?);
    (d.len() == 10 && t.len() == 8 && z.len() == 5).then(|| format!("{d}T{t}{}:{}", &z[..3], &z[3..]))
}

/// Wert in die feste Einheit umrechnen. `None` = Einheit unbekannt.
fn convert(v: f64, unit: &str, target: &str) -> Option<f64> {
    let u = unit.trim();
    Some(match (u, target) {
        ("%", "%") => v * 100.0, // Health speichert Prozent als 0..1 (vor „gleiche Einheit“)
        _ if u == target => v,
        ("km", "m") => v * 1000.0,
        ("mi", "m") => v * 1609.344,
        ("ft", "m") => v * 0.3048,
        ("cm", "m") => v / 100.0,
        ("Cal", "kcal") => v,
        ("kJ", "kcal") => v / 4.184,
        ("lb", "kg") => v * 0.453_592_37,
        ("degF", "degC") => (v - 32.0) / 1.8,
        ("km/hr", "m/s") => v / 3.6,
        ("mi/hr", "m/s") => v * 0.44704,
        ("mL/min·kg", "ml/kg/min") => v,
        ("L", "mL") => v * 1000.0,
        ("hr", "min") => v * 60.0,
        ("count", "kg/m2") => v,
        _ if u.starts_with("mg<") && target == "mg/dL" => v,
        _ if u.starts_with("mmol<") && target == "mg/dL" => v * 18.0156,
        _ => return None,
    })
}

fn attrs(e: &BytesStart) -> HashMap<String, String> {
    e.attributes()
        .flatten()
        .map(|a| (String::from_utf8_lossy(a.key.as_ref()).into_owned(), a.unescape_value().map(|c| c.into_owned()).unwrap_or_default()))
        .collect()
}

fn base(t: &str, a: &HashMap<String, String>, s: &str, e: &str, meta: &[(String, String)], end_day: bool) -> Value {
    let day_src = if end_day { e } else { s };
    let mut r = json!({ "t": t, "s": s, "e": e, "day": &day_src[..10], "src": a.get("sourceName").map(String::as_str).unwrap_or("?") });
    if let Some(d) = a.get("device").and_then(|d| d.split("name:").nth(1)).map(|d| d.split(',').next().unwrap_or("").trim().to_string()).filter(|d| !d.is_empty()) {
        r["dev"] = json!(d);
    }
    if !meta.is_empty() {
        r["m"] = Value::Object(meta.iter().map(|(k, v)| (k.clone(), json!(v))).collect());
    }
    r
}

/// Ein `<Record>` -> Wert. `Err(Art)` = nicht uebernommen.
fn record(a: &HashMap<String, String>, meta: &[(String, String)]) -> Result<Value, String> {
    let ty = a.get("type").map(String::as_str).unwrap_or("");
    let (s, e) = (a.get("startDate").and_then(|d| iso(d)), a.get("endDate").and_then(|d| iso(d)));
    let (Some(s), Some(e)) = (s, e) else { return Err(ty.to_string()) };
    if let Some(name) = ty.strip_prefix("HKQuantityTypeIdentifier") {
        let (_, key, target) = QUANTITIES.iter().find(|q| q.0 == name).ok_or_else(|| ty.to_string())?;
        let raw: f64 = a.get("value").and_then(|v| v.parse().ok()).ok_or_else(|| ty.to_string())?;
        let v = convert(raw, a.get("unit").map(String::as_str).unwrap_or(target), target).ok_or_else(|| format!("{ty} ({})", a.get("unit").cloned().unwrap_or_default()))?;
        let mut r = base(key, a, &s, &e, meta, false);
        r["v"] = json!(v);
        r["u"] = json!(target);
        return Ok(r);
    }
    match ty {
        "HKCategoryTypeIdentifierSleepAnalysis" => {
            let (n, l) = match a.get("value").map(String::as_str).unwrap_or("").trim_start_matches("HKCategoryValueSleepAnalysis") {
                "InBed" => (0, "inBed"),
                "Asleep" | "AsleepUnspecified" => (1, "asleep"),
                "Awake" => (2, "awake"),
                "AsleepCore" => (3, "core"),
                "AsleepDeep" => (4, "deep"),
                "AsleepREM" => (5, "rem"),
                _ => return Err(ty.to_string()),
            };
            let mut r = base("sleep", a, &s, &e, meta, true);
            r["v"] = json!(n);
            r["vl"] = json!(l);
            Ok(r)
        }
        "HKCategoryTypeIdentifierMindfulSession" => {
            let mut r = base("mindfulSession", a, &s, &e, meta, false);
            r["v"] = json!(secs(&s, &e));
            r["u"] = json!("s");
            Ok(r)
        }
        _ => Err(ty.to_string()),
    }
}

/// Sekunden zwischen zwei ISO-Zeiten derselben Zeitzone (genau genug fuer Achtsamkeit); sonst 0
fn secs(s: &str, e: &str) -> f64 {
    let t = |x: &str| -> Option<f64> {
        let (h, m, sec) = (x.get(11..13)?.parse::<f64>().ok()?, x.get(14..16)?.parse::<f64>().ok()?, x.get(17..19)?.parse::<f64>().ok()?);
        let day: f64 = x.get(8..10)?.parse().ok()?;
        Some(day * 86400.0 + h * 3600.0 + m * 60.0 + sec)
    };
    match (t(s), t(e)) {
        (Some(a), Some(b)) if b >= a => b - a,
        _ => 0.0,
    }
}

/// Ein `<Workout>` samt `<WorkoutStatistics>` -> Wert
fn workout(a: &HashMap<String, String>, stats: &[HashMap<String, String>], meta: &[(String, String)]) -> Result<Value, String> {
    let (s, e) = (a.get("startDate").and_then(|d| iso(d)), a.get("endDate").and_then(|d| iso(d)));
    let (Some(s), Some(e)) = (s, e) else { return Err("Workout".into()) };
    let dur: f64 = a.get("duration").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let seconds = match a.get("durationUnit").map(String::as_str) {
        Some("min") | None => dur * 60.0,
        Some("hr") => dur * 3600.0,
        Some("s") => dur,
        _ => return Err("Workout (Einheit)".into()),
    };
    let wt = a.get("workoutActivityType").map(|t| t.trim_start_matches("HKWorkoutActivityType").to_string()).unwrap_or_default();
    let name = match wt.as_str() {
        "Running" => "running".to_string(),
        "Walking" => "walking".into(),
        "Cycling" => "cycling".into(),
        "Swimming" => "swimming".into(),
        "Hiking" => "hiking".into(),
        "Yoga" => "yoga".into(),
        "TraditionalStrengthTraining" => "strength".into(),
        "FunctionalStrengthTraining" => "functionalStrength".into(),
        "HighIntensityIntervalTraining" => "hiit".into(),
        "Elliptical" => "elliptical".into(),
        "Rowing" => "rowing".into(),
        other => other.to_lowercase(),
    };
    let mut r = base("workout", a, &s, &e, meta, false);
    r["v"] = json!(seconds);
    r["u"] = json!("s");
    r["wt"] = json!(name);
    // aeltere Exporte: Summen als Attribute, neuere: <WorkoutStatistics>
    let mut kcal = a.get("totalEnergyBurned").and_then(|v| v.parse::<f64>().ok()).and_then(|v| convert(v, a.get("totalEnergyBurnedUnit").map(String::as_str).unwrap_or("kcal"), "kcal"));
    let mut dist = a.get("totalDistance").and_then(|v| v.parse::<f64>().ok()).and_then(|v| convert(v, a.get("totalDistanceUnit").map(String::as_str).unwrap_or("m"), "m"));
    for st in stats {
        let (Some(sum), Some(ty)) = (st.get("sum").and_then(|v| v.parse::<f64>().ok()), st.get("type")) else { continue };
        let unit = st.get("unit").map(String::as_str).unwrap_or("");
        if ty.ends_with("ActiveEnergyBurned") {
            kcal = convert(sum, unit, "kcal").or(kcal);
        } else if ty.ends_with("DistanceWalkingRunning") || ty.ends_with("DistanceCycling") || ty.ends_with("DistanceSwimming") {
            dist = convert(sum, unit, "m").or(dist);
        }
    }
    if let Some(k) = kcal {
        r["kcal"] = json!(k);
    }
    if let Some(d) = dist {
        r["dist"] = json!(d);
    }
    Ok(r)
}

fn hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(s.as_bytes()).iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// `export.xml` streamen; `progress` bekommt die Zahl der bisher gelesenen Werte
pub fn import_xml<R: BufRead>(dir: &Path, input: R, progress: &mut dyn FnMut(usize)) -> Result<Import, String> {
    let mut rd = Reader::from_reader(input);
    let mut buf = Vec::new();
    let mut out = Import::default();
    let mut batch: Vec<Value> = Vec::new();
    // offenes <Record>/<Workout> mit seinen Kindern
    let mut cur: Option<(bool, HashMap<String, String>, Vec<(String, String)>, Vec<HashMap<String, String>>)> = None;

    let mut flush = |batch: &mut Vec<Value>, out: &mut Import| -> Result<(), String> {
        if batch.is_empty() {
            return Ok(());
        }
        let (added, _) = ingest_into(dir, &json!({ "records": std::mem::take(batch) }))?;
        out.added += added;
        Ok(())
    };
    let mut finish = |c: (bool, HashMap<String, String>, Vec<(String, String)>, Vec<HashMap<String, String>>), batch: &mut Vec<Value>, out: &mut Import| {
        let (is_workout, a, meta, stats) = c;
        let r = if is_workout { workout(&a, &stats, &meta) } else { record(&a, &meta) };
        match r {
            Ok(mut v) => {
                // Apples Export hat keine UUIDs: Kennung aus dem Inhalt, doppelte erkennt die Ablage ohnehin am Inhalt
                v["id"] = json!(format!("x{}", hex(&v.to_string())));
                out.seen += 1;
                batch.push(v);
            }
            Err(t) => {
                out.skipped.insert(t);
            }
        }
    };

    loop {
        buf.clear();
        match rd.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("export.xml ist beschaedigt (Position {}): {e}", rd.buffer_position())),
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"Record" => cur = Some((false, attrs(&e), Vec::new(), Vec::new())),
                b"Workout" => cur = Some((true, attrs(&e), Vec::new(), Vec::new())),
                b"MetadataEntry" => push_meta(&mut cur, &e),
                b"WorkoutStatistics" => push_stat(&mut cur, &e),
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"Record" => finish((false, attrs(&e), Vec::new(), Vec::new()), &mut batch, &mut out),
                b"Workout" => finish((true, attrs(&e), Vec::new(), Vec::new()), &mut batch, &mut out),
                b"MetadataEntry" => push_meta(&mut cur, &e),
                b"WorkoutStatistics" => push_stat(&mut cur, &e),
                _ => {}
            },
            Ok(Event::End(e)) if matches!(e.name().as_ref(), b"Record" | b"Workout") => {
                if let Some(c) = cur.take() {
                    finish(c, &mut batch, &mut out);
                }
            }
            Ok(_) => {}
        }
        if batch.len() >= 5000 {
            flush(&mut batch, &mut out)?;
            progress(out.seen);
        }
    }
    flush(&mut batch, &mut out)?;
    progress(out.seen);
    Ok(out)
}

fn push_meta(cur: &mut Option<(bool, HashMap<String, String>, Vec<(String, String)>, Vec<HashMap<String, String>>)>, e: &BytesStart) {
    if let Some(c) = cur {
        let a = attrs(e);
        if let (Some(k), Some(v)) = (a.get("key"), a.get("value")) {
            c.2.push((k.clone(), v.clone()));
        }
    }
}

fn push_stat(cur: &mut Option<(bool, HashMap<String, String>, Vec<(String, String)>, Vec<HashMap<String, String>>)>, e: &BytesStart) {
    if let Some(c) = cur {
        c.3.push(attrs(e));
    }
}

/// `export.zip` (die `export.xml` darin wird gesucht) oder eine lose `export.xml`
pub fn import_file(dir: &Path, path: &Path, progress: &mut dyn FnMut(usize)) -> Result<Import, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("xml")) {
        return import_xml(dir, BufReader::with_capacity(1 << 20, file), progress);
    }
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("keine gueltige Export-Datei: {e}"))?;
    let idx = (0..zip.len())
        .find(|&i| zip.by_index(i).is_ok_and(|f| f.name().to_lowercase().ends_with("/export.xml") || f.name().eq_ignore_ascii_case("export.xml")))
        .ok_or("export.xml fehlt in der Datei")?;
    let entry = zip.by_index(idx).map_err(|e| e.to_string())?;
    import_xml(dir, BufReader::with_capacity(1 << 20, entry), progress)
}

/// Sieht der Dateiname nach Apples Export aus? (`export.zip`, auch `export 2.zip`, oder `export.xml`)
pub fn looks_like_export(name: &str) -> bool {
    let n = name.to_lowercase();
    (n.starts_with("export") && (n.ends_with(".zip") || n.ends_with(".xml"))) && !n.starts_with("export_cda")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<HealthData locale="de_DE">
 <Record type="HKQuantityTypeIdentifierHeartRate" sourceName="Zepp" unit="count/min" startDate="2026-11-18 08:00:00 +0100" endDate="2026-11-18 08:00:00 +0100" value="72"/>
 <Record type="HKQuantityTypeIdentifierDietaryEnergyConsumed" sourceName="Yazio" unit="Cal" startDate="2026-11-18 12:30:00 +0100" endDate="2026-11-18 12:30:00 +0100" value="450.5">
  <MetadataEntry key="HKFoodType" value="Haferflocken"/>
 </Record>
 <Record type="HKQuantityTypeIdentifierOxygenSaturation" sourceName="Zepp" unit="%" startDate="2026-11-18 03:00:00 +0100" endDate="2026-11-18 03:00:00 +0100" value="0.97"/>
 <Record type="HKQuantityTypeIdentifierDistanceWalkingRunning" sourceName="iPhone" unit="km" startDate="2026-11-18 09:00:00 +0100" endDate="2026-11-18 09:10:00 +0100" value="1.2"/>
 <Record type="HKQuantityTypeIdentifierBloodGlucose" sourceName="Libre" unit="mmol&lt;180.1559&gt;/L" startDate="2026-11-18 09:00:00 +0100" endDate="2026-11-18 09:00:00 +0100" value="5.5"/>
 <Record type="HKCategoryTypeIdentifierSleepAnalysis" sourceName="Zepp" startDate="2026-11-17 23:30:00 +0100" endDate="2026-11-18 06:30:00 +0100" value="HKCategoryValueSleepAnalysisAsleepDeep"/>
 <Record type="HKQuantityTypeIdentifierUnbekannt" sourceName="X" unit="count" startDate="2026-11-18 08:00:00 +0100" endDate="2026-11-18 08:00:00 +0100" value="1"/>
 <Workout workoutActivityType="HKWorkoutActivityTypeRunning" duration="30" durationUnit="min" sourceName="Zepp" startDate="2026-11-18 17:00:00 +0100" endDate="2026-11-18 17:30:00 +0100">
  <WorkoutStatistics type="HKQuantityTypeIdentifierActiveEnergyBurned" sum="300" unit="kcal"/>
  <WorkoutStatistics type="HKQuantityTypeIdentifierDistanceWalkingRunning" sum="5" unit="km"/>
 </Workout>
</HealthData>"#;

    #[test]
    fn export_einlesen_umrechnen_und_nicht_doppelt() {
        let d = std::env::temp_dir().join(format!("notch-health-import-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let r = import_xml(&d, XML.as_bytes(), &mut |_| {}).unwrap();
        assert_eq!((r.added, r.seen), (7, 7));
        assert!(r.skipped.iter().any(|s| s.contains("Unbekannt")));

        let day = crate::health::range_in(&d, "2026-11-18", "2026-11-18", None).unwrap();
        let get = |t: &str| day.iter().find(|r| r["t"] == t).unwrap_or_else(|| panic!("{t} fehlt")).clone();
        assert_eq!(get("dietaryEnergy")["m"]["HKFoodType"], "Haferflocken");
        assert_eq!(get("dietaryEnergy")["v"], 450.5);
        assert!((get("oxygenSaturation")["v"].as_f64().unwrap() - 97.0).abs() < 1e-9);
        assert!((get("distanceWalkingRunning")["v"].as_f64().unwrap() - 1200.0).abs() < 1e-9);
        assert!((get("bloodGlucose")["v"].as_f64().unwrap() - 99.09).abs() < 0.01);
        assert_eq!(get("sleep")["vl"], "deep"); // zaehlt zum Aufwach-Tag
        assert_eq!(get("sleep")["s"], "2026-11-17T23:30:00+01:00");
        let w = get("workout");
        assert_eq!((w["v"].as_f64().unwrap(), w["kcal"].as_f64().unwrap(), w["dist"].as_f64().unwrap(), w["wt"].as_str().unwrap()), (1800.0, 300.0, 5000.0, "running"));

        // noch einmal: nichts doppelt
        assert_eq!(import_xml(&d, XML.as_bytes(), &mut |_| {}).unwrap().added, 0);
        // derselbe Puls, wie ihn die App schickt (andere id), ist auch nicht neu
        let app = json!({ "records": [{ "id": "UUID-1", "t": "heartRate", "day": "2026-11-18", "s": "2026-11-18T08:00:00+01:00", "e": "2026-11-18T08:00:00+01:00", "v": 72.0, "u": "count/min", "src": "Zepp" }] });
        assert_eq!(ingest_into(&d, &app).unwrap().0, 0);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn zip_wie_von_apple() {
        use std::io::Write;
        let d = std::env::temp_dir().join(format!("notch-health-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let zp = d.join("export.zip");
        let mut z = zip::ZipWriter::new(File::create(&zp).unwrap());
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("apple_health_export/export_cda.xml", opt).unwrap();
        z.write_all(b"<ClinicalDocument/>").unwrap();
        z.start_file("apple_health_export/export.xml", opt).unwrap();
        z.write_all(XML.as_bytes()).unwrap();
        z.finish().unwrap();
        let r = import_file(&d.join("daten"), &zp, &mut |_| {}).unwrap();
        assert_eq!(r.added, 7);
        assert!(import_file(&d, &d.join("fehlt.zip"), &mut |_| {}).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn dateinamen_und_zeit() {
        assert!(looks_like_export("export.zip") && looks_like_export("Export 2.zip") && looks_like_export("export.xml"));
        assert!(!looks_like_export("export_cda.xml") && !looks_like_export("urlaub.zip"));
        assert_eq!(iso("2026-11-18 08:00:00 +0100").unwrap(), "2026-11-18T08:00:00+01:00");
        assert!(iso("kaputt").is_none());
    }
}
