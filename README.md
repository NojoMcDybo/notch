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
| compact   | Musik läuft (bis 30 s nach Pause) oder Activity vorhanden |
| expanded  | Maus drauf, Datei wird draufgezogen, oder Activity mit `alert: true` (3,5 s) |

Bei Vollbild-Programmen auf dem Hauptmonitor fährt die Notch nach oben weg.

## Was man anklicken kann

**Jetzt** (Kreis-Symbol)
- Cover / Titel → holt den Player nach vorn
- Fortschrittsleiste → klicken oder ziehen = spulen
- Zurück / Play-Pause / Weiter
- Lautsprecher → stumm; Leiste daneben oder Mausrad über dem Player = Lautstärke
- Quellen-Chip (z. B. „Spotify ⇄") → zur nächsten Medienquelle, wenn mehrere laufen
- Activity-Zeile → öffnet die zugehörige App (`open`), Knöpfe darunter (`actions`), × oder Rechtsklick = weg

**Ablage** (Fach-Symbol)
- Dateien auf die Notch ziehen → landen hier (nur Verweise, keine Kopien; bleiben über Neustarts)
- Klick = auswählen, Doppelklick = öffnen, Ziehen = Datei wieder rausziehen, Rechtsklick = aus der Ablage
- Auswahl-Leiste: Öffnen, Im Ordner zeigen, Konvertieren, Entfernen
- Konvertieren: Bilder nativ (PNG, JPG, WEBP, GIF, BMP, TIFF, ICO, ≤ 1600 px). Audio/Video nur mit installiertem ffmpeg. Ergebnis landet neben dem Original, nie überschrieben.

**Timer** (Stoppuhr-Symbol)
- Vorwahl 1–60 min → läuft als Activity mit Pause, +1 min, Stopp; Ton und Aufklappen am Ende

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
  ]
}
```

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

- **Haze** (Blutzucker-Widget, `OneDrive\Projects\Haze`): `NotchBridge` in `Haze.cs` schickt Wert, Trend, Alter; Farbe nach Bereich; `alert` beim Wechsel in hoch/tief; `ttl` 180 s + Auffrischen jede Minute (stürzt Haze ab, verschwindet der Wert von selbst); beim Beenden wird der Eintrag gelöscht. Klick auf die Zeile holt Haze nach vorn. Aktuell **Demowerte** — Haze hat noch keine echte Datenquelle.

## Aufbau

- `src-tauri/src/lib.rs` — Fenster, Platzierung, Maus-Polling (Klick-durch außerhalb der Form), Tray
- `src-tauri/src/media.rs` — Now Playing über die Windows-Media-Session, Quellenwahl, Spulen
- `src-tauri/src/audio.rs` — Systemlautstärke (Core Audio)
- `src-tauri/src/activities.rs` — HTTP-Schnittstelle, Klicks, Events
- `src-tauri/src/open.rs` — öffnen / nach vorn holen / localhost-Rückmeldung
- `src-tauri/src/timer.rs` — Timer als Activity
- `src-tauri/src/shelf.rs`, `convert.rs` — Ablage und Konvertieren
- `src-tauri/src/win.rs` — Win32: Cursor, Vollbild, Fensterstile, Fenster nach vorn holen
- `src/main.ts`, `src/styles.css` — Form, Federanimation, Ansichten
