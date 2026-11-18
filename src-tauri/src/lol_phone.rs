//! League am iPhone: Spiel annehmen, Champions bannen und picken, im Stil des Clients.
//! Der PC reicht den Zustand der lokalen Client-Schnittstelle (LCU, siehe lol.rs) als kleines JSON an die App
//! (`GET /k/<s>/lol`) und fuehrt ihre Befehle aus (`POST /k/<s>/lol/<aktion>`). Namen und Bilder der Champions
//! holt die App selbst bei Riots Data Dragon; hier laufen nur Zahlen (Champion-IDs).
//!
//! Zusaetzlich meldet ein kleiner Waechter „Spiel gefunden“ und „Championauswahl“ per ntfy, damit das iPhone in
//! der Tasche davon erfaehrt (siehe iphone::notify).

use std::time::Duration;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::lol::{lockfile, Lcu};

/// Bereitschaftspruefung: Zustand und wie lange sie schon laeuft (der Client zaehlt aufwaerts)
fn ready(rc: &Value) -> Value {
    json!({
        "state": rc["state"].as_str().unwrap_or(""),
        "response": rc["playerResponse"].as_str().unwrap_or("None"),
        "elapsed": rc["timer"].as_f64().unwrap_or(0.0),
    })
}

fn slot(p: &Value) -> Value {
    json!({
        "cell": p["cellId"].as_i64().unwrap_or(-1),
        "champ": p["championId"].as_i64().unwrap_or(0),
        "intent": p["championPickIntent"].as_i64().unwrap_or(0),
        "pos": p["assignedPosition"].as_str().unwrap_or(""),
    })
}

/// Championauswahl aus der Sitzung des Clients: beide Teams, gebannte Champions, meine offene Aktion, Restzeit
fn select(s: &Value) -> Value {
    let me = s["localPlayerCellId"].as_i64().unwrap_or(-1);
    let team = |k: &str| s[k].as_array().map(|a| a.iter().map(slot).collect::<Vec<_>>()).unwrap_or_default();
    let mut action = Value::Null;
    let (mut my_bans, mut their_bans) = (Vec::new(), Vec::new());
    for a in s["actions"].as_array().into_iter().flatten().flat_map(|g| g.as_array().into_iter().flatten()) {
        let kind = a["type"].as_str().unwrap_or("");
        let champ = a["championId"].as_i64().unwrap_or(0);
        if kind == "ban" && a["completed"].as_bool() == Some(true) && champ > 0 {
            if a["isAllyAction"].as_bool() == Some(true) { my_bans.push(champ) } else { their_bans.push(champ) }
        }
        if (kind == "pick" || kind == "ban") && a["actorCellId"].as_i64() == Some(me) && a["isInProgress"].as_bool() == Some(true) && a["completed"].as_bool() != Some(true) {
            action = json!({ "id": a["id"].as_i64().unwrap_or(-1), "kind": kind, "champ": champ });
        }
    }
    json!({
        "me": me,
        "my": team("myTeam"),
        "their": team("theirTeam"),
        "myBans": my_bans,
        "theirBans": their_bans,
        "action": action,
        "timer": s["timer"]["phase"].as_str().unwrap_or(""),
        "left": (s["timer"]["adjustedTimeLeftInPhase"].as_f64().unwrap_or(0.0) / 1000.0).max(0.0),
    })
}

fn ids(v: Result<Value, String>) -> Value {
    v.ok().filter(Value::is_array).unwrap_or_else(|| json!([]))
}

/// Alles, was die App zeichnet. Ohne Client: `{"phase":"Offline"}`
pub fn state() -> Value {
    let Some(lcu) = lockfile() else { return json!({ "phase": "Offline" }) };
    let Some(phase) = lcu.phase() else { return json!({ "phase": "Offline" }) };
    let mut v = json!({ "phase": phase });
    match phase.as_str() {
        "Matchmaking" => {
            if let Ok(m) = lcu.call("GET", "/lol-lobby/v2/lobby/matchmaking/search-state", None) {
                v["search"] = json!({ "time": m["timeInQueue"].as_f64().unwrap_or(0.0), "estimate": m["estimatedQueueTime"].as_f64().unwrap_or(0.0) });
            }
        }
        "ReadyCheck" => {
            if let Ok(rc) = lcu.call("GET", "/lol-matchmaking/v1/ready-check", None) {
                v["ready"] = ready(&rc);
            }
        }
        "ChampSelect" => {
            if let Ok(s) = lcu.call("GET", "/lol-champ-select/v1/session", None) {
                let mut sel = select(&s);
                // waehlbare Champions nur, wenn ich dran bin (Liste ist gross)
                let list = match sel["action"]["kind"].as_str() {
                    Some("ban") => Some(ids(lcu.call("GET", "/lol-champ-select/v1/bannable-champion-ids", None))),
                    Some("pick") => Some(ids(lcu.call("GET", "/lol-champ-select/v1/pickable-champion-ids", None))),
                    _ => None,
                };
                if let Some(l) = list {
                    sel["choices"] = l;
                }
                v["select"] = sel;
            }
        }
        _ => {}
    }
    v
}

fn num(q: &dyn Fn(&str) -> Option<String>, k: &str) -> Result<i64, String> {
    q(k).and_then(|s| s.parse::<i64>().ok()).filter(|n| (0..100_000).contains(n)).ok_or_else(|| format!("{k} fehlt"))
}

/// Befehl der App ausfuehren. `q` liefert Werte aus der Adresse (`id`, `champ`).
/// „hover“ waehlt vor (die Mitspieler sehen es), „lock“ bestaetigt (Bann oder Pick, je nach Aktion).
pub fn act(what: &str, q: &dyn Fn(&str) -> Option<String>) -> Result<Value, String> {
    let lcu = lockfile().ok_or("League-Client laeuft nicht")?;
    match what {
        "accept" => lcu.call("POST", "/lol-matchmaking/v1/ready-check/accept", None)?,
        "decline" => lcu.call("POST", "/lol-matchmaking/v1/ready-check/decline", None)?,
        "hover" | "lock" => {
            let (id, champ) = (num(q, "id")?, num(q, "champ")?);
            lcu.call("PATCH", &format!("/lol-champ-select/v1/session/actions/{id}"), Some(json!({ "championId": champ })))?;
            if what == "lock" {
                lcu.call("POST", &format!("/lol-champ-select/v1/session/actions/{id}/complete"), None)?;
            }
            Value::Null
        }
        _ => return Err("unbekannte Aktion".into()),
    };
    Ok(json!({ "ok": true }))
}

/// Waechter: Mitteilung aufs iPhone, wenn ein Spiel gefunden ist oder die Championauswahl beginnt
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = String::new();
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let phase = lockfile().and_then(|l: Lcu| l.phase()).unwrap_or_default();
            if phase != last {
                match phase.as_str() {
                    "ReadyCheck" => crate::iphone::notify(&app, "Spiel gefunden", "In der Notch-App annehmen"),
                    "ChampSelect" => crate::iphone::notify(&app, "Championauswahl", "Bannen und picken in der Notch-App"),
                    _ => {}
                }
                last = phase;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Value {
        json!({
            "localPlayerCellId": 2,
            "myTeam": [{"cellId": 0, "championId": 11, "assignedPosition": "jungle"}, {"cellId": 2, "championId": 0, "championPickIntent": 51, "assignedPosition": "middle"}],
            "theirTeam": [{"cellId": 5, "championId": 0}],
            "actions": [
                [{"id": 0, "actorCellId": 0, "type": "ban", "championId": 157, "completed": true, "isAllyAction": true, "isInProgress": false},
                 {"id": 5, "actorCellId": 5, "type": "ban", "championId": 238, "completed": true, "isAllyAction": false, "isInProgress": false},
                 {"id": 2, "actorCellId": 2, "type": "ban", "championId": 0, "completed": false, "isAllyAction": true, "isInProgress": true}]
            ],
            "timer": {"phase": "BAN_PICK", "adjustedTimeLeftInPhase": 21500.0}
        })
    }

    #[test]
    fn auswahl_findet_meine_aktion_und_banns() {
        let s = select(&session());
        assert_eq!(s["action"], json!({"id": 2, "kind": "ban", "champ": 0}));
        assert_eq!(s["myBans"], json!([157]));
        assert_eq!(s["theirBans"], json!([238]));
        assert_eq!(s["my"][0]["pos"], "jungle");
        assert_eq!(s["left"], 21.5);
    }

    #[test]
    fn auswahl_ohne_eigene_aktion() {
        let mut j = session();
        j["actions"][0][2]["isInProgress"] = json!(false);
        assert!(select(&j)["action"].is_null());
        assert!(select(&json!({}))["my"].as_array().unwrap().is_empty());
    }

    #[test]
    fn befehle_pruefen_zahlen() {
        let q = |k: &str| match k { "id" => Some("3".to_string()), "champ" => Some("../x".to_string()), _ => None };
        assert!(num(&q, "id").is_ok());
        assert!(num(&q, "champ").is_err());
    }
}
