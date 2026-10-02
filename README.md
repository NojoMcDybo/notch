# Notch

Dynamic-Island-artige Infoanzeige für Windows, bündig an der oberen Bildschirmkante.
Tauri v2 + Rust + Vanilla-TS.

## Starten

```
npm install
npm run tauri dev
```

Beenden über das Tray-Symbol → „Notch beenden".

## Zustände

| Zustand   | wann                                         |
|-----------|----------------------------------------------|
| idle      | nichts los: kleine schwarze Form             |
| compact   | Musik läuft (Nachlauf nach Pause einstellbar, Standard 30 s) oder eine sichtbare Quelle hat etwas |
| expanded  | Maus drauf, Datei wird draufgezogen, oder Activity mit `alert: true` (3,5 s) |

## Kleine Notch: Rangliste (Einstellungsfenster)

Was die zugeklappte Notch zeigt, entscheidet eine Rangliste (`src/compact.ts`, Schema in `src/settings-model.ts`, gespeichert unter `compact` in `config.json`). Einstellungen öffnen: Zahnrad in der aufgeklappten Notch oder Tray → „Einstellungen …“ — eigenes Fenster, nicht in der Notch.

- Quellen: Blutzucker (Haze / Activity mit `chart`), Musik, Timer (`notch:timer`), Puls (Activity mit `pulse`: Garmin über Haze `haze:hr`, sonst Helio — die höhere `priority` gewinnt, aufgeklappt steht nur eine Puls-Zeile), Folio (`app: "Folio"`), Andere Apps.
- Standard: Blutzucker, Musik, Timer, Puls, Folio, Andere — zwei Plätze gleichzeitig.
- Musik + etwas anderes: Pegel-Balken wandern neben das Cover statt zu verschwinden.
- Timer läuft: die Notch wird breiter, der Timer hängt sich rechts an und verdrängt nichts (abschaltbar → reiht sich ein).
- Puls ab Schwelle (Standard 140 bpm) ganz nach oben, zurück erst unter Schwelle − 5.
- Blutzucker außerhalb des Zielbereichs ganz nach oben (schlägt auch hohen Puls).
- Folio blättert, liegt aber nicht in der Notch: Seitenzahl übernimmt kurz (Standard 2 s) den letzten Platz.
- Pro Quelle ausblendbar (nur kompakt; aufgeklappt bleibt alles da).

Bei Vollbild-Programmen auf dem Hauptmonitor verhält sich die Notch nach **Einstellungen › Andocken › Im Vollbild**:
- **Ausblenden** (Standard): Sie fährt weg.
- **Am Rand**: Sie fährt weg; Maus an die Bildschirmkante, wo sie sonst sitzt, holt sie heraus, 0,4 s nach dem Wegfahren der Maus verschwindet sie wieder.
- **Nur anzeigen**: Sie bleibt klein sichtbar, ist aber durchklickbar (Klicks gehen ans Vollbild-Programm) und klappt nicht auf.

Exklusives Vollbild (manche Spiele) lässt kein Fenster darüber zu; dort bleibt die Notch in jedem Fall unsichtbar.

## Andocken: oben, links, rechts

Die Form (nicht Knöpfe/Regler/Dateien) mit gedrückter Maus greifen und Richtung Kante ziehen, beim Loslassen dockt sie an: linkes Viertel des Bildschirms → links, rechtes → rechts, Mitte → oben. Alternativ im Tray-Menü. Die Wahl wird gespeichert (`%APPDATA%\de.nojo.notch\config.json`).

Seitlich ist die Notch eine senkrechte Pille; aufgeklappt zeigt sie **alles auf einmal** (Musik, Activities, Ablage, Timer) statt Reitern.

## Sprachassistent (OpenAI Realtime)

Mikrofon-Knopf oben rechts in der aufgeklappten Notch oder Tastenkürzel (erstes freies aus Strg+Alt+Leertaste → Strg+Umschalt+Alt+Leertaste → Strg+Alt+N; das aktive steht im Tooltip des Knopfs). Nochmal drücken = auflegen; nach 40 s Stille legt er selbst auf.

- Modell `gpt-realtime-2.1`, Stimme `marin`, Sprache rein/raus über WebRTC direkt zu OpenAI
- Der API-Schlüssel bleibt in Rust (`src-tauri/src/voice.rs`); das Frontend bekommt nur einen kurzlebigen Sitzungsschlüssel
- Schlüssel: in **Einstellungen › App** eintragen. Die Notch prüft ihn bei OpenAI und speichert ihn mit Windows (DPAPI, nur dieses Benutzerkonto) verschlüsselt in `%APPDATA%\de.nojo.notch\openai-key.bin`; angezeigt wird danach nur `sk-…` plus die letzten vier Zeichen. Ohne Schlüssel zeigt die Notch „Schlüssel eintragen“ und öffnet genau diese Einstellung. Die Umgebungsvariable `OPENAI_API_KEY` hat Vorrang; eine alte Klartextdatei `openai-key.txt` wird beim ersten Start übernommen und gelöscht.
- Werkzeuge des Assistenten: Musik steuern, Lautstärke, Timer starten/stoppen, Status der Notch lesen, Datei aus der Ablage öffnen oder konvertieren
- Mikrofonfreigabe wird für das Notch-Fenster automatisch erteilt (nur Mikrofon)
- Kosten: API-Abrechnung pro Audiominute, getrennt von einem ChatGPT-Abo

## Was man anklicken kann

**Jetzt** (Kreis-Symbol)
- Cover / Titel → holt den Player nach vorn
- Fortschrittsleiste → klicken oder ziehen = spulen
- Zurück / Play-Pause / Weiter
- Lautsprecher → stumm; Leiste daneben oder Mausrad über dem Player = Lautstärke
- Pegel-Balken in der kleinen Notch folgen dem echten Ausgangspegel des Players (Core Audio Peak-Meter, ~30×/s)
- Quellen-Chip (z. B. „Spotify ⇄") → zur nächsten Medienquelle, wenn mehrere laufen
- Activity-Zeile → öffnet die zugehörige App (`open`), Knöpfe darunter (`actions`), × oder Rechtsklick = weg

**Ablage** (Fach-Symbol)
- Dateien auf die Notch ziehen → landen hier (nur Verweise, keine Kopien; bleiben über Neustarts)
- Klick = auswählen (Strg/Umschalt = mehrere), Doppelklick = öffnen, Ziehen = Datei wieder rausziehen
- Rechtsklick-Menü: Öffnen, Im Ordner zeigen, Umwandeln in …, Aus der Ablage nehmen
- Unten erscheint nur noch während einer Umwandlung eine Statusleiste (läuft → grün fertig / rot Fehler)
- **Zwischenablage**: die letzten 5 kopierten Dinge — Text, Dateien (aus dem Explorer kopiert) und Bilder (z. B. Screenshots). Klick = wieder in die Zwischenablage, Bilder/Dateien rausziehen, „+“ = in die Ablage (Bilder werden dafür unter Bilder\Notch gespeichert), × = aus dem Verlauf. Nur im Speicher; Inhalte, die Passwortmanager als „nicht für den Verlauf“ markieren, werden übersprungen (wie beim Windows-Verlauf Win+V). Code: `src-tauri/src/clipboard.rs`
- Neu kopiert → die Notch fährt kurz auf (~2,3 s): das kopierte Ding fällt von der Bildschirmkante in die Ablage-Schale, die Schale federt, grüner Punkt, Vorschau daneben. Ist die Notch schon offen, rutscht der Eintrag oben in die Liste und der Ablage-Reiter zuckt. Erneutes Kopieren aus dem Verlauf löst nichts aus.
- Konvertieren: Bilder nativ (PNG, JPG, WEBP, GIF, BMP, TIFF, ICO, ≤ 1600 px). Audio (MP3, M4A, WAV, FLAC, OGG) und Video (MP4, GIF, MP3) über ffmpeg (`winget install Gyan.FFmpeg`, wird auch im winget-Paketordner gefunden). Ergebnis landet neben dem Original, nie überschrieben.

**Timer** (Stoppuhr-Symbol)
- Drehräder wie in der iPhone-Uhr (Std./Min./Sek., endlos): ziehen mit Schwung, Mausrad = ein Schritt, Klick auf eine Zahl springt hin; schlichter Start-Knopf, gedämpfte Optik. Letzte Einstellung bleibt gemerkt.
- Läuft als Activity mit Pause, +1 min, Stopp; Balken gleitet flüssig (`ends_at`); Ton und Aufklappen am Ende

## Live Activities — so hängt sich jede App an

Die Notch hört auf `http://127.0.0.1:47800`.

| Methode | Pfad              | Zweck |
|---------|-------------------|-------|
| POST    | `/activity`       | anlegen / ersetzen (gleiche `id`) |
| DELETE  | `/activity/<id>`  | entfernen |
| GET     | `/activities`     | aktuelle Liste |
| GET     | `/events?after=N` | Klicks auf Zeilen/Knöpfe (für Apps ohne eigenen Server) |
| POST    | `/shelf`          | `{"paths": [...]}` → Dateien in die Ablage legen (z. B. nach einem Export) |
| GET     | `/health`         | `ok` |

```jsonc
{
  "id": "blutzucker",        // Pflicht, eindeutig ("notch:" ist reserviert)
  "title": "Blutzucker",     // Pflicht
  "app": "Glukose",
  "subtitle": "stabil · vor 2 min",
  "value": "112",
  "unit": "mg/dL",
  "icon": "🩸",              // Emoji, Text oder data:-URL (SVG/PNG)
  "color": "#5ee38a",
  "progress": 0.62,          // 0..1 -> Balken, negativ -> läuft ohne Ende
  "ttl": 300,                // Sekunden bis Auto-Entfernen
  "priority": 1,             // höchste landet im kompakten Zustand
  "alert": false,            // true = Notch klappt kurz auf
  "open": "C:\\Pfad\\App.exe", // Klick auf die Zeile: .exe wird nach vorn geholt oder gestartet,
                             // URL/Datei wird geöffnet, "reveal:<pfad>" zeigt im Explorer
  "actions": [               // Knöpfe unter der Zeile
    { "id": "snooze", "label": "Später", "icon": "pause",
      "post": "http://127.0.0.1:5000/notch" },   // bekommt {"activity","action"}
    { "id": "log", "label": "Protokoll", "open": "C:\\…\\log.txt" }
  ],
  "ends_at": 1790870000000,  // Countdown: ms seit 1970, an dem progress 0 ist -> Balken läuft flüssig
  "pulse": 72,               // Herzfrequenz: Symbol schlaegt in diesem Takt (Phase laeuft weiter, z. B. Helio)
  "input": {                 // Eingabefeld in der aufgeklappten Zeile (z. B. Suche)
    "placeholder": "In Folio suchen",
    "value": ""              // Stand aus Sicht der App; die Notch übernimmt ihn, solange man nicht tippt
  }
}
```

### Verlaufs-Activities (Blutzucker aus Haze)

Optionaler Feldblock an `POST /activity` — sobald `chart` da ist, zeichnet die Notch statt der normalen Zeile eine Verlaufs-Karte:

```jsonc
{
  "id": "haze:bg", "app": "Haze", "title": "Blutzucker",
  "value": 112, "unit": "mg/dL",          // value darf Zahl oder Text sein
  "trend": "down45",                      // up2 | up | up45 | flat | down45 | down | down2
  "delta": -7,                            // Wert minus vorheriger Messwert
  "chart": { "low": 70, "high": 180,
             "points": [[1790870000000, 119], ...],   // [epoch_ms, wert], 24 h, die Notch schneidet selbst
             "ranges": [3, 6, 12, 24], "range": 3 },
  "pid": 12345,                           // fuer AllowSetForegroundWindow beim Doppelklick
  "ttl": 900, "alert": false
}
```

- Zu: nur Wert + Pfeil + Änderung (`112 ↘ −7`), Farbe aus low/high (über high bernstein, unter low rot). Seitlich ohne Änderung.
- Auf: Kopf mit Wert/Pfeil/Änderung/Alter und Pillen 3/6/12/24 Std., darunter der Graph: weiße Punkte im Zielbereich, bernstein über `high`, rot unter `low`, gestrichelte Grenzlinien mit Beschriftung rechts, drei Uhrzeiten unten. Punkt antippen → Zeit und Wert. Doppelklick → `open`-Ereignis (siehe unten).
- **Veraltet:** Maßgeblich ist der Zeitstempel des letzten Punkts, nicht die letzte Sendung. Ab 12 Min. ohne neuen Messwert grau, kein Pfeil, keine Änderung, „vor X Min“. Läuft die `ttl` ab, bleibt der Eintrag stehen und zeigt „keine Daten“ (verschwindet nur per `DELETE` oder ×).
- **Alarme** entscheidet die App (`alert`), die Notch klappt dann nur kurz auf.
- **Doppelklick auf den Graphen:** Die Notch ruft `AllowSetForegroundWindow(pid)` auf, legt `{"activity":"haze:bg","action":"open"}` in `GET /events` und gibt den Fokus 1,2 s lang nicht an das vorige Fenster zurück. Die App pollt `/events` (200 ms) und holt sich selbst nach vorn (Restore/Show/Focus) — klappt so auch aus dem Tray. Bewusste Ausnahme zu „ein Klick klaut nie den Fokus“.
- Zum Testen ohne Haze: `.\tools\bg-demo.ps1` (Optionen `-AgeMin 15`, `-Ttl 20`, `-Alert`, `-Remove`).

Eingabefeld: Tippen landet (entprellt) als Ereignis `input`, Enter als `submit`, Umschalt+Enter als `submit-prev`,
jeweils mit `value` in `GET /events`. Bei Enter holt die Notch außerdem `open` nach vorn. Solange das Feld den Fokus
hat, behält die Notch die Tastatur; ist das Feld leer und die Maus weg, gibt sie sie sofort zurück.

Eingebaute Knopf-Symbole: `play pause stop check close folder open restart plus`. Ohne Symbol wird `label` als Text gezeigt.

Beispiele:

```powershell
.\tools\activity.ps1 -Id timer -Title "Pasta" -Value "4:12" -Icon "⏱" -Ttl 260
.\tools\activity.ps1 -Id timer -Remove
```

```python
import requests
requests.post("http://127.0.0.1:47800/activity", json={"id": "bz", "title": "Blutzucker", "value": "112", "unit": "mg/dL"})
```

```js
fetch("http://127.0.0.1:47800/activity", { method: "POST", body: JSON.stringify({ id: "x", title: "Hallo" }) });
```

Browser-Seiten dürfen nur von `localhost`/`127.0.0.1`/`tauri://` aus schreiben — fremde Websites werden abgewiesen.
`post`-Rückmeldungen gehen nur an localhost.

## Angebunden

- **Folio** (`D:\Dev\folio\src\notch.ts` + `src-tauri\src\notch.rs`): jedes offene Dokument erscheint mit Deckelbild und „Seite x von y“ — beim Scrollen höchstens alle 80 ms nachgeführt, in fester Reihenfolge über einen Rust-Faden. Aufgeklappt steht darunter dauerhaft **Folios Suchfeld**: Tippen sucht im Dokument (Treffer stehen in der Zeile), Enter/Umschalt+Enter springt zum nächsten/vorigen Treffer und holt das Fenster nach vorn. Folio holt die Eingaben alle 200 ms über `GET /events` ab. Klick auf die Zeile holt genau dieses Dokumentfenster nach vorn. Auffrischen jede Minute, ttl 180 s, Entfernen beim Schließen. `notchShelf(paths)` legt Dateien in die Ablage.
- **Haze** (Electron-App, `D:\Dev\haze`, Repo NojoMcDybo/Haze): Nightscout-Dashboard (Dexcom Share über Nightscout als Brücke), hält selbst bis zu 600 Messwerte (~2 Tage). `desktop/notch-bridge.cjs` schickt bei jedem neuen Messwert `haze:bg` mit Wert, Trend (Sensor, sonst Dexcom-Schwellen), Änderung und 24 h Verlauf, `ttl` 900 s + Auffrischen jede Minute, `alert` beim Wechsel in hoch/tief; beim Beenden wird der Eintrag gelöscht. Doppelklick auf den Graphen holt das Dashboard nach vorn (Haze pollt `/events`). Seit Haze-PR #4/#5 in `main`; einen Haze-Branch testen: `tools\haze-branch-start.cmd` per Explorer starten.
- Der ältere C#-Prototyp (jetzt `D:\Dev\_archiv\haze-widget-lab`) (Widget Lab, nur Demowerte, id `haze-bz`) ist nicht mehr die angebundene App.

## Musik-Reaktion

Einstellungen › Anzeige › Musik › **Notch reagiert auf Musik**: *Aus* (Standard) / *Auto* / *Zacken* / *Welle* / *Puls*, dazu **Stärke** (50–200 %). Die schwarze Kante verformt sich zur Musik, Bass in der Mitte, Höhen nach außen.

- **Abgriff** (`spectrum.rs`): WASAPI-Loopback genau des Ausgangs, auf dem der Player spielt (z. B. „Sonar - Media“), sonst des Standardausgangs. Läuft nur, wenn die Einstellung an ist, Musik spielt und kein Vollbild-Programm vorne ist; sonst ist der Abgriff geschlossen.
- **Analyse** (`music_style.rs`, getestet mit künstlichen Signalen): FFT (2048) alle 512 Samples, 24 logarithmische Bänder 40 Hz–16 kHz mit automatischer Aussteuerung; Schläge aus dem pegelrelativen spektralen Fluss (Bass doppelt) mit adaptiver Schwelle und Mindeststärke; Tempo aus der Autokorrelation der erkannten Schläge über 8 s (Halbtempo-Korrektur).
- **Auto**: *Zacken* bei deutlichem Takt und ≥ 135 BPM oder dichten, basslastigen Schlägen; *Welle* ohne Takt und mit wenig Schlägen; sonst *Puls*. Ein Wechsel braucht drei Bestätigungen im Sekundenabstand, nach einem Songwechsel zählt die erste Einschätzung. Die Schwellen sind Startwerte; das Einstellungsfenster zeigt live, was erkannt wird.
- **Zeichnen** (`src/music-react.ts`): Canvas an der Kante, ~30 Pakete/s, `requestAnimationFrame` nur solange Daten kommen; bei „Bewegung reduzieren“ aus.

## Updates

Notch prüft 20 s nach dem Start und danach alle 6 h, ob es auf GitHub eine neue Version gibt (Tray › **Nach Updates suchen** prüft sofort). Gibt es eine, erscheint in der Notch „Update verfügbar“ mit **Installieren** / **Später**; installiert wird nur nach Klick und nur, wenn das Update mit dem Schlüssel aus `tauri.conf.json` (`plugins.updater.pubkey`) signiert ist.

Neue Version veröffentlichen: Version in `package.json`, `src-tauri/Cargo.toml` und `src-tauri/tauri.conf.json` anheben, committen, Tag `v<version>` pushen. Der Workflow `.github/workflows/release.yml` baut, signiert (Secret `TAURI_SIGNING_PRIVATE_KEY`) und lädt Installer und `latest.json` ins Release. Der private Schlüssel liegt lokal in `%USERPROFILE%\.tauri\nojo-updater.key` und gehört nie ins Repo.

## Aufbau

- `src-tauri/src/lib.rs` — Fenster, Platzierung, Maus-Polling (Klick-durch außerhalb der Form), Tray
- `src-tauri/src/media.rs` — Now Playing über die Windows-Media-Session, Quellenwahl, Spulen
- `src-tauri/src/audio.rs` — Systemlautstärke (Core Audio)
- `src-tauri/src/spectrum.rs`, `music_style.rs` — Musik-Reaktion: Loopback-Abgriff, Analyse, Stil
- `src-tauri/src/activities.rs` — HTTP-Schnittstelle, Klicks, Events
- `src-tauri/src/open.rs` — öffnen / nach vorn holen / localhost-Rückmeldung
- `src-tauri/src/timer.rs` — Timer als Activity
- `src-tauri/src/update.rs` — Update-Prüfung und -Installation als Activity
- `src-tauri/src/shelf.rs`, `convert.rs` — Ablage und Konvertieren
- `src-tauri/src/win.rs` — Win32: Cursor, Vollbild, Fensterstile, Fenster nach vorn holen
- `src/main.ts`, `src/styles.css` — Form, Federanimation, Ansichten

## Lizenz

MIT, siehe [LICENSE](LICENSE).
