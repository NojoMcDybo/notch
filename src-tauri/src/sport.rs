//! Live-Sport in der Notch: Spielstaende, Ticker und Ballverlauf.
//!
//! Quellen — alle ohne Konto und ohne Schluessel:
//! - ESPN (site.api.espn.com): Spielstaende, Tore und Karten fuer Fussball und US-Sport. Oeffentlich
//!   abrufbar, aber inoffiziell (keine Zusage, nur fuer den privaten Gebrauch) — kann sich jederzeit aendern.
//! - ESPN Core (sports.core.api.espn.com): jede Ballaktion mit Feldposition und Uhrzeit (Pass von wo
//!   nach wo, Schuss, Zweikampf …). Nur fuer das eine Spiel, das man gerade aufgeklappt ansieht.
//! - OpenLigaDB (api.openligadb.de): offiziell frei, von der Community gepflegt. Hauptquelle fuer
//!   3. Liga und Frauen-Bundesliga, Ausweichquelle fuer Bundesliga, 2. Liga und DFB-Pokal, wenn ESPN ausfaellt.
//!
//! Echte Positionsdaten aller Spieler (Tracking) gibt es live nirgends frei. Die Notch zeigt
//! stattdessen den Ballverlauf aus den Ballaktionen mit Rueckennummern.
//!
//! Einstellungen: Schema im Frontend (settings-model.ts -> "sport"). Events an die Notch:
//!   "sport"        { matches, error, updated }   alle Spiele im Blick (nur bei Aenderung)
//!   "sport-news"   News                          neue Meldung (Tor, Karte, Anpfiff …)
//!   "sport-plays"  { key, reset, plays }         Ballaktionen des angesehenen Spiels

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

const ESPN: &str = "https://site.api.espn.com/apis/site/v2/sports";
const CORE: &str = "https://sports.core.api.espn.com/v2/sports";
const OLDB: &str = "https://api.openligadb.de";

// ---------- Wettbewerbe ----------

pub struct League {
    pub id: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    /// ESPN-Sportart: soccer | football | basketball | hockey | baseball
    pub sport: &'static str,
    /// ESPN-Ligen, z. B. "ger.1"
    pub espn: &'static [&'static str],
    /// OpenLigaDB-Kuerzel: Hauptquelle, wenn `espn` leer ist, sonst Ausweichquelle
    pub oldb: Option<&'static str>,
    /// nur Spiele mit dieser Mannschaft (deutscher Name)
    pub team: Option<&'static str>,
}

const NATIONS: &[&str] = &["fifa.friendly", "uefa.nations", "fifa.worldq.uefa", "uefa.euroq", "fifa.world", "uefa.euro"];

pub const LEAGUES: &[League] = &[
    League { id: "bl1", name: "Bundesliga", group: "Deutschland", sport: "soccer", espn: &["ger.1"], oldb: Some("bl1"), team: None },
    League { id: "bl2", name: "2. Bundesliga", group: "Deutschland", sport: "soccer", espn: &["ger.2"], oldb: Some("bl2"), team: None },
    League { id: "bl3", name: "3. Liga", group: "Deutschland", sport: "soccer", espn: &[], oldb: Some("bl3"), team: None },
    League { id: "dfb", name: "DFB-Pokal", group: "Deutschland", sport: "soccer", espn: &["ger.dfb_pokal"], oldb: Some("dfb"), team: None },
    League { id: "ffb1", name: "Frauen-Bundesliga", group: "Deutschland", sport: "soccer", espn: &[], oldb: Some("ffb1"), team: None },
    League { id: "dfbteam", name: "Länderspiele Deutschland", group: "Länderspiele", sport: "soccer", espn: NATIONS, oldb: None, team: Some("Deutschland") },
    League { id: "turnier", name: "WM & EM – alle Spiele", group: "Länderspiele", sport: "soccer", espn: &["fifa.world", "uefa.euro"], oldb: None, team: None },
    League { id: "ucl", name: "Champions League", group: "Europa", sport: "soccer", espn: &["uefa.champions"], oldb: None, team: None },
    League { id: "uel", name: "Europa League", group: "Europa", sport: "soccer", espn: &["uefa.europa"], oldb: None, team: None },
    League { id: "uecl", name: "Conference League", group: "Europa", sport: "soccer", espn: &["uefa.europa.conf"], oldb: None, team: None },
    League { id: "epl", name: "Premier League", group: "Europa", sport: "soccer", espn: &["eng.1"], oldb: None, team: None },
    League { id: "laliga", name: "LaLiga", group: "Europa", sport: "soccer", espn: &["esp.1"], oldb: None, team: None },
    League { id: "seriea", name: "Serie A", group: "Europa", sport: "soccer", espn: &["ita.1"], oldb: None, team: None },
    League { id: "ligue1", name: "Ligue 1", group: "Europa", sport: "soccer", espn: &["fra.1"], oldb: None, team: None },
    League { id: "nfl", name: "NFL", group: "US-Sport", sport: "football", espn: &["nfl"], oldb: None, team: None },
    League { id: "nba", name: "NBA", group: "US-Sport", sport: "basketball", espn: &["nba"], oldb: None, team: None },
    League { id: "nhl", name: "NHL", group: "US-Sport", sport: "hockey", espn: &["nhl"], oldb: None, team: None },
    League { id: "mlb", name: "MLB", group: "US-Sport", sport: "baseball", espn: &["mlb"], oldb: None, team: None },
];

fn league(id: &str) -> Option<&'static League> {
    LEAGUES.iter().find(|l| l.id == id)
}

// ---------- Daten fuer die Notch ----------

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Team {
    /// "soccer:124" (ESPN) bzw. "oldb:7" (OpenLigaDB)
    pub id: String,
    pub name: String,
    pub short: String,
    pub abbr: String,
    pub logo: String,
    pub color: String,
    pub score: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Ev {
    pub id: String,
    pub minute: String,
    /// goal | red | yellow | sub | miss | var | wood | chance | corner | offside | kickoff | half | end | score | info
    pub kind: String,
    /// home | away | ""
    pub side: String,
    pub title: String,
    pub text: String,
    /// Wichtigkeit: 3 Tor/Rot, 2 Anpfiff/Halbzeit/Abpfiff/Elfer verschossen/Videobeweis, 1 Gelb/Wechsel/Pfosten, 0 Rest
    pub big: u8,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Match {
    pub key: String,
    pub league: String,
    pub league_name: String,
    pub sport: String,
    pub home: Team,
    pub away: Team,
    /// pre | in | post
    pub state: String,
    pub clock: String,
    /// Anstoss (ms seit 1970)
    pub start: u64,
    pub fav: bool,
    pub link: String,
    /// Ballverlauf verfuegbar (ESPN-Fussball)
    pub pitch: bool,
    /// Ticker, aelteste zuerst
    pub events: Vec<Ev>,
    pub source: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct News {
    pub key: String,
    pub ev: Ev,
    pub score: String,
    pub at: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Play {
    pub id: String,
    /// Zeitpunkt der Aktion (ms seit 1970)
    pub t: u64,
    pub minute: String,
    pub kind: String,
    pub side: String,
    pub jersey: String,
    pub who: String,
    /// Feldposition 0..100, Heim spielt immer nach rechts (x = 100 ist das Tor der Gaeste)
    pub x: f32,
    pub y: f32,
    pub x2: Option<f32>,
    pub y2: Option<f32>,
}

// ---------- Hilfen ----------

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// "2026-10-09T18:30Z", "2026-09-19T16:30:00Z", "2026-09-19T13:55:02.123Z" -> ms seit 1970 (UTC)
pub fn parse_utc(t: &str) -> Option<u64> {
    let t = t.trim().trim_end_matches('Z');
    let (date, time) = t.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let time = time.split(['+', '.']).next()?;
    let mut tm = time.split(':').map(|x| x.parse::<i64>().ok());
    let (h, mi) = (tm.next()??, tm.next()??);
    let sec = tm.next().flatten().unwrap_or(0);
    // Tage seit 1970 (Howard Hinnant, days_from_civil)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let ms = ((days * 86_400) + h * 3600 + mi * 60 + sec) * 1000;
    (ms >= 0).then_some(ms as u64)
}

/// ESPN-Minute "45'+2'" -> "45+2'", "67'" bleibt
pub fn minute_de(m: &str) -> String {
    let m = m.trim();
    if m.is_empty() {
        return String::new();
    }
    match m.split_once("'+") {
        Some((a, b)) => format!("{a}+{}'", b.trim_end_matches('\'')),
        None => m.to_string(),
    }
}

/// Spielername aus einem ESPN-Text: "Moritz Nicolas (Borussia M.) Pass at 19'" -> "Moritz Nicolas",
/// "Attempt missed. Anthony Caci (Mainz) right footed …" -> "Anthony Caci", "Foul by X (Y)." -> "X"
pub fn name_from_text(text: &str) -> String {
    let Some(i) = text.find(" (") else { return String::new() };
    let head = &text[..i];
    let head = head.rsplit(". ").next().unwrap_or(head);
    let head = ["Foul by ", "Handball by ", "Penalty conceded by "]
        .iter()
        .fold(head, |h, p| h.strip_prefix(p).unwrap_or(h));
    head.trim().to_string()
}

/// "#rrggbb" aus ESPN-"rrggbb"; leer, wenn ungueltig
fn hex(c: &str) -> String {
    let c = c.trim().trim_start_matches('#');
    if c.len() == 6 && c.chars().all(|x| x.is_ascii_hexdigit()) { format!("#{}", c.to_ascii_lowercase()) } else { String::new() }
}

fn rgb(c: &str) -> Option<(f32, f32, f32)> {
    let c = c.trim_start_matches('#');
    if c.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).ok().map(|v| v as f32);
    Some((p(0)?, p(2)?, p(4)?))
}

fn luma(c: &str) -> f32 {
    rgb(c).map_or(0.0, |(r, g, b)| 0.2126 * r + 0.7152 * g + 0.0722 * b)
}

fn color_dist(a: &str, b: &str) -> f32 {
    match (rgb(a), rgb(b)) {
        (Some(x), Some(y)) => ((x.0 - y.0).powi(2) + (x.1 - y.1).powi(2) + (x.2 - y.2).powi(2)).sqrt(),
        _ => 0.0,
    }
}

/// Teamfarben, die auf der schwarzen Notch sichtbar und voneinander unterscheidbar sind
pub fn pick_colors(home: (&str, &str), away: (&str, &str)) -> (String, String) {
    let ok = |c: &str| !c.is_empty() && luma(c) > 45.0;
    let first = |(a, b): (&str, &str), fallback: &str| {
        let (a, b) = (hex(a), hex(b));
        if ok(&a) { a } else if ok(&b) { b } else { fallback.to_string() }
    };
    let h = first(home, "#4da3ff");
    let mut a = first(away, "#ff6b6b");
    if color_dist(&h, &a) < 90.0 {
        let alt = hex(away.1);
        a = if ok(&alt) && color_dist(&h, &alt) >= 90.0 {
            alt
        } else if color_dist(&h, "#ff6b6b") >= 90.0 {
            "#ff6b6b".into()
        } else {
            "#4da3ff".into()
        };
    }
    (h, a)
}

// ---------- Deutsche Namen ----------

/// ESPN-Vereine mit englischem oder unvollstaendigem Namen: id -> (Name, kurz, Kuerzel)
const CLUBS: &[(&str, &str, &str, &str)] = &[
    ("132", "FC Bayern München", "Bayern", "FCB"),
    ("124", "Borussia Dortmund", "Dortmund", "BVB"),
    ("122", "1. FC Köln", "Köln", "KOE"),
    ("127", "Hamburger SV", "HSV", "HSV"),
    ("2950", "1. FSV Mainz 05", "Mainz", "M05"),
    ("270", "FC St. Pauli", "St. Pauli", "STP"),
    ("3070", "SpVgg Greuther Fürth", "Fürth", "SGF"),
    ("7013", "VfL Osnabrück", "Osnabrück", "OSN"),
    ("3067", "Eintracht Braunschweig", "Braunschweig", "EBS"),
    ("7017", "Dynamo Dresden", "Dresden", "SGD"),
    ("130", "1. FC Kaiserslautern", "Lautern", "FCK"),
    ("129", "Hertha BSC", "Hertha", "BSC"),
    ("269", "1. FC Nürnberg", "Nürnberg", "FCN"),
    ("4471", "Karlsruher SC", "Karlsruhe", "KSC"),
    ("268", "Borussia Mönchengladbach", "Gladbach", "BMG"),
];

/// Laendernamen englisch -> deutsch (Nationalmannschaften)
const COUNTRIES: &[(&str, &str)] = &[
    ("Germany", "Deutschland"), ("Austria", "Österreich"), ("Switzerland", "Schweiz"), ("France", "Frankreich"),
    ("Spain", "Spanien"), ("Italy", "Italien"), ("Netherlands", "Niederlande"), ("Belgium", "Belgien"),
    ("Denmark", "Dänemark"), ("Sweden", "Schweden"), ("Norway", "Norwegen"), ("Finland", "Finnland"),
    ("Poland", "Polen"), ("Czechia", "Tschechien"), ("Czech Republic", "Tschechien"), ("Slovakia", "Slowakei"),
    ("Hungary", "Ungarn"), ("Croatia", "Kroatien"), ("Serbia", "Serbien"), ("Slovenia", "Slowenien"),
    ("Bosnia-Herzegovina", "Bosnien-Herzegowina"), ("Bosnia and Herzegovina", "Bosnien-Herzegowina"),
    ("Albania", "Albanien"), ("North Macedonia", "Nordmazedonien"), ("Greece", "Griechenland"),
    ("Turkey", "Türkei"), ("Türkiye", "Türkei"), ("Romania", "Rumänien"), ("Bulgaria", "Bulgarien"),
    ("Russia", "Russland"), ("Moldova", "Moldau"), ("Georgia", "Georgien"), ("Armenia", "Armenien"),
    ("Azerbaijan", "Aserbaidschan"), ("Kazakhstan", "Kasachstan"), ("Scotland", "Schottland"),
    ("Northern Ireland", "Nordirland"), ("Republic of Ireland", "Irland"), ("Ireland", "Irland"),
    ("Iceland", "Island"), ("Faroe Islands", "Färöer"), ("Luxembourg", "Luxemburg"), ("Cyprus", "Zypern"),
    ("Estonia", "Estland"), ("Latvia", "Lettland"), ("Lithuania", "Litauen"), ("Brazil", "Brasilien"),
    ("Argentina", "Argentinien"), ("Colombia", "Kolumbien"), ("Bolivia", "Bolivien"), ("Mexico", "Mexiko"),
    ("United States", "USA"), ("Canada", "Kanada"), ("South Korea", "Südkorea"), ("Korea Republic", "Südkorea"),
    ("Australia", "Australien"), ("Morocco", "Marokko"), ("Egypt", "Ägypten"), ("Cameroon", "Kamerun"),
    ("Ivory Coast", "Elfenbeinküste"), ("Côte d'Ivoire", "Elfenbeinküste"), ("Tunisia", "Tunesien"),
    ("Algeria", "Algerien"), ("South Africa", "Südafrika"), ("Saudi Arabia", "Saudi-Arabien"),
    ("Qatar", "Katar"), ("New Zealand", "Neuseeland"), ("Jamaica", "Jamaika"), ("Cape Verde", "Kap Verde"),
    ("Jordan", "Jordanien"), ("Uzbekistan", "Usbekistan"), ("Ukraine", "Ukraine"), ("Wales", "Wales"),
    ("England", "England"), ("Portugal", "Portugal"),
];

fn country_de(name: &str) -> Option<&'static str> {
    COUNTRIES.iter().find(|(en, _)| en.eq_ignore_ascii_case(name)).map(|(_, de)| *de)
}

/// Fuer den Vergleich von Teamnamen zwischen Quellen: klein, ohne Umlaute und Vereinskuerzel
pub fn norm_name(n: &str) -> String {
    let low = n.to_lowercase().replace('ä', "a").replace('ö', "o").replace('ü', "u").replace('ß', "ss");
    low.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !matches!(*w, "1" | "fc" | "sv" | "vfl" | "vfb" | "tsg" | "sc" | "fsv" | "spvgg" | "tsv" | "bsc" | "e" | "v" | "05" | "04" | "1846"))
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------- ESPN: Spielstaende ----------

fn team_of(c: &Value, sport: &str) -> Team {
    let t = &c["team"];
    let id = s(&t["id"]);
    let display = s(&t["displayName"]);
    let mut team = Team {
        id: format!("{sport}:{id}"),
        name: display.clone(),
        short: { let x = s(&t["shortDisplayName"]); if x.is_empty() { display.clone() } else { x } },
        abbr: s(&t["abbreviation"]),
        logo: s(&t["logo"]),
        color: String::new(),
        score: s(&c["score"]),
    };
    if sport == "soccer" {
        if let Some((_, name, short, abbr)) = CLUBS.iter().find(|x| x.0 == id) {
            team.name = (*name).into();
            team.short = (*short).into();
            team.abbr = (*abbr).into();
        } else if let Some(de) = country_de(&display) {
            team.name = de.into();
            team.short = de.into();
        }
    }
    team
}

fn clock_text(sport: &str, state: &str, st: &Value) -> String {
    let name = s(&st["type"]["name"]);
    let short = s(&st["type"]["shortDetail"]);
    let dc = s(&st["displayClock"]);
    let period = st["period"].as_u64().unwrap_or(0);
    match state {
        "pre" => String::new(), // die Notch zeigt die Anstosszeit
        "post" => {
            let pens = short.contains("Pen") || name.contains("PEN") || name.contains("SHOOTOUT");
            let aet = short.contains("AET") || short.contains("OT") || name.contains("AET") || name.contains("OVERTIME");
            if name.contains("POSTPONED") { "verschoben".into() } else if name.contains("CANCELED") { "abgesagt".into() } else if pens { "Ende i. E.".into() } else if aet { "Ende n. V.".into() } else { "Ende".into() }
        }
        _ => {
            if name.contains("HALFTIME") {
                return "Halbzeit".into();
            }
            match sport {
                "soccer" if name.contains("SHOOTOUT") => "Elfmeterschießen".into(),
                "soccer" if name.contains("END_OF_REGULATION") || name.contains("END_REGULAR") => "vor Verlängerung".into(),
                "soccer" => minute_de(&dc),
                "hockey" if name.contains("END_PERIOD") => format!("Pause nach {period}. Drittel"),
                "hockey" if period > 3 => format!("Verl. {dc}"),
                "hockey" => format!("{period}. Drittel · {dc}"),
                "basketball" | "football" if name.contains("END_PERIOD") => format!("Ende {period}. Viertel"),
                "basketball" | "football" if period > 4 => format!("Verl. {dc}"),
                "basketball" | "football" => format!("{period}. Viertel · {dc}"),
                "baseball" => format!("{period}. Inning {}", if short.starts_with("Bot") { "▼" } else { "▲" }),
                _ => short,
            }
        }
    }
}

/// Tore, Karten, Wechsel aus dem ESPN-Spielplan (competitions[0].details)
fn details(comp: &Value, home: &Team, away: &Team) -> Vec<Ev> {
    let mut out = Vec::new();
    for d in comp["details"].as_array().into_iter().flatten() {
        let typ = s(&d["type"]["text"]);
        let minute = minute_de(&s(&d["clock"]["displayValue"]));
        let team_id = s(&d["team"]["id"]);
        let side = if home.id.ends_with(&format!(":{team_id}")) && !team_id.is_empty() {
            "home"
        } else if away.id.ends_with(&format!(":{team_id}")) && !team_id.is_empty() {
            "away"
        } else {
            ""
        };
        let team = match side { "home" => home.short.as_str(), "away" => away.short.as_str(), _ => "" };
        let who = {
            let a = &d["athletesInvolved"][0];
            let n = s(&a["displayName"]);
            if n.is_empty() { s(&a["shortName"]) } else { n }
        };
        let flag = |k: &str| d[k].as_bool().unwrap_or(false);
        let low = typ.to_lowercase();
        let (kind, title, extra, big) = if flag("scoringPlay") || (low.contains("goal") && !low.contains("disallowed")) {
            let how = if flag("ownGoal") || low.contains("own") {
                " (Eigentor)"
            } else if flag("penaltyKick") || low.contains("penalty") {
                " (Elfmeter)"
            } else if low.contains("header") {
                " (Kopfball)"
            } else {
                ""
            };
            ("goal", if team.is_empty() { "Tor!".to_string() } else { format!("Tor für {team}!") }, how, 3)
        } else if flag("redCard") || low.contains("red card") {
            ("red", "Rote Karte".to_string(), "", 3)
        } else if flag("yellowCard") || low.contains("yellow") {
            ("yellow", "Gelbe Karte".to_string(), "", 1)
        } else if low.contains("substitution") {
            ("sub", format!("Wechsel {team}").trim().to_string(), "", 1)
        } else if low.contains("penalty") && (low.contains("miss") || low.contains("saved") || low.contains("woodwork")) {
            ("miss", "Elfmeter verschossen!".to_string(), "", 2)
        } else {
            ("info", typ.clone(), "", 0)
        };
        let mut text = format!("{who}{extra}");
        if matches!(kind, "red" | "yellow") && !team.is_empty() {
            text = format!("{who} ({team})");
        }
        out.push(Ev {
            id: format!("d|{typ}|{minute}|{who}|{team_id}"),
            minute,
            kind: kind.into(),
            side: side.into(),
            title,
            text: text.trim().to_string(),
            big,
        });
    }
    out
}

pub fn parse_espn(v: &Value, sport: &str, path: &str) -> Vec<Match> {
    let mut out = Vec::new();
    for e in v["events"].as_array().into_iter().flatten() {
        let id = s(&e["id"]);
        let comp = &e["competitions"][0];
        let st = if e["status"].is_object() { &e["status"] } else { &comp["status"] };
        let state = s(&st["type"]["state"]);
        let (mut home, mut away) = (Team::default(), Team::default());
        let mut raw_colors = [("".to_string(), "".to_string()), ("".to_string(), "".to_string())];
        for c in comp["competitors"].as_array().into_iter().flatten() {
            let t = team_of(c, sport);
            let cols = (s(&c["team"]["color"]), s(&c["team"]["alternateColor"]));
            if s(&c["homeAway"]) == "home" {
                home = t;
                raw_colors[0] = cols;
            } else {
                away = t;
                raw_colors[1] = cols;
            }
        }
        let (hc, ac) = pick_colors(
            (&raw_colors[0].0, &raw_colors[0].1),
            (&raw_colors[1].0, &raw_colors[1].1),
        );
        home.color = hc;
        away.color = ac;
        let link = e["links"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|l| l["rel"].as_array().is_some_and(|r| r.iter().any(|x| x == "summary")))
            .map(|l| s(&l["href"]))
            .unwrap_or_default();
        let events = if sport == "soccer" { details(comp, &home, &away) } else { Vec::new() };
        out.push(Match {
            key: format!("{sport}/{path}:{id}"),
            sport: sport.into(),
            clock: clock_text(sport, &state, st),
            start: parse_utc(&s(&e["date"])).unwrap_or(0),
            state: if matches!(state.as_str(), "pre" | "in" | "post") { state } else { "pre".into() },
            home,
            away,
            link,
            pitch: sport == "soccer",
            events,
            source: "espn".into(),
            ..Default::default()
        });
    }
    out
}

// ---------- OpenLigaDB ----------

/// Spielminute ohne Live-Uhr: aus der Anstosszeit geschaetzt (15 Min Halbzeitpause)
pub fn oldb_clock(start: u64, now: u64) -> String {
    let m = now.saturating_sub(start) / 60_000;
    match m {
        0..=45 => format!("{}'", m.max(1)),
        46..=47 => "45+'".into(),
        48..=62 => "Halbzeit".into(),
        _ if m - 15 <= 90 => format!("{}'", m - 15),
        _ => "90+'".into(),
    }
}

fn oldb_abbr(short: &str) -> String {
    let n = norm_name(short);
    for (k, a) in [("bayern", "FCB"), ("dortmund", "BVB"), ("leverkusen", "B04"), ("leipzig", "RBL"), ("frankfurt", "SGE"),
                   ("gladbach", "BMG"), ("hamburg", "HSV"), ("koln", "KOE"), ("pauli", "STP"), ("schalke", "S04")] {
        if n.contains(k) {
            return a.into();
        }
    }
    short.chars().filter(|c| c.is_alphanumeric()).take(3).collect::<String>().to_uppercase()
}

pub fn parse_oldb(v: &Value, shortcut: &str, now: u64) -> Vec<Match> {
    let mut out = Vec::new();
    for m in v.as_array().into_iter().flatten() {
        let id = s(&m["matchID"]);
        let start = parse_utc(&s(&m["matchDateTimeUTC"])).unwrap_or(0);
        let finished = m["matchIsFinished"].as_bool().unwrap_or(false);
        let team = |t: &Value| {
            let short = { let x = s(&t["shortName"]); if x.is_empty() { s(&t["teamName"]) } else { x } };
            Team {
                id: format!("oldb:{}", s(&t["teamId"])),
                name: s(&t["teamName"]),
                abbr: oldb_abbr(&short),
                short,
                logo: s(&t["teamIconUrl"]),
                color: String::new(),
                score: String::new(),
            }
        };
        let (mut home, mut away) = (team(&m["team1"]), team(&m["team2"]));
        home.color = "#4da3ff".into();
        away.color = "#ff6b6b".into();
        let mut events = Vec::new();
        let (mut h, mut a) = (0i64, 0i64);
        for g in m["goals"].as_array().into_iter().flatten() {
            let (gh, ga) = (g["scoreTeam1"].as_i64().unwrap_or(h), g["scoreTeam2"].as_i64().unwrap_or(a));
            let side = if gh > h { "home" } else if ga > a { "away" } else { "" };
            h = gh;
            a = ga;
            let tshort = if side == "home" { home.short.clone() } else { away.short.clone() };
            let how = if g["isOwnGoal"].as_bool() == Some(true) {
                " (Eigentor)"
            } else if g["isPenalty"].as_bool() == Some(true) {
                " (Elfmeter)"
            } else {
                ""
            };
            let minute = g["matchMinute"].as_i64().map(|x| format!("{x}'")).unwrap_or_default();
            events.push(Ev {
                id: format!("g|{}", s(&g["goalID"])),
                minute,
                kind: "goal".into(),
                side: side.into(),
                title: if side.is_empty() { "Tor!".into() } else { format!("Tor für {tshort}!") },
                text: format!("{}{how}", s(&g["goalGetterName"])).trim().to_string(),
                big: 3,
            });
        }
        if events.is_empty() && finished {
            // Endstand ohne Torschuetzen
            if let Some(r) = m["matchResults"].as_array().and_then(|r| r.iter().max_by_key(|x| x["resultOrderID"].as_i64().unwrap_or(0))) {
                h = r["pointsTeam1"].as_i64().unwrap_or(0);
                a = r["pointsTeam2"].as_i64().unwrap_or(0);
            }
        }
        let state = if finished {
            "post"
        } else if now >= start && now < start + 3 * 3_600_000 {
            "in"
        } else if now >= start {
            "post"
        } else {
            "pre"
        };
        if state != "pre" {
            home.score = h.to_string();
            away.score = a.to_string();
        }
        out.push(Match {
            key: format!("oldb/{shortcut}:{id}"),
            sport: "soccer".into(),
            clock: match state { "in" => oldb_clock(start, now), "post" => "Ende".into(), _ => String::new() },
            state: state.into(),
            start,
            home,
            away,
            events,
            source: "openligadb".into(),
            ..Default::default()
        });
    }
    out
}

// ---------- Einstellungen ----------

#[derive(Clone, Debug)]
struct Cfg {
    on: bool,
    leagues: Vec<String>,
    /// (Schluessel, Name)
    favs: Vec<(String, String)>,
    only_fav: bool,
}

fn cfg() -> Cfg {
    let v = crate::settings_value("/sport").unwrap_or(Value::Null);
    let leagues = v["leagues"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).filter(|x| league(x).is_some()).collect())
        .unwrap_or_else(|| vec!["bl1".into(), "dfbteam".into()]);
    let favs = v["teams"]
        .as_array()
        .map(|a| a.iter().map(|t| (s(&t["key"]), s(&t["name"]))).filter(|t| !t.0.is_empty()).collect())
        .unwrap_or_default();
    Cfg {
        on: v["on"].as_bool().unwrap_or(true),
        leagues,
        favs,
        only_fav: v["scope"].as_str() == Some("fav"),
    }
}

fn is_fav(t: &Team, favs: &[(String, String)]) -> bool {
    let n = norm_name(&t.name);
    favs.iter().any(|(k, name)| *k == t.id || (!n.is_empty() && norm_name(name) == n))
}

// ---------- Abruf ----------

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .user_agent(concat!("Notch/", env!("CARGO_PKG_VERSION"), " (Windows)"))
        .build()
        .into()
}

fn get_json(agent: &ureq::Agent, url: &str) -> Result<Value, String> {
    let mut r = agent.get(url).call().map_err(|e| e.to_string())?;
    r.body_mut().with_config().limit(8 * 1024 * 1024).read_json::<Value>().map_err(|e| e.to_string())
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Src {
    Espn(&'static str, &'static str),
    Oldb(&'static str),
}

impl Src {
    fn url(&self) -> String {
        match self {
            Src::Espn(sport, l) => format!("{ESPN}/{sport}/{l}/scoreboard"),
            Src::Oldb(sc) => format!("{OLDB}/getmatchdata/{sc}"),
        }
    }
}

struct Feed {
    due: Instant,
    fails: u32,
    matches: Vec<Match>,
}

/// Wie oft eine Quelle neu gefragt wird: live alle 15 s, kurz vor Anstoss alle 30 s, sonst selten
fn next_poll(ms: &[Match], now: u64) -> Duration {
    if ms.iter().any(|m| m.state == "in") {
        Duration::from_secs(15)
    } else if ms.iter().any(|m| m.state == "pre" && m.start <= now + 20 * 60_000) {
        Duration::from_secs(30)
    } else if ms.iter().any(|m| m.state == "pre" && m.start <= now + 3 * 3_600_000) {
        Duration::from_secs(300)
    } else {
        Duration::from_secs(20 * 60)
    }
}

/// Spiel ist fuer die Notch interessant: laeuft, beginnt in den naechsten 18 h oder endete vor kurzem
fn in_window(m: &Match, now: u64) -> bool {
    match m.state.as_str() {
        "in" => true,
        "pre" => m.start <= now + 18 * 3_600_000 && m.start + 4 * 3_600_000 >= now,
        _ => m.start + 5 * 3_600_000 >= now,
    }
}

/// Welche Quellen gebraucht werden; ESPN-Ligen mit OpenLigaDB-Ersatz wechseln nach 3 Fehlern
fn wanted_sources(c: &Cfg, feeds: &HashMap<Src, Feed>) -> Vec<(Src, &'static League)> {
    let mut out = Vec::new();
    for id in &c.leagues {
        let Some(l) = league(id) else { continue };
        let espn: Vec<Src> = l.espn.iter().map(|p| Src::Espn(l.sport, p)).collect();
        let broken = !espn.is_empty() && espn.iter().all(|s| feeds.get(s).is_some_and(|f| f.fails >= 3));
        match (l.oldb, espn.is_empty() || broken) {
            (Some(sc), true) => out.push((Src::Oldb(sc), l)),
            _ => out.extend(espn.into_iter().map(|s| (s, l))),
        }
        // waehrend ESPN ausfaellt, weiter probieren (wird wieder genommen, sobald es antwortet)
        if broken {
            out.extend(l.espn.iter().map(|p| (Src::Espn(l.sport, p), l)));
        }
    }
    out
}

// ---------- Ticker: Unterschiede zwischen zwei Abrufen ----------

#[derive(Default)]
struct Seen {
    evs: HashSet<String>,
    score: (String, String),
    state: String,
    half: bool,
    ticker: Vec<Ev>,
}

fn note(id: &str, kind: &str, title: String, text: String, minute: &str, big: u8) -> Ev {
    Ev { id: id.into(), minute: minute.into(), kind: kind.into(), side: String::new(), title, text, big }
}

/// Neue Meldungen fuer ein Spiel seit dem letzten Abruf. Beim ersten Mal nur merken (keine Flut beim Start).
fn diff(seen: &mut Seen, m: &Match, first: bool) -> Vec<Ev> {
    let score = (m.home.score.clone(), m.away.score.clone());
    let half = m.clock == "Halbzeit";
    let mut out = Vec::new();
    if first {
        seen.evs = m.events.iter().map(|e| e.id.clone()).collect();
        seen.ticker = m.events.clone();
    } else {
        let sc = format!("{}:{}", m.home.score, m.away.score);
        if seen.state == "pre" && m.state == "in" {
            out.push(note(&format!("ko|{}", m.key), "kickoff", "Anpfiff".into(), format!("{} – {}", m.home.short, m.away.short), "", 2));
        }
        if half && !seen.half {
            out.push(note(&format!("ht|{}", m.key), "half", "Halbzeit".into(), format!("{} {sc} {}", m.home.short, m.away.short), "45'", 2));
        }
        let mut goal = false;
        for e in &m.events {
            if seen.evs.insert(e.id.clone()) {
                goal |= e.kind == "goal";
                out.push(e.clone());
            }
        }
        // Spielstand geaendert, aber kein Tor gemeldet (US-Sport, fehlende Details): allgemein melden
        let changed = score != seen.score && !seen.score.0.is_empty() && m.state == "in";
        if changed && !goal && m.sport != "basketball" {
            let h_up = m.home.score.parse::<i64>().unwrap_or(0) > seen.score.0.parse::<i64>().unwrap_or(0);
            let team = if h_up { &m.home.short } else { &m.away.short };
            let what = match m.sport.as_str() { "soccer" | "hockey" => "Tor", "football" => "Punkte", _ => "Punkt" };
            let mut e = note(&format!("sc|{}|{sc}", m.key), "score", format!("{what} für {team}!"), format!("{} {sc} {}", m.home.short, m.away.short), &m.clock, 3);
            e.side = if h_up { "home" } else { "away" }.into();
            out.push(e);
        }
        if seen.state == "in" && m.state == "post" {
            out.push(note(&format!("ft|{}", m.key), "end", m.clock.clone(), format!("{} {sc} {}", m.home.short, m.away.short), "", 2));
        }
        seen.ticker.extend(out.iter().cloned());
        let n = seen.ticker.len();
        if n > 40 {
            seen.ticker.drain(..n - 40);
        }
    }
    seen.score = score;
    seen.state = m.state.clone();
    seen.half = half;
    out
}

// ---------- Ballverlauf (ESPN Core) ----------

/// Spielfeld-Koordinaten so drehen, dass Heim immer nach rechts spielt
fn flip(side: &str, x: f32, y: f32) -> (f32, f32) {
    if side == "away" { (100.0 - x, 100.0 - y) } else { (x, y) }
}

fn team_id_of_ref(r: &str) -> String {
    r.split("/teams/").nth(1).map(|x| x.split(['?', '/']).next().unwrap_or("").to_string()).unwrap_or_default()
}

pub fn play_of(it: &Value, home_id: &str) -> Option<Play> {
    let kind = s(&it["type"]["type"]);
    if kind.is_empty() {
        return None;
    }
    let tid = team_id_of_ref(&s(&it["team"]["$ref"]));
    let side = if tid.is_empty() { "" } else if tid == home_id { "home" } else { "away" };
    let f = |k: &str| it[k].as_f64().map(|v| v as f32);
    let (x, y) = match (f("fieldPositionX"), f("fieldPositionY")) {
        (Some(x), Some(y)) => flip(side, x, y),
        _ => (-1.0, -1.0),
    };
    let (x2, y2) = match (f("fieldPosition2X"), f("fieldPosition2Y")) {
        (Some(a), Some(b)) => {
            let (a, b) = flip(side, a, b);
            (Some(a), Some(b))
        }
        _ => (None, None),
    };
    let text = s(&it["text"]);
    Some(Play {
        id: s(&it["id"]),
        t: parse_utc(&s(&it["wallclock"])).unwrap_or(0),
        minute: minute_de(&s(&it["clock"]["displayValue"])),
        kind,
        side: side.into(),
        jersey: s(&it["participants"][0]["jersey"]),
        who: name_from_text(&text),
        x,
        y,
        x2,
        y2,
    })
}

/// Ballaktion -> Tickerzeile (nur Bemerkenswertes; Tore und Karten kommen aus dem Spielplan)
pub fn play_ev(p: &Play, text: &str, m: &Match) -> Option<Ev> {
    let team = match p.side.as_str() { "home" => m.home.short.as_str(), "away" => m.away.short.as_str(), _ => "" };
    let who = if p.who.is_empty() { team.to_string() } else { p.who.clone() };
    let k = p.kind.as_str();
    let (kind, title, body, big) = if k == "substitution" {
        // "Substitution, Team. A replaces B."
        let tail = text.rsplit(". ").next().unwrap_or("").trim_end_matches('.');
        let body = match tail.split_once(" replaces ") { Some((a, b)) => format!("{a} für {b}"), None => tail.to_string() };
        ("sub", format!("Wechsel {team}").trim().to_string(), body, 1)
    } else if k.starts_with("penalty") && !k.contains("scored") {
        ("miss", "Elfmeter verschossen!".to_string(), who, 2)
    } else if k.starts_with("var") {
        let low = text.to_lowercase();
        let body = if low.contains("penalty") { format!("Elfmeter {team}").trim().to_string() } else if low.contains("goal") { "Tor wird geprüft".into() } else { "Entscheidung wird geprüft".into() };
        ("var", "Videobeweis".to_string(), body, 2)
    } else if k.contains("woodwork") || k.contains("post") {
        ("wood", "Aluminium!".to_string(), who, 1)
    } else if k == "shot-on-target" {
        ("chance", "Chance".to_string(), format!("{who} – gehalten"), 0)
    } else if k == "shot-off-target" {
        ("chance", "Chance".to_string(), format!("{who} – vorbei"), 0)
    } else if k == "shot-blocked" {
        ("chance", "Chance".to_string(), format!("{who} – geblockt"), 0)
    } else if k == "corner-awarded" {
        ("corner", format!("Ecke {team}").trim().to_string(), String::new(), 0)
    } else if k == "offside" {
        ("offside", "Abseits".to_string(), who, 0)
    } else {
        return None;
    };
    Some(Ev { id: format!("p|{}", p.id), minute: p.minute.clone(), kind: kind.into(), side: p.side.clone(), title, text: body, big })
}

#[derive(Default)]
struct Watch {
    key: String,
    url: String,
    home_id: String,
    /// so viele Aktionen sind verarbeitet
    next: usize,
    started: bool,
    due: Option<Instant>,
    ids: HashSet<String>,
}

/// Neue Ballaktionen holen (beim ersten Mal die letzten 40). Danach kleine Seiten: ESPN liefert immer die
/// ganze Seite, und jede Aktion ist ~1,5 KB — so bleibt es bei etwa 10–15 MB pro Stunde Zuschauen.
fn fetch_plays(agent: &ureq::Agent, w: &mut Watch) -> Result<Vec<(Play, String)>, String> {
    let size = if w.started { 10 } else { 40 };
    if !w.started {
        let v = get_json(agent, &format!("{}?limit=1&page=1", w.url))?;
        let count = v["count"].as_u64().unwrap_or(0) as usize;
        w.next = count.saturating_sub(40);
        w.started = true;
    }
    let mut out = Vec::new();
    for _ in 0..5 {
        let page = w.next / size + 1;
        let v = get_json(agent, &format!("{}?limit={size}&page={page}", w.url))?;
        let items = v["items"].as_array().cloned().unwrap_or_default();
        let count = v["count"].as_u64().unwrap_or(0) as usize;
        for (i, it) in items.iter().enumerate() {
            if (page - 1) * size + i < w.next {
                continue;
            }
            if let Some(p) = play_of(it, &w.home_id) {
                if w.ids.insert(p.id.clone()) {
                    out.push((p, s(&it["text"])));
                }
            }
        }
        w.next = w.next.max((page - 1) * size + items.len());
        if items.len() < size || w.next >= count {
            break;
        }
    }
    if w.ids.len() > 3000 {
        w.ids.clear();
    }
    Ok(out)
}

// ---------- Zustand fuer Befehle ----------

static LAST: LazyLock<Mutex<Value>> = LazyLock::new(|| Mutex::new(serde_json::json!({ "matches": [] })));
/// Spiel, das die Notch gerade aufgeklappt zeigt (+ wann zuletzt bestaetigt)
static WATCH: Mutex<Option<(String, Instant)>> = Mutex::new(None);

pub fn last() -> Value {
    LAST.lock().unwrap().clone()
}

/// Die Notch zeigt dieses Spiel gerade aufgeklappt -> Ballverlauf holen (alle ~10 s erneuern)
#[tauri::command]
pub fn sport_watch(key: Option<String>) {
    *WATCH.lock().unwrap() = key.filter(|k| !k.is_empty()).map(|k| (k, Instant::now()));
}

#[derive(Serialize)]
pub struct LeagueInfo {
    id: &'static str,
    name: &'static str,
    group: &'static str,
    sport: &'static str,
    source: &'static str,
}

#[tauri::command]
pub fn sport_leagues() -> Vec<LeagueInfo> {
    LEAGUES
        .iter()
        .map(|l| LeagueInfo {
            id: l.id,
            name: l.name,
            group: l.group,
            sport: l.sport,
            source: if l.espn.is_empty() { "OpenLigaDB" } else { "ESPN" },
        })
        .collect()
}

#[derive(Serialize, Clone)]
pub struct TeamInfo {
    key: String,
    name: String,
    logo: String,
}

/// Mannschaften eines Wettbewerbs (fuer die Auswahl der Lieblingsteams)
#[tauri::command]
pub async fn sport_teams(league: String) -> Result<Vec<TeamInfo>, String> {
    let l = self::league(&league).ok_or("unbekannter Wettbewerb")?;
    tauri::async_runtime::spawn_blocking(move || {
        let agent = agent();
        let mut out: Vec<TeamInfo> = Vec::new();
        if let Some(path) = l.espn.iter().find(|p| **p != "fifa.friendly").or(l.espn.first()) {
            // Nationalmannschaften: Teilnehmer der WM bzw. des Wettbewerbs
            let p = if l.id == "dfbteam" { "fifa.world" } else { path };
            let v = get_json(&agent, &format!("{ESPN}/{}/{p}/teams", l.sport))?;
            for t in v.pointer("/sports/0/leagues/0/teams").and_then(|x| x.as_array()).into_iter().flatten() {
                let c = serde_json::json!({ "team": t["team"], "score": "" });
                let team = team_of(&c, l.sport);
                let logo = t["team"]["logos"][0]["href"].as_str().unwrap_or("").to_string();
                out.push(TeamInfo { key: team.id, name: team.name, logo });
            }
            if l.id == "dfbteam" && !out.iter().any(|t| t.name == "Deutschland") {
                out.push(TeamInfo { key: "soccer:481".into(), name: "Deutschland".into(), logo: String::new() });
            }
        } else if let Some(sc) = l.oldb {
            let now = crate::activities::now_ms();
            // Saison beginnt im Sommer
            let year = 1970 + now / 31_556_952_000;
            let month = ((now % 31_556_952_000) / 2_629_746_000) + 1;
            let season = if month >= 7 { year } else { year - 1 };
            let v = get_json(&agent, &format!("{OLDB}/getavailableteams/{sc}/{season}"))?;
            for t in v.as_array().into_iter().flatten() {
                out.push(TeamInfo { key: format!("oldb:{}", s(&t["teamId"])), name: s(&t["teamName"]), logo: s(&t["teamIconUrl"]) });
            }
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        out.dedup_by(|a, b| a.key == b.key);
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------- Schleife ----------

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let agent = agent();
        let mut feeds: HashMap<Src, Feed> = HashMap::new();
        let mut seen: HashMap<String, Seen> = HashMap::new();
        let mut sent = String::new();
        let mut watch = Watch::default();
        let mut error = String::new();
        loop {
            let c = cfg();
            if !c.on {
                if !feeds.is_empty() || sent.is_empty() {
                    feeds.clear();
                    seen.clear();
                    let v = serde_json::json!({ "matches": [], "error": "", "updated": crate::activities::now_ms(), "off": true });
                    *LAST.lock().unwrap() = v.clone();
                    let _ = app.emit("sport", v);
                    sent = "off".into();
                }
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }
            let now = Instant::now();
            let now_ms = crate::activities::now_ms();
            let want = wanted_sources(&c, &feeds);
            let srcs: HashSet<Src> = want.iter().map(|(s, _)| s.clone()).collect();
            feeds.retain(|s, _| srcs.contains(s));

            // faellige Quellen abfragen
            for src in &srcs {
                let f = feeds.entry(src.clone()).or_insert(Feed { due: now, fails: 0, matches: Vec::new() });
                if f.due > now {
                    continue;
                }
                match get_json(&agent, &src.url()) {
                    Ok(v) => {
                        f.matches = match src {
                            Src::Espn(sport, l) => parse_espn(&v, sport, l),
                            Src::Oldb(sc) => parse_oldb(&v, sc, now_ms),
                        };
                        f.fails = 0;
                        error.clear();
                        // nur die Spiele zaehlen, die eine gewaehlte Liga wirklich zeigt
                        let relevant: Vec<Match> = f
                            .matches
                            .iter()
                            .filter(|m| want.iter().any(|(w, l)| w == src && l.team.is_none_or(|t| m.home.name == t || m.away.name == t)))
                            .cloned()
                            .collect();
                        f.due = now + next_poll(&relevant, now_ms);
                    }
                    Err(e) => {
                        f.fails += 1;
                        f.due = now + Duration::from_secs((30u64 << f.fails.min(5)).min(600));
                        error = format!("Sportdaten nicht erreichbar: {e}");
                        eprintln!("[notch] Sport {}: {e}", src.url());
                    }
                }
            }

            // alle Spiele im Blick zusammenstellen (Liga, Filter, Lieblingsteams)
            let mut list: Vec<Match> = Vec::new();
            let mut keys = HashSet::new();
            for (src, l) in &want {
                let Some(f) = feeds.get(src) else { continue };
                for m in &f.matches {
                    if keys.contains(&m.key) || !in_window(m, now_ms) {
                        continue;
                    }
                    if let Some(t) = l.team {
                        if m.home.name != t && m.away.name != t {
                            continue;
                        }
                    }
                    let mut m = m.clone();
                    m.league = l.id.into();
                    m.league_name = l.name.into();
                    m.fav = is_fav(&m.home, &c.favs) || is_fav(&m.away, &c.favs);
                    if c.only_fav && !c.favs.is_empty() && !m.fav {
                        continue;
                    }
                    keys.insert(m.key.clone());
                    list.push(m);
                }
            }
            // gleiche Partie aus zwei Quellen (ESPN + Ersatz): nur einmal
            let mut pairs = HashSet::new();
            list.retain(|m| pairs.insert((norm_name(&m.home.name), norm_name(&m.away.name), m.start / 3_600_000)));
            list.sort_by(|a, b| {
                let rank = |m: &Match| match m.state.as_str() { "in" => 0, "pre" => 1, _ => 2 };
                (rank(a), !a.fav, a.start).cmp(&(rank(b), !b.fav, b.start))
            });

            // Ballverlauf fuer das angesehene Spiel
            let wkey = WATCH
                .lock()
                .unwrap()
                .clone()
                .filter(|(_, at)| at.elapsed() < Duration::from_secs(25))
                .map(|(k, _)| k);
            let mut plays_out: Option<(bool, Vec<Play>)> = None;
            let mut play_news: Vec<(String, Ev)> = Vec::new();
            match wkey.as_ref().and_then(|k| list.iter().find(|m| &m.key == k && m.pitch && m.source == "espn" && m.state != "pre")) {
                Some(m) => {
                    if watch.key != m.key {
                        // neues Spiel: Adresse der Ballaktionen aus dem Schluessel "soccer/ger.1:123"
                        let (path, id) = m.key.split_once(':').unwrap_or(("", ""));
                        let (sport, lg) = path.split_once('/').unwrap_or(("soccer", ""));
                        watch = Watch {
                            key: m.key.clone(),
                            url: format!("{CORE}/{sport}/leagues/{lg}/events/{id}/competitions/{id}/plays"),
                            home_id: m.home.id.rsplit(':').next().unwrap_or("").to_string(),
                            ..Default::default()
                        };
                    }
                    let live = m.state == "in";
                    if watch.due.is_none_or(|d| d <= now) && (live || !watch.started) {
                        let reset = !watch.started;
                        match fetch_plays(&agent, &mut watch) {
                            Ok(ps) => {
                                for (p, text) in &ps {
                                    if let Some(e) = play_ev(p, text, m) {
                                        play_news.push((m.key.clone(), e));
                                    }
                                }
                                let plays: Vec<Play> = ps.into_iter().map(|(p, _)| p).collect();
                                if reset || !plays.is_empty() {
                                    plays_out = Some((reset, plays));
                                }
                            }
                            Err(e) => eprintln!("[notch] Ballverlauf: {e}"),
                        }
                        watch.due = Some(now + Duration::from_secs(if live { 6 } else { 60 }));
                    }
                }
                None => {
                    if !watch.key.is_empty() {
                        watch = Watch::default();
                    }
                }
            }

            // Ticker: Unterschiede seit dem letzten Abruf
            let mut news: Vec<News> = Vec::new();
            for m in &list {
                let first = !seen.contains_key(&m.key);
                let st = seen.entry(m.key.clone()).or_default();
                for e in diff(st, m, first) {
                    news.push(News { key: m.key.clone(), score: format!("{}:{}", m.home.score, m.away.score), ev: e, at: now_ms });
                }
            }
            for (k, e) in play_news {
                if let (Some(st), Some(m)) = (seen.get_mut(&k), list.iter().find(|m| m.key == k)) {
                    // erste Ladung beim Ansehen: nur in den Ticker, nicht als neue Meldung
                    let fresh = plays_out.as_ref().is_none_or(|(reset, _)| !*reset);
                    if st.evs.insert(e.id.clone()) {
                        st.ticker.push(e.clone());
                        if fresh {
                            news.push(News { key: k.clone(), score: format!("{}:{}", m.home.score, m.away.score), ev: e, at: now_ms });
                        }
                    }
                    st.ticker.sort_by_key(ev_order);
                }
            }
            seen.retain(|k, _| keys.contains(k));
            for m in list.iter_mut() {
                if let Some(st) = seen.get(&m.key) {
                    let n = st.ticker.len();
                    m.events = st.ticker[n.saturating_sub(14)..].to_vec();
                }
            }

            let payload = serde_json::json!({ "matches": list, "error": error });
            let sig = payload.to_string();
            if sig != sent {
                sent = sig;
                let mut v = payload;
                v["updated"] = now_ms.into();
                *LAST.lock().unwrap() = v.clone();
                let _ = app.emit("sport", v);
            }
            if let Some((reset, plays)) = plays_out {
                let _ = app.emit("sport-plays", serde_json::json!({ "key": watch.key, "reset": reset, "plays": plays }));
            }
            for n in news {
                let _ = app.emit("sport-news", n);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Reihenfolge im Ticker: Anpfiff zuerst, Abpfiff zuletzt, sonst nach Minute ("45+2'" -> 4502)
fn ev_order(e: &Ev) -> u32 {
    match e.kind.as_str() {
        "kickoff" => 0,
        "end" => u32::MAX,
        _ => minute_order(&e.minute).min(u32::MAX - 1),
    }
}

/// "45+2'" -> 4502, "67'" -> 6700; ohne Minute ans Ende
fn minute_order(m: &str) -> u32 {
    let m = m.trim_end_matches('\'');
    let (a, b) = m.split_once('+').unwrap_or((m, "0"));
    match a.parse::<u32>() {
        Ok(a) => a * 100 + b.parse::<u32>().unwrap_or(0),
        Err(_) => u32::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_zeiten() {
        assert_eq!(parse_utc("1970-01-01T00:00Z"), Some(0));
        assert_eq!(parse_utc("2026-10-09T18:30Z"), Some(1_791_570_600_000));
        assert_eq!(parse_utc("2026-09-19T16:30:00Z"), Some(1_789_835_400_000));
        assert_eq!(parse_utc("2026-09-19T13:55:02.123Z"), Some(1_789_826_102_000));
        assert_eq!(parse_utc("Quatsch"), None);
    }

    #[test]
    fn minuten_und_namen() {
        assert_eq!(minute_de("45'+2'"), "45+2'");
        assert_eq!(minute_de("67'"), "67'");
        assert_eq!(minute_order("45+2'"), 4502);
        assert!(minute_order("46'") > minute_order("45+2'"));
        assert_eq!(name_from_text("Moritz Nicolas (Borussia Mönchengladbach) Pass at 19'"), "Moritz Nicolas");
        assert_eq!(name_from_text("Attempt missed. Anthony Caci (Mainz) right footed shot"), "Anthony Caci");
        assert_eq!(name_from_text("Goal! Borussia M'gladbach 1, Mainz 0. Florian Neuhaus (Borussia M'gladbach) right"), "Florian Neuhaus");
        assert_eq!(name_from_text("Foul by Lukas Ullrich (Borussia M'gladbach)."), "Lukas Ullrich");
        assert_eq!(name_from_text("Delay in match"), "");
    }

    #[test]
    fn farben_sichtbar_und_verschieden() {
        let (h, a) = pick_colors(("ffffff", "000000"), ("ffffff", "dc052d"));
        assert_eq!(h, "#ffffff");
        assert_eq!(a, "#dc052d", "gleiche Farbe -> Ausweichfarbe der Gaeste");
        let (h, _) = pick_colors(("000000", "272726"), ("03915c", "ffffff"));
        assert_eq!(h, "#4da3ff", "schwarz ist auf der Notch unsichtbar");
    }

    #[test]
    fn openligadb_minute() {
        let k = 1_000_000u64;
        assert_eq!(oldb_clock(k, k + 10 * 60_000), "10'");
        assert_eq!(oldb_clock(k, k + 55 * 60_000), "Halbzeit");
        assert_eq!(oldb_clock(k, k + 80 * 60_000), "65'");
        assert_eq!(oldb_clock(k, k + 120 * 60_000), "90+'");
    }

    #[test]
    fn namen_vergleichen() {
        assert_eq!(norm_name("FC Bayern München"), norm_name("Bayern München"));
        assert_eq!(norm_name("1. FSV Mainz 05"), "mainz");
        assert_ne!(norm_name("Borussia Dortmund"), norm_name("Borussia Mönchengladbach"));
    }

    fn espn_sample(state: &str, home: u32, away: u32, details: Value) -> Value {
        serde_json::json!({ "events": [{
            "id": "77", "date": "2026-10-09T18:30Z",
            "status": { "displayClock": "67'", "period": 2, "type": { "state": state, "name": if state == "in" { "STATUS_SECOND_HALF" } else { "STATUS_SCHEDULED" }, "shortDetail": "67'" } },
            "links": [{ "rel": ["summary", "desktop"], "href": "https://www.espn.com/soccer/match/_/gameId/77" }],
            "competitions": [{
                "competitors": [
                    { "homeAway": "home", "score": home.to_string(), "team": { "id": "124", "displayName": "Borussia Dortmund", "shortDisplayName": "Dortmund", "abbreviation": "DOR", "color": "ffee00", "alternateColor": "272726", "logo": "x" } },
                    { "homeAway": "away", "score": away.to_string(), "team": { "id": "137", "displayName": "Werder Bremen", "shortDisplayName": "Bremen", "abbreviation": "SVW", "color": "03915c", "alternateColor": "ffffff", "logo": "y" } }
                ],
                "details": details
            }]
        }]})
    }

    #[test]
    fn espn_spielstand_und_tore() {
        let goal = serde_json::json!([{ "type": { "text": "Goal - Header" }, "clock": { "displayValue": "45'+1'" }, "team": { "id": "137" },
            "scoringPlay": true, "athletesInvolved": [{ "displayName": "Marco Grüll" }] }]);
        let ms = parse_espn(&espn_sample("in", 1, 1, goal), "soccer", "ger.1");
        assert_eq!(ms.len(), 1);
        let m = &ms[0];
        assert_eq!(m.key, "soccer/ger.1:77");
        assert_eq!(m.home.abbr, "BVB", "deutsches Kuerzel");
        assert_eq!(m.clock, "67'");
        assert_eq!(m.events[0].title, "Tor für Bremen!");
        assert_eq!(m.events[0].text, "Marco Grüll (Kopfball)");
        assert_eq!(m.events[0].minute, "45+1'");
        assert_eq!(m.events[0].side, "away");
    }

    #[test]
    fn ticker_meldet_nur_neues() {
        let mut st = Seen::default();
        let m0 = &parse_espn(&espn_sample("pre", 0, 0, serde_json::json!([])), "soccer", "ger.1")[0];
        assert!(diff(&mut st, m0, true).is_empty(), "erster Abruf: nur merken");
        let m1 = &parse_espn(&espn_sample("in", 0, 0, serde_json::json!([])), "soccer", "ger.1")[0];
        let n = diff(&mut st, m1, false);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].kind, "kickoff");
        let goal = serde_json::json!([{ "type": { "text": "Goal" }, "clock": { "displayValue": "12'" }, "team": { "id": "124" },
            "scoringPlay": true, "athletesInvolved": [{ "displayName": "Maximilian Beier" }] }]);
        let m2 = &parse_espn(&espn_sample("in", 1, 0, goal.clone()), "soccer", "ger.1")[0];
        let n = diff(&mut st, m2, false);
        assert_eq!(n.len(), 1, "Tor, aber keine zweite allgemeine Spielstand-Meldung");
        assert_eq!(n[0].title, "Tor für Dortmund!");
        assert_eq!(n[0].big, 3);
        assert!(diff(&mut st, m2, false).is_empty(), "nichts doppelt");
        // Spielstand aendert sich ohne Details (z. B. Daten hinken): allgemeine Meldung
        let m3 = &parse_espn(&espn_sample("in", 1, 1, goal), "soccer", "ger.1")[0];
        let n = diff(&mut st, m3, false);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].kind, "score");
        assert_eq!(n[0].title, "Tor für Bremen!");
    }

    #[test]
    fn ballaktion_gedreht_und_benannt() {
        let it = serde_json::json!({ "id": "1", "type": { "type": "pass" }, "text": "Jens Stage (Werder Bremen) Pass at 19'",
            "team": { "$ref": "http://sports.core.api.espn.com/v2/sports/soccer/leagues/ger.1/seasons/2026/teams/137?lang=en" },
            "clock": { "displayValue": "19'" }, "wallclock": "2026-09-19T13:48:18Z", "participants": [{ "jersey": "6" }],
            "fieldPositionX": 20.0, "fieldPositionY": 30.0, "fieldPosition2X": 60.0, "fieldPosition2Y": 50.0 });
        let p = play_of(&it, "124").unwrap();
        assert_eq!(p.side, "away");
        assert_eq!((p.x, p.y), (80.0, 70.0), "Gaeste gespiegelt: Heim spielt nach rechts");
        assert_eq!((p.x2, p.y2), (Some(40.0), Some(50.0)));
        assert_eq!(p.who, "Jens Stage");
        assert_eq!(p.jersey, "6");
    }

    /// Gegen die echten Quellen (Netz noetig): cargo test --lib live_quellen -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_quellen() {
        let a = agent();
        for (sport, l) in [("soccer", "ger.1"), ("soccer", "uefa.nations"), ("hockey", "nhl"), ("football", "nfl")] {
            let v = get_json(&a, &Src::Espn(sport, l).url()).unwrap();
            for m in parse_espn(&v, sport, l).iter().take(4) {
                println!("{l:14} {:5} {:>3}:{:<3} {} – {} | {} | {} Ereignisse", m.state, m.home.score, m.away.score, m.home.name, m.away.name, m.clock, m.events.len());
            }
        }
        let v = get_json(&a, &Src::Oldb("bl3").url()).unwrap();
        for m in parse_oldb(&v, "bl3", crate::activities::now_ms()).iter().take(3) {
            println!("bl3 {:5} {} – {} {}:{}", m.state, m.home.name, m.away.name, m.home.score, m.away.score);
        }
        // Ballaktionen eines beendeten Bundesligaspiels
        let v = get_json(&a, &format!("{ESPN}/soccer/ger.1/scoreboard?dates=20260919")).unwrap();
        let m = &parse_espn(&v, "soccer", "ger.1")[0];
        let (path, id) = m.key.split_once(':').unwrap();
        let mut w = Watch {
            url: format!("{CORE}/soccer/leagues/{}/events/{id}/competitions/{id}/plays", path.split('/').nth(1).unwrap()),
            home_id: m.home.id.rsplit(':').next().unwrap().into(),
            ..Default::default()
        };
        let ps = fetch_plays(&a, &mut w).unwrap();
        println!("{} – {}: {} Ballaktionen (die letzten 40), weiter ab {}", m.home.name, m.away.name, ps.len(), w.next);
        for (p, t) in ps.iter().rev().take(8) {
            let ev = play_ev(p, t, m).map(|e| format!("{} {}", e.title, e.text)).unwrap_or_default();
            println!("  {:6} {:16} {:5} #{:3} {:22} ({:.0},{:.0}) {}", p.minute, p.kind, p.side, p.jersey, p.who, p.x, p.y, ev);
        }
        assert!(!ps.is_empty());
    }

    #[test]
    fn openligadb_spiel() {
        let v = serde_json::json!([{ "matchID": 5, "matchDateTimeUTC": "2026-09-19T16:30:00Z", "matchIsFinished": false,
            "team1": { "teamId": 16, "teamName": "VfB Stuttgart", "shortName": "Stuttgart", "teamIconUrl": "" },
            "team2": { "teamId": 7, "teamName": "Borussia Dortmund", "shortName": "Dortmund", "teamIconUrl": "" },
            "goals": [{ "goalID": 1, "scoreTeam1": 0, "scoreTeam2": 1, "matchMinute": 70, "goalGetterName": "M. Beier", "isPenalty": false, "isOwnGoal": false }],
            "matchResults": [] }]);
        let start = parse_utc("2026-09-19T16:30:00Z").unwrap();
        let ms = parse_oldb(&v, "bl1", start + 95 * 60_000);
        assert_eq!(ms[0].state, "in");
        assert_eq!(ms[0].clock, "80'");
        assert_eq!((ms[0].home.score.as_str(), ms[0].away.score.as_str()), ("0", "1"));
        assert_eq!(ms[0].away.abbr, "BVB");
        assert_eq!(ms[0].events[0].title, "Tor für Dortmund!");
    }
}
