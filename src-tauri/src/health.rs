//! Health-Export vom iPhone: die App schickt Werte aus Apple Health (Zepp/Amazfit, Essens-App, iPhone …),
//! die Notch legt sie je Tag als JSON-Zeilen unter `Dokumente\Notch\Health\JJJJ-MM-TT.jsonl` ab.
//! Eigene Programme (Gesundheitsanalyse) lesen die Dateien oder fragen die lokale Schnittstelle
//! (activities.rs, `127.0.0.1:47800/health-data/…`). Format und Beispiele: docs/health-export.md.
//!
//! Ein Wert = eine Zeile: `{"id","t","s","e","day","v","u","src",…}`. Gleiche `id` wird nie doppelt abgelegt,
//! darum darf die App jederzeit „alles neu senden“.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const MAX_RECORDS: usize = 20_000;

pub fn dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().document_dir().ok().map(|d| d.join("Notch").join("Health"))
}

fn valid_day(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

fn safe_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn day_file(dir: &Path, day: &str) -> PathBuf {
    dir.join(format!("{day}.jsonl"))
}

fn read_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path).map(|t| t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()).unwrap_or_default()
}

/// Uebersicht (`_status.json`): je Wertart Anzahl, Quellen, letzter Zeitpunkt
pub fn overview(dir: &Path) -> Value {
    fs::read_to_string(dir.join("_status.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({ "updated": 0, "types": {} }))
}

/// Werte der App ablegen. Rueckgabe: (neu, uebersprungen). Ungueltige oder doppelte Werte werden uebersprungen.
pub fn ingest_into(dir: &Path, body: &Value) -> Result<(usize, usize), String> {
    let recs = body["records"].as_array().ok_or("records fehlt")?;
    if recs.len() > MAX_RECORDS {
        return Err("zu viele Werte auf einmal".into());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut by_day: HashMap<&str, Vec<&Value>> = HashMap::new();
    for r in recs {
        let day = r["day"].as_str().unwrap_or("");
        if valid_day(day) && safe_id(r["id"].as_str().unwrap_or("")) && r["t"].as_str().is_some_and(|t| !t.is_empty()) {
            by_day.entry(day).or_default().push(r);
        }
    }
    let mut status = overview(dir);
    let mut added = 0;
    for (day, list) in by_day {
        let path = day_file(dir, day);
        let mut known: HashSet<String> = read_lines(&path).iter().filter_map(|r| r["id"].as_str().map(String::from)).collect();
        let mut file = fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
        for r in list {
            if !known.insert(r["id"].as_str().unwrap_or("").to_string()) {
                continue;
            }
            writeln!(file, "{r}").map_err(|e| e.to_string())?;
            added += 1;
            let t = &mut status["types"][r["t"].as_str().unwrap_or("")];
            t["count"] = json!(t["count"].as_u64().unwrap_or(0) + 1);
            let src = r["src"].as_str().unwrap_or("?");
            t["sources"][src] = json!(t["sources"][src].as_u64().unwrap_or(0) + 1);
            if r["e"].as_str() > t["last"].as_str() {
                t["last"] = r["e"].clone();
            }
        }
    }
    status["updated"] = json!(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    fs::write(dir.join("_status.json"), status.to_string()).map_err(|e| e.to_string())?;
    Ok((added, recs.len() - added))
}

/// Alle Tage, fuer die es Werte gibt (aufsteigend)
pub fn days_in(dir: &Path) -> Vec<String> {
    let mut d: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".jsonl")).filter(|d| valid_day(d)).map(String::from))
        .collect();
    d.sort();
    d
}

/// Werte von `from` bis `to` (einschliesslich, `JJJJ-MM-TT`), optional nur eine Wertart
pub fn range_in(dir: &Path, from: &str, to: &str, ty: Option<&str>) -> Result<Vec<Value>, String> {
    if !valid_day(from) || !valid_day(to) {
        return Err("Datum als JJJJ-MM-TT angeben (from, to)".into());
    }
    let mut out = Vec::new();
    for d in days_in(dir).iter().filter(|d| d.as_str() >= from && d.as_str() <= to) {
        out.extend(read_lines(&day_file(dir, d)).into_iter().filter(|r| ty.is_none_or(|t| r["t"].as_str() == Some(t))));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, day: &str, t: &str, src: &str) -> Value {
        json!({ "id": id, "t": t, "day": day, "s": format!("{day}T08:00:00+01:00"), "e": format!("{day}T08:01:00+01:00"), "v": 72.0, "u": "count/min", "src": src })
    }

    #[test]
    fn ablegen_ohne_doppelte_und_abfragen() {
        let d = std::env::temp_dir().join(format!("notch-health-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let body = json!({ "records": [rec("a1", "2026-11-17", "heartRate", "Zepp"), rec("a2", "2026-11-18", "heartRate", "Zepp"),
                                       rec("a3", "2026-11-18", "stepCount", "iPhone"), rec("../x", "2026-11-18", "heartRate", "x"),
                                       rec("a4", "kaputt", "heartRate", "x")] });
        assert_eq!(ingest_into(&d, &body).unwrap(), (3, 2));
        // zweites Mal: alles schon da
        assert_eq!(ingest_into(&d, &body).unwrap(), (0, 5));
        assert_eq!(days_in(&d), ["2026-11-17", "2026-11-18"]);
        assert_eq!(range_in(&d, "2026-11-18", "2026-11-18", None).unwrap().len(), 2);
        assert_eq!(range_in(&d, "2026-11-01", "2026-11-30", Some("heartRate")).unwrap().len(), 2);
        assert!(range_in(&d, "x", "y", None).is_err());
        let o = overview(&d);
        assert_eq!(o["types"]["heartRate"]["count"], 2);
        assert_eq!(o["types"]["heartRate"]["sources"]["Zepp"], 2);
        assert!(ingest_into(&d, &json!({})).is_err());
        let _ = fs::remove_dir_all(&d);
    }
}
