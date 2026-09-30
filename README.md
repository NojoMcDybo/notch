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
| expanded  | Maus drauf, oder Activity mit `alert: true` (3,5 s) |

Bei Vollbild-Programmen auf dem Hauptmonitor fährt die Notch nach oben weg.

## Live Activities — so hängt sich jede App an

Die Notch hört auf `http://127.0.0.1:47800`.

| Methode | Pfad              | Zweck |
|---------|-------------------|-------|
| POST    | `/activity`       | anlegen / ersetzen (gleiche `id`) |
| DELETE  | `/activity/<id>`  | entfernen |
| GET     | `/activities`     | aktuelle Liste |
| GET     | `/health`         | `ok` |

```json
{
  "id": "blutzucker",        // Pflicht, eindeutig
  "title": "Blutzucker",     // Pflicht
  "app": "Glukose",
  "subtitle": "stabil · vor 2 min",
  "value": "112",
  "unit": "mg/dL",
  "icon": "🩸",              // Emoji, Text oder data:-URL
  "color": "#5ee38a",
  "progress": 0.62,          // 0..1 -> Balken
  "ttl": 300,                // Sekunden bis Auto-Entfernen
  "priority": 1,             // höchste landet im kompakten Zustand
  "alert": false             // true = Notch klappt kurz auf
}
```

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
Rechtsklick auf eine Activity in der aufgeklappten Notch wischt sie weg.

## Aufbau

- `src-tauri/src/lib.rs` — Fenster, Platzierung, Maus-Polling (Klick-durch außerhalb der Form), Tray
- `src-tauri/src/media.rs` — Now Playing über die Windows-Media-Session (jede App, die Windows Mediensteuerung meldet)
- `src-tauri/src/activities.rs` — HTTP-Schnittstelle für Live Activities
- `src-tauri/src/win.rs` — Win32: Cursor, Vollbild-Erkennung, Fensterstile (kein Fokus-Klau, nicht in Alt-Tab)
- `src/main.ts`, `src/styles.css` — Form, Federanimation, Inhalte
