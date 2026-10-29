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

Aufgeklappt hat die Notch keine feste Höhe mehr: Sie wird so hoch wie ihr Inhalt (das Fenster ist so hoch wie der Monitor, außerhalb der Form klickt man durch). Erst am Bildschirmrand scrollt sie.

## Kleine Notch: Rangliste (Einstellungsfenster)

Was die zugeklappte Notch zeigt, entscheidet eine Rangliste (`src/compact.ts`, Schema in `src/settings-model.ts`, gespeichert unter `compact` in `config.json`). Einstellungen öffnen: Zahnrad in der aufgeklappten Notch oder Tray → „Einstellungen …“ — eigenes Fenster, nicht in der Notch.

- Quellen: Blutzucker (Haze / Activity mit `chart`), Musik, Timer (`notch:timer`), **Spielzeit** (Minute bzw. Viertel des Spiels in der Mitte, aus dem Live-Sport), Puls (Activity mit `pulse`: Garmin über Haze `haze:hr`, sonst Helio — die höhere `priority` gewinnt, aufgeklappt steht nur eine Puls-Zeile), Folio (`app: "Folio"`), Andere Apps.
- Standard: Blutzucker, Musik, Timer, Spielzeit, Puls, Folio, Andere — zwei Plätze gleichzeitig.
- Spielzeit am Rand: Live-Punkt + „68'“ als eigenes Element; die Mitte zeigt dann nur Wappen–Stand–Wappen. Bekommt sie keinen Platz (oder ist ausgeblendet), steht die Minute wie bisher klein in der Mitte.
- Neue Werte (Minute, Stand, Blutzucker …) ändern nur ihre Zahl (`src/morph.ts`): nichts wird neu gebaut, Wappen, Cover und Pegel blinken nicht.
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

## Aussehen der Einstellungen

Das Einstellungsfenster spricht dieselbe Designsprache wie Folio, Haze und Helio (Quelle `D:\Dev\nojo-design`, Kopie unter `src/nojo/`): Folio-Grau, Fensterknöpfe als Glaspille, **Liquid Glass** wie in Folio (die Reiter schweben über dem Inhalt, der darunter durchscrollt; ihr Rand bricht ihn), Segmente als Glasrinne mit gleitender Perle, Karten statt Rahmen, Schalter leuchten, wenn sie an sind. Statt einer Bildlaufleiste zeigt eine dünne **Lichtleiste** am rechten Rand, wo man ist (leuchtet beim Scrollen kurz auf, lässt sich ziehen). Die Notch selbst bleibt schwarz. Alle Symbole (Notch und Einstellungen) kommen aus der gemeinsamen Bibliothek (`src/nojo/nojo-icons.ts`, Quelle `nojo-design/assets/icons.mjs`); `index.html` enthält nur Platzhalter `<svg data-icon="…">`.

## Andocken: oben, links, rechts

Die Form (nicht Knöpfe/Regler/Dateien) mit gedrückter Maus greifen und Richtung Kante ziehen, beim Loslassen dockt sie an: linkes Viertel des Bildschirms → links, rechtes → rechts, Mitte → oben. Alternativ im Tray-Menü. Die Wahl wird gespeichert (`%APPDATA%\de.nojo.notch\config.json`).

Seitlich ist die Notch eine senkrechte Pille; aufgeklappt zeigt sie **alles auf einmal** (Musik, Activities, Ablage, Timer) statt Reitern.

## Sprachassistent (OpenAI Realtime)

Mikrofon-Knopf oben rechts in der aufgeklappten Notch oder Tastenkürzel (Standard: erstes freies aus Strg+Alt+Leertaste → Strg+Umschalt+Alt+Leertaste → Strg+Alt+N; eigenes unter Einstellungen › App › Tastatur › **Ändern**; das aktive steht im Tooltip des Knopfs). Nochmal drücken = auflegen.

- **Sofort losreden:** Das Mikrofon läuft ab dem Drücken mit, auch während die Verbindung noch aufgebaut wird („Hört zu · verbindet …“). Steht sie, geht das schon Gesagte als Audio hinterher (`conversation.item.create` mit `input_audio`, PCM16 24 kHz, in Stücken von höchstens 2,5 s), danach läuft das Mikro live weiter. War man beim Verbinden schon fertig, antwortet er direkt darauf.
- **Schließt von selbst:** Ist die Antwort zu Ende gesprochen, hört er noch 8 s auf eine Rückfrage, dann legt er auf und die Notch klappt zu. Per Controller verschwindet die Notch sofort nach der Antwort; die Verbindung wartet unsichtbar 20 s auf den nächsten Druck. Ohne Gespräch legt er spätestens nach 40 s auf.

- **Modell wählbar** (Einstellungen › App › Sprachassistent): `gpt-realtime-2.1-mini` (Standard, etwa ein Drittel der
  Kosten) oder `gpt-realtime-2.1` (genauer bei mehrstufigen Befehlen). Stimme `marin`, Sprache rein/raus über WebRTC direkt zu OpenAI
- **Claude als Alternative** (gleiche Einstellung, Modell Haiku 5.5 / Sonnet 5.5 / Opus 5.5): Claude hat keine
  Echtzeit-Sprachschnittstelle, deshalb eine Kette (`src/voice-claude.ts`): die Notch nimmt den Satz selbst auf (Ende nach
  0,8 s Stille bzw. Loslassen am Controller), OpenAI `gpt-transcribe` schreibt mit, Claude entscheidet mit denselben
  Werkzeugen, OpenAI `gpt-4o-mini-tts` liest vor. Braucht zusätzlich einen Claude-Schlüssel (`sk-ant-…`, gespeichert wie
  der OpenAI-Schlüssel in `claude-key.bin`, `ANTHROPIC_API_KEY` hat Vorrang); Antworten kommen etwa 1–2 s später als mit
  Realtime. Anfragen laufen über Rust (`voice.rs`: `claude_messages`, `voice_stt`, `voice_tts`), nur freigegebene Modelle.
- Der API-Schlüssel bleibt in Rust (`src-tauri/src/voice.rs`); das Frontend bekommt nur einen kurzlebigen Sitzungsschlüssel
- Schlüssel: in **Einstellungen › App** eintragen. Die Notch prüft ihn bei OpenAI und speichert ihn mit Windows (DPAPI, nur dieses Benutzerkonto) verschlüsselt in `%APPDATA%\de.nojo.notch\openai-key.bin`; angezeigt wird danach nur `sk-…` plus die letzten vier Zeichen. Ohne Schlüssel zeigt die Notch „Schlüssel eintragen“ und öffnet genau diese Einstellung. Die Umgebungsvariable `OPENAI_API_KEY` hat Vorrang; eine alte Klartextdatei `openai-key.txt` wird beim ersten Start übernommen und gelöscht.
- Plan für den nächsten Schritt (PC-Steuerung, Handeln schon während man spricht): [`docs/jarvis.md`](docs/jarvis.md)
- **PC-Zugriff (Jarvis, Schritt 1):** „Öffne Chrome“, „hol Discord nach vorn“, „such nach …“ — Programme aus dem
  Startmenü/`shell:AppsFolder` starten oder nach vorn holen, offene Fenster wechseln, Seiten und Suchen im Browser
  öffnen (`src-tauri/src/jarvis.rs`). Nur, was im Katalog steht, Seiten nur http(s); jede Aktion zeigt sich kurz als Zeile.
  Frisch Gestartetes lässt sich dort mit × wieder schließen (15 s lang).
- **Anzeige:** Der Assistent löst sich als Kreis (so hoch wie die eingeklappte Notch) flüssig rechts aus der Notch
  (seitlich angedockt: darunter) und fließt am Ende zurück; Klick beendet ihn. Schwarz-weiß, je Zustand ein eigenes
  Punktmuster: verbinden (Ladekreis), zuhören (Ring weitet sich mit dem Mikrofon), nachdenken (Schweif kreist),
  arbeiten (vier Punkte pumpen), sprechen (Balken). Der Kreis atmet mit der Stimme und staucht sich bei jedem Wechsel. `src/bubble.ts`, `bubble.css`,
  Entwurfsseite `tools/bubble-preview.html`. Das aufgeklappte Panel gibt es nur noch für Fehler und fehlenden Schlüssel.
- **„Hey Jarvis“ (optional, Einstellungen › App):** Das Mikrofon hört lokal auf das Wort (openWakeWord-Modelle in
  `public/wakeword/`, Pipeline `src/wake/`, ONNX Runtime auf der CPU) und startet dann den Assistenten; vorher geht nichts
  an OpenAI. Pausiert, solange der Assistent selbst das Mikrofon hat. Modelle nur nicht-kommerziell (CC BY-NC-SA 4.0,
  `public/wakeword/README.md`). Test: `tools/wake-test.mjs` (Node) und `tools/wake-test.html` (Browser).
- **Schon beim Sprechen handeln (Jarvis, Schritt 2, optional):** Einstellungen › App › „Schon beim Sprechen handeln“
  lädt ein feingetuntes Laya-Modell (354 MB, GitHub-Release `laya-jarvis-1`, Größe und SHA-256 fest in
  `src-tauri/src/laya.rs`). Dann schreibt eine zweite OpenAI-Sitzung (`gpt-live-transcribe`, ~1,7 ct/min Sprechzeit)
  beim Sprechen mit; sobald ein Programm ganz genannt ist und Laya „starten“/„wechseln“ erkennt, startet die Notch
  es schon vor dem Satzende (`src/jarvis-live.ts`, Laya im Worker `src/laya/worker.ts` auf der Grafikkarte, ~0,2 s).
  „… nein, Firefox“ oder „ich meine …“ schließt das gerade Gestartete wieder; der Assistent macht danach wie gewohnt weiter.
- **Bildschirm lesen und bedienen:** `bildschirm_lesen` liefert die Bedienelemente des vorderen Fensters mit Nummern
  (UI Automation; meldet ein Fenster nichts, liest die Windows-Texterkennung offline), `klicken`, `tippen`, `taste`
  bedienen es, `bildschirm_ansehen` schickt nur auf Anforderung ein verkleinertes Bild ans Modell. Knöpfe wie
  Senden/Kaufen/Löschen erst nach Rückfrage, nie in Passwortfelder (`src-tauri/src/screen.rs`).
- **Spotify ohne Anmeldung:** Jarvis bedient die Spotify-App selbst — Playlists und Lieblingssongs über die
  Seitenleiste, Songs/Künstler/Alben über die Suchadresse der App und den passenden „… wiedergeben“-Knopf
  (`screen.rs`, `spotify_local`). Das geht für alle; die Web-API unten ist optional (Spotify erlaubt seit Februar
  2026 je Entwickler-App nur 5 freigeschaltete Nutzer).
- **Spotify direkt:** Nach dem Verbinden (Einstellungen › App, eigene Spotify-App mit Redirect-URI
  `http://127.0.0.1:8738/callback`, Anmeldung mit PKCE ohne geheimen Schlüssel) spielt Jarvis Playlists,
  Lieblingssongs, Songs, Künstler und Alben über die Spotify-Web-API ab (`src-tauri/src/spotify.rs`, Premium nötig).
  Das Erneuerungs-Token liegt mit Windows verschlüsselt in `spotify.bin`.
- **Gedächtnis und Vorhersage:** „Merk dir das als Zocken“ speichert einen Ablauf (z. B. Playlist + Spiel +
  Discord-Kanal), „Zocken“ führt ihn aus. Discord-Sprachkanäle lernt Jarvis beim Beitreten mit ihrem Server
  (danach reicht der Kanalname, ohne Namen der zuletzt genutzte); dazu merkt er sich zuletzt Gespieltes und
  Gestartetes und nimmt bei fehlenden Angaben das Wahrscheinlichste statt nachzufragen
  (`src-tauri/src/memory.rs`, `jarvis-memory.json`; Discord-Beitritt per Bedienschnittstelle in `screen.rs`).
- **Lernen:** „Merk dir diese Seite als Uni“ übernimmt die Adresse der gerade im Browser offenen Seite, „mit
  Zockprogramm meine ich Steam“ einen eigenen Programmnamen (nur Programme aus dem Startmenü). `seite_oeffnen` und
  `programm_starten` fragen diese Namen zuerst (`memory.rs`, Werkzeug `lernen`).
- **League spielen:** „Starte League“ legt die Lobby im zuletzt gespielten Modus an, startet die Suche und nimmt das
  gefundene Spiel an (`lol.rs`, bis 20 Minuten, hört bei Championauswahl/Abbruch auf); nur „öffne den Client“ öffnet
  bloß den Client.
- **Selbst schließen:** Nach erledigtem Auftrag, Dank/Abschied oder wenn er merkt, dass nicht mehr mit ihm geredet
  wird, legt Jarvis selbst auf (Werkzeug `beenden`).
- **Chatprogramme:** Enter in Discord, WhatsApp, Telegram, Teams … schickt ab und geht deshalb nur nach Rückfrage.
- **League of Legends:** „Starte eine Runde ARAM“ legt über die lokale Client-Schnittstelle eine Lobby an und startet
  die Suche, bei Bedarf nach dem Start des Clients (`src-tauri/src/lol.rs`). Annehmen und Championauswahl bleiben bei dir.
- Werkzeuge des Assistenten: Programme/Fenster/Seiten (siehe oben), Videos abspielen, Bildschirm lesen/bedienen, LoL-Runde, Musik steuern, Lautstärke, Timer/Pomodoro starten/stoppen, Status der Notch lesen, Datei aus der Ablage öffnen oder konvertieren
- Mikrofonfreigabe wird für das Notch-Fenster automatisch erteilt (nur Mikrofon)
- Kosten: API-Abrechnung pro Audiominute, getrennt von einem ChatGPT-Abo

## Was man anklicken kann

**Jetzt** (Kreis-Symbol)
- **Karten** (Musik, Zeilen der Apps, Blutzucker, Pomodoro, eigenes LoL-Spiel, Profispiel, Live-Sport): mit der Maus greifen und
  ziehen = eigene Reihenfolge (die Karte hebt sich an, die anderen gleiten zur Seite, beim Loslassen federt sie ein). Gemerkt
  in `config.json` → `cards` (`src/cards.ts`). Neue Karten stehen an ihrer natürlichen Stelle.
- **×** auf einer Karte einer App (Haze, Folio, Helio, Vantage …) oder auf Live-Sport blendet sie aus — **dauerhaft**, auch wenn
  die App sie jede Minute neu schickt. Unten erscheint dann ein kleines **+**: Liste der ausgeblendeten Karten, Klick holt sie
  zurück. Nur die aufgeklappte Ansicht; was die kleine Notch zeigt, regelt weiter die Rangliste. Eigene Einträge (Timer,
  Umwandeln, iPhone-Anfrage, Update) beendet × wie bisher. Nach 30 Tagen, in denen eine ausgeblendete Karte nicht mehr
  auftaucht, vergisst die Notch sie.
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
- **Vorschaubilder** statt Dateityp (Ablage und Zwischenablage): dieselben wie im Explorer über die Windows-Shell
  (`src-tauri/src/thumb.rs`) — Fotos auch als HEIC (braucht die HEIF-Erweiterung aus dem Store), Videobilder,
  PDF/Office, sofern ein Vorschau-Anbieter installiert ist, sonst das Programmsymbol. Einfache Bildformate lädt die Notch direkt.
- **Zwischenablage**: die letzten 5 kopierten Dinge — Text, Dateien (aus dem Explorer kopiert) und Bilder (z. B. Screenshots). Klick = wieder in die Zwischenablage, Bilder/Dateien rausziehen, „+“ = in die Ablage (Bilder werden dafür unter Bilder\Notch gespeichert), × = aus dem Verlauf. Nur im Speicher; Inhalte, die Passwortmanager als „nicht für den Verlauf“ markieren, werden übersprungen (wie beim Windows-Verlauf Win+V). Code: `src-tauri/src/clipboard.rs`
- Neu kopiert → die Notch fährt kurz auf (~2,3 s): das kopierte Ding fällt von der Bildschirmkante in die Ablage-Schale, die Schale federt, grüner Punkt, Vorschau daneben. Ist die Notch schon offen, rutscht der Eintrag oben in die Liste und der Ablage-Reiter zuckt. Erneutes Kopieren aus dem Verlauf löst nichts aus.
- Konvertieren: Bilder nativ (PNG, JPG, WEBP, GIF, BMP, TIFF, ICO, ≤ 1600 px). Audio (MP3, M4A, WAV, FLAC, OGG) und Video (MP4, GIF, MP3) über ffmpeg (`winget install Gyan.FFmpeg`, wird auch im winget-Paketordner gefunden). Ergebnis landet neben dem Original, nie überschrieben.

**Timer** (Stoppuhr-Symbol)
- Drehräder wie in der iPhone-Uhr (Std./Min./Sek., endlos): ziehen mit Schwung, Mausrad = ein Schritt, Klick auf eine Zahl springt hin; schlichter Start-Knopf, gedämpfte Optik. Letzte Einstellung bleibt gemerkt.
- Läuft als Activity mit Pause, +1 min, Stopp; Balken gleitet flüssig (`ends_at`); Ton und Aufklappen am Ende
- **Pomodoro:** Chip „Timer ⇄“ oben im Reiter (wie die Medienquelle im Player) schaltet um. Fokus- und Pausenlänge als zwei
  große Zahlen (Mausrad oder Pfeile beim Drüberfahren; Fokus bis 10 min in Einern, darüber in Fünfern). Karte: dünner Ring,
  Zeit groß, Pfirsich = Fokus, Mint = Pause. Ist eine Phase um, hält nichts an: weicher Dreiklang, die Notch klappt kurz auf,
  der Ring ist leer, ein Punkt in der Farbe der nächsten Phase kreist, die **Extrazeit** zählt hoch („+1:24“), bis man
  „Pause starten“ bzw. „Weiter fokussieren“ drückt; dann gleitet die Farbe um und der Ring füllt sich in einem Zug.
  Sprachassistent: Werkzeug `pomodoro_starten`. Gestalten ohne App: `npm run dev`, dann `/tools/pomo-preview.html`.

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
- **Doppelklick auf den Wert** (die große Zahl im Kopf, nur bei `app: "Haze"`): Ereignis `{"activity":"haze:bg","action":"widget"}` in `GET /events` — Haze blendet sein Widget ein oder aus (ab Haze 1.3.2). Ein einfacher Klick auf die Zahl tut nichts.
- Zum Testen ohne Haze: `.\tools\bg-demo.ps1` (Optionen `-AgeMin 15`, `-Ttl 20`, `-Alert`, `-Remove`).

### LoL-Profispiele (Vantage)

Vantage (`esnotch.rs`) schickt laufende Serien gefolgter Teams und die in Vantage geöffnete Serie als Activity
`vantage:esports:<serie>` mit dem Feld `esports` (fertige Daten; Ansicht in `src/esports.ts`):

- **Klein (Mitte):** Logo – Gold – Spielzeit – Gold – Logo, wenn sonst nichts in der kleinen Notch steht; teilt sie
  sich den Platz, nur der Goldvorsprung als positive Zahl auf der Seite des Führenden. Draft: Logos und „Draft“.
- **Auf:** Karte wie in der Übertragung — Kopfleiste (Logo, Kürzel, Türme, Gold, gespiegelt; Kills in der Mitte),
  Baron, Inhibitoren, Drachen und Spielzeit, je Lane Rune mit Level, Items, K/D/A, CS, Champion-Bild und
  Goldvorsprung, Goldverlauf, Serienstand. Schriften wie in Vantage (Cinzel, Saira Semi Condensed, Source Sans 3).
- **Klick** auf die Karte öffnet `open` (`vantage://pro/<serie>/<spiel>`): Vantage zeigt Serie und Spiel.
- **Sprachassistent:** Werkzeug `esports` liefert Stand, Objectives und Lanes.
- Bilder nur von Data Dragon und lolesports. Der Live-Feed liegt etwa vier Minuten hinter dem Spiel; Bans und Picks
  während des Drafts liefert er nicht.
- **Symbole** (Türme, Gold, Kills, Baron, Inhibitoren, Drachen): die Originale aus dem Zuschauer-Scoreboard des Spiels
  (`src/lolicons.ts`, erzeugt mit `python tools/lolicons.py` aus CommunityDragon), Türme/Gold/Kills/Baron/Inhibitoren
  in der Teamfarbe eingefärbt wie in der Übertragung.

### Eigenes LoL-Spiel (Vantage)

Vantage (`ingame.rs`) schickt während eines eigenen Spiels jede Sekunde `vantage:game` mit dem Feld `lol`
(Ansicht in `src/lolgame.ts`). Statt der Spielzeit zeigt die Notch, was gleich wichtig wird:

- **Klein (Mitte) — die Hauptansicht im Spiel:** links eine Vasallenwelle als kurze Meldung, wenn sie spawnt
  (5 s; „Minion wave“, „Cannon wave“, „Super minions Bot“, „Enemy supers Mid“; Porträt pulsiert), sonst das nächste
  Objective (Countdown, letzte Minute hervorgehoben, „is up“, „taken ✓“, „Enemy took Baron“); tot: Respawn. Rechts
  das **CS-Ziel**: CS, frei auch das Ziel am nächsten Meilenstein (`63/70`), Ring bis dahin, Vorsprung/Rückstand
  zum Plan. Texte im Spiel auf Englisch.
- **Auf:** Champion, K/D/A, CS, Knopf „Clip“ (= Alt+F10 in Vantage); CS-Ziel als Leiste mit Meilensteinen alle
  5 Minuten (Soll-Marke wandert live mit, jeder CS füllt nach und steigt als „+1“ auf, erreichte Meilensteine
  leuchten gold, verpasste rot), Hinweis „bis 15:00 noch 21 CS in 2:41 · 7,8/min nötig“; die nächsten Wellen;
  Objectives (Drache mit Element, sobald bekannt, Elder nach der Seele, Larven, Herold, Baron); Drachen beider Teams,
  fehlende Inhibitoren mit Super-Vasallen und Respawn.
- Zwischen zwei Meldungen zählt die Notch selbst weiter (Spielzeit `t` zur Wanduhr `at`) — im gemessenen Tempo (Vorspulen im Übungstool) und ohne Rücksprünge; Pause hält an. Feste Breiten in der kleinen Notch, damit sie im Spiel nicht springt.
- CS-Plan mit 60 s Vorlauf (erste Welle 0:30 plus Weg in die Lane): Soll und Meilensteine zählen ab 1:00.
- **Sprachassistent:** Werkzeug `mein_spiel`.
- Zeiten nach Patch 26.1 (siehe `ingame.rs`); das Ziel stellt man in Vantage ein (Einstellungen › App, 0 = aus).

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
- **Haze** (Electron-App, `D:\Dev\haze`, Repo NojoMcDybo/Haze): Nightscout-Dashboard (Dexcom Share über Nightscout als Brücke), hält selbst bis zu 600 Messwerte (~2 Tage). `desktop/notch-bridge.cjs` schickt bei jedem neuen Messwert `haze:bg` mit Wert, Trend (Sensor, sonst Dexcom-Schwellen), Änderung und 24 h Verlauf, `ttl` 900 s + Auffrischen jede Minute, `alert` beim Wechsel in hoch/tief; beim Beenden wird der Eintrag gelöscht. Doppelklick auf den Graphen holt das Dashboard nach vorn, Doppelklick auf den Wert blendet das Haze-Widget ein/aus (Haze pollt `/events`). Seit Haze-PR #4/#5 in `main`; einen Haze-Branch testen: `tools\haze-branch-start.cmd` per Explorer starten.
- Der ältere C#-Prototyp (jetzt `D:\Dev\_archiv\haze-widget-lab`) (Widget Lab, nur Demowerte, id `haze-bz`) ist nicht mehr die angebundene App.

## Controller

Xbox-Controller und alles, was sich per XInput meldet (GameSir, Steam Input …), auch während ein Spiel vorne ist (`src-tauri/src/gamepad.rs`, ohne Controller wird nur alle 250 ms nachgesehen):

Alle drei Kombinationen lassen sich unter Einstellungen › App › Controller selbst belegen (**Aufnehmen**, Tasten gleichzeitig halten, loslassen): mindestens zwei Tasten, keine doppelt; halten mehrere Kombinationen gleichzeitig, gewinnt die längste. Standard:

| Kombination | Wirkung |
|---|---|
| Steuerkreuz links + RB (R1) | Notch klein heraus – nur im Vollbild mit *Im Vollbild: Am Rand*; nochmal = weg, sonst nach 10 s |
| Steuerkreuz links + RT (R2) | Notch aufgeklappt heraus – ebenso |
| Steuerkreuz links + R3 halten | Nach **1,5 s** Halten hört der Sprachassistent zu (ab 0,25 s füllt sich ein Ring in der Mitte der kleinen Notch; früher loslassen = nichts passiert). Solange gedrückt: aufgeklappt mit „Hört zu“. Loslassen: Notch wird klein, in der Mitte zeigt eine Animation Nachdenken und Sprechen (die Balken folgen seiner Stimme). Ist er fertig, verschwindet die Notch wieder an den Rand bzw. zeigt wieder ihren normalen Inhalt. Erneutes Drücken unterbricht ihn. |

Herausgeholt bleibt die Notch durchklickbar, Klicks gehen weiter ans Spiel. **Grenze:** XInput kann nur mitlesen – das Spiel sieht dieselben Tasten. Wer das nicht will, legt die Kombination z. B. in Steam Input auf eine Taste, die das Spiel nicht nutzt.

## Live-Sport

Einstellungen › **Sport**: oben eine Live-Leiste (Wappen 2:1 Wappen), Lieblingsteams als Wappen (Name im Tooltip; *Hinzufügen* öffnet ein Raster aller Wappen eines Wettbewerbs mit Suche), Wettbewerbe wählen (Bundesliga, 2. Bundesliga, 3. Liga, DFB-Pokal, Frauen-Bundesliga, Länderspiele Deutschland, WM/EM, Champions/Europa/Conference League, Premier League, LaLiga, Serie A, Ligue 1, NFL, NBA, WNBA, NHL, MLB; **Testspiele** der Vereine aus den gewählten Ligen und der Lieblingsteams kommen von selbst dazu), Lieblingsteams stehen immer vorn und in der Mitte, *Alle Spiele* oder *Nur meine Teams*. Überall stehen **Wappen statt Vereinsnamen**; ohne Wappen das Kürzel mit Strich in der Teamfarbe.

- **Kleine Notch:** In der Mitte steht der Spielstand (Wappen `2:2` Wappen `• 68'`). Laufen mehrere Spiele, wechseln sie sich alle 8 s ab (Konferenz); nach einem Tor steht dieses Spiel eine Minute vorn, der Stand leuchtet kurz in der Farbe des Torschützen und sein Wappen hebt sich. Lieblingsteams zusätzlich eine Stunde vor Anpfiff und kurz nach Abpfiff.
- **Meldungen:** Bei Toren (einstellbar: Toren / Wichtigem / Allem / Nie) klappt die Notch 6,5 s auf; die Meldung schwebt ganz oben als Liquid Glass über einem Licht in der Teamfarbe („Tor! · Machino (Elfmeter) · 68'“, rechts Wappen 2:2 Wappen). Im Vollbild klappt nichts auf; bei *Am Rand* / *Nur anzeigen* zeigt sie 7 s den Spielstand klein (abschaltbar).
- **Aufgeklappt:** Karte mit Wappen, Spielstand und Minute, Ticker (Tore, Karten, Wechsel, Halbzeit, bei angesehenen Spielen auch Chancen, Ecken, Abseits, Videobeweis), weitere laufende Spiele zum Antippen, „Spielseite“ öffnet das Spiel im Browser.
- **Wurfbild** (Basketball über ESPN, Schalter „Spielfeld: Ballverlauf, Wurfbild“): ganzes Feld, Heim wirft rechts, Gast links; Treffer als Punkt in der Teamfarbe, Fehlwürfe als Kreuz, der letzte Wurf leuchtet auf, die Glaspille nennt ihn („3. 5:12 · Herro · Dreier ✓“). Darunter je Team Würfe (mit Quote), Dreier und Freiwürfe. Beim Ansehen werden einmal alle Würfe des Spiels geladen (ESPN Core, Ort in Fuß, Korb bei y ≈ 1 – gegen die Distanzen im Spieltext geprüft), danach nur neue.
- Aktualisierungen ändern in der Karte nur Neues: Stand und Minute als Text, neue Tickerzeilen laufen oben ein, Wappen und Spielfeld bleiben stehen.
- **Spielfeld mit Ballverlauf** (nur Fußball über ESPN): Echte Positionsdaten aller Spieler gibt es live nirgends frei. Die Notch nimmt stattdessen jede Ballaktion mit Feldposition und Uhrzeit (Pass von wo nach wo, Flanke, Schuss, Zweikampf, Ballgewinn …) und spielt sie im echten Takt nach: der Ball wandert, beteiligte Spieler erscheinen mit Rückennummer in Teamfarbe und gleiten zu ihrer neuen Position, Schüsse als Linie aufs Tor; blasse Wappen in jeder Hälfte, die Beschriftung der Aktion schwebt als Glaspille über dem Feld. Beim Aufklappen laufen die letzten acht Aktionen als Zusammenfassung — klappt die Notch für ein Tor auf, sieht man den Angriff. Geladen wird nur, solange die Karte zu sehen ist (alle ~6 s, etwa 10–15 MB pro Stunde Zuschauen).
- **Sprachassistent** kennt die Spielstände (Werkzeug `sport`): „Wie steht es bei Gladbach?“
- **Arena** (eigene Sport-App, `D:\Dev\arena`): Spielplan, Tabellen, Teams und das Spielfeld groß. Ist sie installiert, zeigt die Sportkarte den Knopf **In Arena öffnen** (öffnet genau dieses Spiel über `arena://spiel/<schlüssel>`), Einstellungen › Sport einen Verweis. Die Sport-Einstellungen bleiben vollständig hier und gelten in Arena genauso: Arena liest und schreibt sie über `GET`/`PUT http://127.0.0.1:47800/sport/settings` (nur Loopback, bereinigt wie im Einstellungsfenster).

Quellen (`src-tauri/src/sport.rs`), alle ohne Konto und Schlüssel:

| Quelle | Wofür | Status |
|---|---|---|
| ESPN `site.api.espn.com` | Spielstände, Minute, Tore, Karten (alle Ligen außer 3. Liga/Frauen) | frei abrufbar, aber **inoffiziell** – keine Zusage, kann sich ändern; nur privat nutzen |
| ESPN `sports.core.api.espn.com` | jede Ballaktion mit Feldposition (Spielfeld) | wie oben |
| OpenLigaDB `api.openligadb.de` | 3. Liga, Frauen-Bundesliga; Ersatz für Bundesliga, 2. Liga, DFB-Pokal, wenn ESPN 3× hintereinander ausfällt | offiziell frei, Community-gepflegt; nur Tore, Minute geschätzt |

Abfragen: laufendes Spiel alle 15 s, kurz vor Anpfiff alle 30 s, sonst alle 5–20 Min (eine kleine Anfrage pro Wettbewerb). Live-Tracking (Spielerpositionen) haben nur kostenpflichtige Anbieter (DFL/Sportec, Opta, Second Spectrum); freie Tracking-Daten gibt es nur für alte Spiele (z. B. StatsBomb Open Data, Metrica Sports, DFL-Open-Data).

## iPhone ↔ PC (wie AirDrop)

Echtes AirDrop geht unter Windows nicht – Apple funkt dafür über ein eigenes WLAN-Protokoll (AWDL) mit Apple-Zertifikaten. Die Notch bietet drei Wege, die sich fast genauso anfühlen (`src-tauri/src/share.rs`, `iphone.rs`). Einschalten: Einstellungen › **iPhone** (aus, bis man es einschaltet; dann lauscht die Notch im lokalen Netz auf Port 53317 – beim ersten Mal fragt die Windows-Firewall: „Private Netzwerke“ erlauben).

1. **LocalSend** (kostenlos, Open Source, App Store): Die Notch spricht das offene [LocalSend-Protokoll v2](https://github.com/localsend/protocol) und taucht in der App als Gerät „Notch · PC-Name“ auf.
   - iPhone → PC: Foto/Datei › Teilen › LocalSend › Notch. Die Notch klappt auf wie bei einem Anruf: **Annehmen** / **Ablehnen** / **Immer annehmen** (Gerät merken). Fortschritt als Zeile, danach „3 Fotos von Nojos iPhone“; die Dateien liegen in der Ablage und in `Downloads\Notch` (einstellbar). Text und Links aus LocalSend erscheinen zum **Kopieren** bzw. **Öffnen**.
   - PC → iPhone: LocalSend auf dem iPhone offen lassen, dann Ablage › Rechtsklick › **An iPhone senden** › Gerät; auf dem iPhone annehmen.
2. **Ohne App (Browser):** iPhone-Knopf in der Ablage zeigt einen QR-Code. Mit der Kamera scannen → Seite in Safari zum Hochladen (Fotos/Dateien wählen) und zum Laden von Dateien, die man am PC über Rechtsklick › An iPhone senden › **Per QR-Code** anbietet (Fotos: gedrückt halten › Zu Fotos hinzufügen).

3. **Kurzbefehl „Notch“ (ohne App, empfohlen)** (`src-tauri/src/iphone.rs`, Einrichtungsseite `iphone-page.html`): Einstellungen › iPhone › Kurzbefehl › **Einrichten (QR-Code)**. Mit der iPhone-Kamera scannen koppelt das iPhone. Die Adresse für den Kurzbefehl nutzt den Rechnernamen (`http://<PC>.local:53317/k/…`, iOS findet ihn per Bonjour), damit sie eine neue IP übersteht. Die Seite bietet die Adresse zum Kopieren und den fertigen, signierten Kurzbefehl zum Laden (`src-tauri/src/Notch.shortcut`, fragt beim Hinzufügen nach der Adresse; Quelle `tools/shortcuts/notch.cherri`, erzeugt im [Cherri-Playground](https://playground.cherrilang.org) und dort über HubSign signiert, Anleitung im Kopf der Datei). Die Bauanleitung von Hand steht darunter zum Aufklappen.
   - Teilen-Menü › **Notch**: Dateien landen in der Ablage und im Empfangsordner („3 Fotos vom iPhone“, Dateien innerhalb von 8 s zählen als eine Sendung), ein einzelner Link öffnet sich im Browser, anderer Text kommt in die Zwischenablage.
   - **Auf Rückseite tippen** (Bedienungshilfen) startet den Kurzbefehl ohne Eingabe: Die Zwischenablage des iPhones landet am PC.
   - **Verbindungsanzeige:** Aufgeklappt zeigt eine Karte „iPhone“, ob es im WLAN ist (die Notch löst seine Adresse alle 20 s per ARP neu auf, ohne App, auch gesperrt; nach drei Fehlversuchen „nicht im WLAN · zuletzt …“), dazu den Akku, den der Kurzbefehl bei jeder Anfrage mitschickt (oder eine Automation „WLAN verbunden“ / „Ladegerät“ über `/hallo`). Kommt das iPhone ins WLAN, zeigt die kleine Notch kurz „Verbunden“. Die Karte belegt nie einen Platz in der kleinen Notch.
   - **PC → iPhone:** Ablage › Rechtsklick › An iPhone senden › *iPhone*, Explorer › Senden an › **iPhone (Notch)** (Schalter in Einstellungen › iPhone; legt eine Verknüpfung auf ein kleines PowerShell-Skript an, das die Pfade an `127.0.0.1:47800/iphone` schickt) oder an der iPhone-Karte **Zwischenablage senden**. Die Notch zeigt „Für iPhone bereit“ (1 Std., *Zurücknehmen*), danach „Auf dem iPhone“. Der Kurzbefehl ohne Eingabe fragt zuerst `/abholen`: liegt etwas bereit, holt er es (`/datei/<id>`, Fotos/Videos in die Fotos-App, sonst Sichern-Dialog, Text in die Zwischenablage), sonst schickt er seine Zwischenablage. Dateien werden gestreamt (auch große Videos, ohne sie in den Speicher zu laden). Alle Anfragen des Kurzbefehls stehen ohne Schlüssel in `iphone-log.txt` im App-Ordner.
   - **Mitteilung (optional):** Einstellungen › iPhone › „Mitteilung aufs iPhone (ntfy)“ legt ein zufälliges Thema an (`iphone-ntfy.txt`); die Notch schickt an `ntfy.sh/<thema>` nur „NotchDrop · 2 Fotos“ (nie Namen oder Inhalte; das Symbol der Mitteilung ist auf iOS immer das der ntfy-App) mit Klick-Adresse `shortcuts://run-shortcut?name=Notch`. Auf dem iPhone die App ntfy, Thema von der Einrichtungsseite abonnieren. **NFC:** Automation „NFC“ › Kurzbefehl Notch ausführen = Antippen zum Austauschen.
   - Schnittstelle: `POST /k/<schlüssel>/senden?name=<datei>`, `POST /k/<schlüssel>/clip`, `GET /k/<schlüssel>/hallo`, `GET /k/<schlüssel>/abholen`, `GET /k/<schlüssel>/datei/<id>`, jeweils optional `akku` (0..1 oder 0..100) und `geraet`. Text oder Datei entscheidet die Notch (klein, UTF-8, kein Bild/PDF und ohne Dateiendung = Text).
   - Fester Schlüssel (128 Bit, `iphone-token.txt` im App-Ordner), **Entkoppeln** macht ihn ungültig. Prüfen: `cargo test --lib iphone`; ARP von Hand: `ARP_IP=<adresse> cargo test --lib arp_probe -- --ignored --nocapture`.

Sicherheit: Fremde Geräte werden jedes Mal gefragt (60 s, sonst abgelehnt); Uploads nur mit dem Einmal-Schlüssel aus der Zusage und von derselben Adresse; Dateinamen werden bereinigt (kein Pfad, keine reservierten Namen), nichts wird überschrieben; höchstens angekündigte Größe. Die Browser-Seite braucht den zufälligen Schlüssel aus dem QR-Code (30 Min gültig). Übertragen wird unverschlüsselt (HTTP) wie LocalSend mit ausgeschalteter Verschlüsselung – gedacht fürs Heim-WLAN, nicht für öffentliche Netze.

## Musik-Reaktion

Einstellungen › Anzeige › Musik › **Notch reagiert auf Musik**: *Aus* (Standard) / *Auto* / *Eigene*, dazu **Stärke** (50–200 %). Die schwarze Notch verformt sich an allen drei freien Seiten (oben angedockt: links, unten, rechts; seitlich entsprechend gedreht), unten in der Mitte der Bass, zu den Enden Mitten und Höhen bis ~2,7 kHz; an der Bildschirmkante läuft es auf 8 px aus. Maximaler Ausschlag bei 100 %: 26 px kompakt, 54 px aufgeklappt.

- **Stile, die sich überlagern:** *Zacken* (scharfe Spitzen, Kick drückt weiter raus), *Welle* (weich, träge, wandernder Schwung), *Puls* (die ganze Kontur wölbt sich im Takt). Pro Punkt zählt der größte gewichtete Ausschlag.
- **Auto:** Rust liefert Anteile je Stil (`music_style::weights`, weiche Übergänge aus Tempo, Taktdeutlichkeit, Schlagdichte, Bassanteil; z. B. Hardstyle ≈ Zacken 100 % + Puls 40 %), das Frontend blendet über ~1 s über. **Eigene:** Zacken, Welle, Puls frei kombinierbar (mindestens einer).
- **Abgriff** (`spectrum.rs`): WASAPI-Loopback genau des Ausgangs, auf dem der Player spielt (z. B. „Sonar - Media“), sonst des Standardausgangs. Läuft nur, wenn die Einstellung an ist, Musik spielt und kein Vollbild-Programm vorne ist; sonst ist der Abgriff geschlossen.
- **Analyse** (`music_style.rs`, getestet mit künstlichen Signalen): FFT (2048) alle 512 Samples, 24 logarithmische Bänder 40 Hz–16 kHz mit automatischer Aussteuerung; Schläge aus dem pegelrelativen spektralen Fluss (Bass doppelt) mit adaptiver Schwelle und Mindeststärke; Tempo aus der Autokorrelation der erkannten Schläge über 8 s (Halbtempo-Korrektur). Die Schwellen sind Startwerte; das Einstellungsfenster zeigt live Anteile, BPM und Takt.
- **Zeichnen** (`src/music-react.ts`): fensterfüllender Canvas hinter der Form, Kontur aus der echten Form (inkl. runder Ecken), `requestAnimationFrame` nur solange Daten kommen; bei „Bewegung reduzieren“ aus.

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
- `src-tauri/src/timer.rs` — Timer und Pomodoro als Activity (`src/pomodoro.ts`: Karte, Einstellen, Klang)
- `src/cards.ts` — Karten der aufgeklappten Notch: Reihenfolge per Ziehen, ausblenden, zurückholen
- `src-tauri/src/thumb.rs` — Vorschaubilder über die Windows-Shell
- `src-tauri/src/jarvis.rs` — PC-Zugriff des Sprachassistenten (App-Katalog, starten, Fenster, Seiten, Rückgängig); Plan und Messungen: `docs/jarvis.md`, `tools/jarvis/`
- `src-tauri/src/screen.rs`, `src-tauri/src/lol.rs` — Jarvis bedient Fenster (UI Automation, Texterkennung, Eingaben) und den LoL-Client
- `src/wake/` — „Hey Jarvis“ (openWakeWord nachgebaut, Modelle in `public/wakeword/`)
- `src-tauri/src/laya.rs`, `src/jarvis-live.ts`, `src/laya/` — Jarvis beim Sprechen: Modellpaket laden, Mitschrift, Laya (laya-ts eingebettet, `src/laya/vendor/README.md`)
- `src-tauri/src/update.rs` — Update-Prüfung und -Installation als Activity
- `src-tauri/src/sport.rs`, `src/sport.ts` — Live-Sport: Quellen, Ticker, Ballverlauf, Wurfbild, Anzeige
- `src/morph.ts` — Inhalte abgleichen statt neu bauen (kein Aufblinken); identisch in Arena
- `src-tauri/src/share.rs`, `share-page.html` — iPhone-Austausch: LocalSend-Protokoll, Browser-Seite, QR-Code
- `src-tauri/src/iphone.rs`, `iphone-page.html` — iPhone per Kurzbefehl: Teilen-Menü, Zwischenablage, Verbindungsanzeige (ARP)
- `src-tauri/src/gamepad.rs` — Controller-Kürzel (XInput)
- `src-tauri/src/shelf.rs`, `convert.rs` — Ablage und Konvertieren
- `src-tauri/src/win.rs` — Win32: Cursor, Vollbild, Fensterstile, Fenster nach vorn holen
- `src/main.ts`, `src/styles.css` — Form, Federanimation, Ansichten

## Lizenz

MIT, siehe [LICENSE](LICENSE).
