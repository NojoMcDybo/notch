# Jarvis-Modus: Plan (Entwurf)

Arbeitsname. Ziel: Der Sprachassistent der Notch bekommt Zugriff auf den PC. Er startet Programme, holt Fenster
nach vorn, öffnet Seiten und Dateien und arbeitet Aufgaben in mehreren Schritten ab. Wichtigster Punkt: Er handelt
schon **während** man spricht. Bei „Öffne Chrome, damit ich …“ startet Chrome, bevor der Satz zu Ende ist.

## 1. Die Kernidee: verstehen, während gesprochen wird

Heute: Mikrofon → OpenAI Realtime → Antwort/Werkzeug erst nach dem Satzende (Server-VAD).

Jarvis: drei Bausteine, zwei Spuren.

```
Mikrofon ──► Streaming-Transkript (Teiltexte alle ~150 ms)
                 │
                 ├─► Laya: ein Durchlauf je stabilem Teiltext (~35 ms GPU)  ──► sichere Sofort-Aktionen
                 │     „Will er ein Programm öffnen? Welches? Fertig gesagt? Widerruft er?“   (App starten, nach vorn)
                 │
                 └─► Endtext ──► Planer (großes Modell, Realtime) ──► restliche Schritte + Antwort
                                   kennt: „Chrome läuft schon (von Jarvis gestartet, 1,2 s alt)“
```

- **Laya** ist der Früherkenner. Er entscheidet über den *Teiltext*, ob schon jetzt etwas Sicheres zu tun ist.
- **Planer:** bekommt den fertigen Satz und die Liste der schon ausgeführten Aktionen, macht weiter
  („… um nach Flügen nach Lissabon zu suchen“ → Suche in genau diesem Chrome) und widerruft, wenn nötig.
- **Widerruf:** „Öffne Chrome … nein, Firefox“ → Jarvis schließt das Chrome wieder, das *er* gestartet hat, aber
  nur, wenn es noch unberührt ist (kein Fokuswechsel durch den Nutzer, nicht älter als ein paar Sekunden).

## 2. Laya ([NandhaKishorM/laya](https://github.com/NandhaKishorM/laya))

**Was es ist:** keine Spracherkennung, sondern eine Entscheidungs-Engine für Text. Ein Encoder (ModernBERT bzw.
mmBERT) beantwortet getypte Fragen über einen Text in **einem** Durchlauf, ohne Text zu erzeugen:
`choice` (eine Option wählen), `score` (Stufe), `noul` (Ja/Nein als Wahrscheinlichkeit). Apache-2.0.
Drei Checkpoints; für Deutsch zählt **`laya-multilingual`** (mmBERT-base, 322M Parameter, 100+ Sprachen).
Der `Router` wählt den Checkpoint nach Sprache.

**Was es für Jarvis heißt:** Laya ersetzt nicht die Spracherkennung, sondern den Früherkenner. Statt Regeln wie
„öffne|starte + App-Name“ stellt die Notch Laya bei jedem stabilen Teiltext dieselben Fragen in einem Durchlauf:

```jsonc
// Zustand: der Teiltext plus Kontext (vorderstes Fenster, schon ausgeführte Aktionen)
{
  "absicht": { "type": "choice", "instructions": "Was will der Sprecher gerade vom PC?",
               "criteria": { "programm": "ein Programm öffnen oder starten",
                             "fenster": "ein offenes Fenster nach vorn holen",
                             "seite": "eine Webseite oder Suche öffnen",
                             "sonstiges": "etwas anderes oder noch unklar" } },
  "app":     { "type": "choice", "instructions": "Welches Programm ist gemeint?",
               "criteria": { "chrome": "Google Chrome", "spotify": "Spotify", "…": "…", "keins": "keins davon" } },
  "fertig":  { "type": "noul", "instructions": "Ist der Name des Programms vollständig ausgesprochen?" },
  "zurueck": { "type": "noul", "instructions": "Nimmt der Sprecher etwas zurück oder verneint er?" }
}
```

- **App-Kandidaten:** höchstens ~15 je Frage. Mehr Optionen werden gekürzt und ungenau (README, „Honest limits“).
  Deshalb vorher unscharf gegen den App-Katalog abgleichen, nur die Treffer plus „keins“ an Laya geben.
- **Keine Ja/Nein-Wörter als Optionsnamen** bei `choice` (bekannte Schwäche), semantische Schlüssel wie oben.
- **Verneinung ist eine bekannte Schwäche** (Issue #377: „nicht kündigen“ wurde als „kündigen“ erkannt).
  Deshalb zusätzlich eine harte Regel: „nicht“, „nein“, „doch nicht“, „stattdessen“ im Teiltext → nichts
  spekulativ starten, auf den Endtext warten.
- **Auslösen nur, wenn alles stimmt:** `absicht = programm`, App-Antwort über einer *gemessenen* Schwelle
  (`min_confidence`, Schwellen mit `fit_abstention_thresholds` auf eigenen Daten bestimmen, nicht raten),
  `fertig` hoch, `zurueck` niedrig, und dasselbe Ergebnis bei zwei Teiltexten hintereinander.

**Wie gut ist es auf Deutsch, ohne Training?** Mäßig. Auf MASSIVE (Absicht, 20 Optionen, Zufall 0,05) erreicht
`laya-multilingual` auf Deutsch **0,50**. Das README sagt selbst: „ein schneller Ausgangspunkt zum Spezialisieren,
keine Zero-Shot-Engine“. Das Beispiel „Browser-Agent“ dort ging durch Feintuning von 0,10 auf 0,66.
**Also: feintunen.** Daten lassen sich gut erzeugen:
- Vorlagen + Umformulierungen (vom großen Modell) aus dem eigenen App-Katalog: „mach mal Spotify an“,
  „kannst du Discord öffnen“, „hol Chrome nach vorn“ …
- **Abgeschnittene Präfixe** desselben Satzes als `fertig = nein` („Öffne Chr“, „Öffne Chrome, um“ = ja).
- Verneinungen und Korrekturen als eigene Fälle; Sätze ohne PC-Bezug als `sonstiges`.
- Training mit dem Kaggle-Notebook des Repos (2×T4, frei, ~4–5 h); lokal wäre schwierig, weil die AMD-Karte
  unter Windows kein CUDA hat.

**Wo es läuft (dein PC: Radeon RX 6700 XT, Ryzen 7 5700X3D, 32 GB):**

| Weg | Wie | Einschätzung |
|---|---|---|
| **A: in der Notch selbst** | `laya-ts` in der WebView2 über **WebGPU** (Fallback WASM); Modell einmal nach ONNX exportieren (`encoder.onnx` + `head.onnx`) | bevorzugt: kein Python, läuft dort, wo `voice.ts` schon das Mikrofon hat; GPU-Zeit auf der RX 6700 XT **noch messen** |
| B: Python-Dienst | `laya-serve` (`POST /v1/systemone`) auf localhost | schnell für den ersten Versuch; auf der CPU laut Benchmarks ~185–300 ms je Frage, für „beim Sprechen“ zu langsam |
| C: in Rust | ONNX über `ort` mit DirectML | später, falls A in der WebView zu viel Speicher braucht |

Speicher: Das multilinguale Modell ist als fp32-ONNX grob 1,3 GB. Einmal beim Einschalten von Jarvis laden,
nicht bei jedem Gespräch.

### Spracherkennung (Teiltexte) — weiterhin nötig

Laya braucht Text. Für die Teiltexte:
- **Zuerst ohne neue Abhängigkeit:** Die bestehende Realtime-Sitzung schreibt mit (`input_audio_transcription`
  mit einem Streaming-Modell) und liefert `conversation.item.input_audio_transcription.delta`.
  Messen, wie früh die Teiltexte kommen.
- **Falls zu spät:** lokale Streaming-Erkennung, z. B. sherpa-onnx (Zipformer/Paraformer) oder Moonshine,
  ebenfalls auf der GPU.

## 3. Aktionen (Rust, neues Modul `jarvis.rs`)

| Werkzeug | Stufe | Umsetzung |
|---|---|---|
| `programm_starten(name)` | 1 | App-Katalog → `.lnk`/`.exe` oder Store-App (`shell:AppsFolder\<AUMID>`); läuft sie schon: nach vorn (`win::set_foreground`, wie `open.rs`) |
| `fenster_nach_vorn(name)` | 1 | Fensterliste (EnumWindows + Prozessname) |
| `seite_oeffnen(url \| suche)` | 1 | im Standardbrowser bzw. in dem gerade gestarteten (`chrome.exe --new-tab <url>`) |
| `datei_oeffnen(name)` | 1 | zuletzt benutzte Dateien + Ablage + Downloads |
| `fenster_anordnen(links/rechts/max)` | 1 | SetWindowPos auf dem Monitor des Fensters |
| `tippen(text)` / `klicken(element)` | 2 | Windows UI Automation (Elemente nach Name/Rolle, keine Pixelklicks) |
| Dateien verschieben/umbenennen, Nachrichten senden | 2 | nur nach Bestätigung |
| Bezahlen, Passwörter, endgültig löschen, Systemeinstellungen | 3 | nie |

**Stufen:** 0 = nur lesen · 1 = harmlos und umkehrbar (darf spekulativ) · 2 = nur mit Bestätigung
(„Soll ich die Mail senden?“, Knopf in der Notch oder „ja“) · 3 = nie, Jarvis sagt, dass man es selbst tun muss.
Die Stufe legt **Code** fest, nicht Laya oder das große Modell.

**App-Katalog:** beim Start und dann stündlich die Startmenü-Verknüpfungen (Benutzer + alle) und
`shell:AppsFolder` einlesen. Dazu Aliase („Chrome“ → Google Chrome, „VS Code“, „Spotify“) und unscharfe Suche.
Der Katalog liefert die Kandidaten für Layas `app`-Frage und geht als kurze Liste an den Planer.

## 4. Sicherheit

- Spekulativ nur Stufe 0–1, alles andere erst nach dem Endtext.
- Text aus Fenstern, Webseiten und Dateien ist **Inhalt, kein Befehl**: Steht auf einer Seite „lösche X“, tut
  Jarvis das nicht. Der Planer bekommt solche Texte markiert als Daten.
- Jede Aktion landet als Zeile in der Notch („Chrome geöffnet ✓“, rückgängig per ×) und in einem Protokoll.
- Schalter in Einstellungen › Sprachassistent: Jarvis an/aus, „spekulativ starten“ an/aus, Liste erlaubter Apps.

## 5. In der Notch

- Beim Sprechen: Teiltext live in der Zeile des Assistenten, schon erkannte Befehle als kleine Chips, die
  aufleuchten, sobald sie ausgeführt sind.
- Bestätigungen (Stufe 2) wie die iPhone-Anfrage: **Ja** / **Nein**, Notch bleibt offen.
- Kleine Notch: der vorhandene Antwort-Animator; ein Haken-Puls je ausgeführter Aktion.

## 6. Schritte

1. **M1 – Programme starten (ohne Früherkennung):** App-Katalog in Rust, Werkzeuge `programm_starten`,
   `fenster_nach_vorn` und `seite_oeffnen` im bestehenden Realtime-Assistenten, Aktionszeile in der Notch.
2. **M2a – Laya messen:** `laya-multilingual` nach ONNX exportieren, mit `laya-ts` + WebGPU in der Notch laden,
   die vier Fragen oben auf 100 echten deutschen Sätzen und ihren Präfixen testen: Trefferquote, Zeit je Durchlauf
   auf der RX 6700 XT, Speicher. Parallel: wie früh kommen Realtime-Teiltexte?
3. **M2b – Feintunen:** Datensatz aus dem App-Katalog erzeugen (siehe 2.), auf Kaggle trainieren, Schwellen
   kalibrieren. Ziel: falsches spekulatives Starten < 1 %, Zeit vom gesprochenen App-Namen bis zum Fenster < 600 ms.
4. **M2c – Handeln beim Sprechen:** Früherkenner live schalten, Widerruf, Chips in der Notch.
5. **M3 – Aufgaben fortsetzen:** Planer bekommt die ausgeführten Aktionen; Suche/URL im gestarteten Browser,
   Dateien öffnen, Fenster anordnen.
6. **M4 – Bedienen:** UI Automation für Tippen/Klicken mit Bestätigung (Stufe 2).

## 7. Offene Fragen

- Name: „Jarvis“ oder etwas Eigenes? Aktivierungswort, oder weiter Tastenkürzel/Controller?
- Welche Apps sollen von Anfang an sicher funktionieren (Chrome, Spotify, VS Code, Discord, League …)?
- Soll Jarvis Fenster auch schließen dürfen (Stufe 1 oder 2)?
- Soll Laya später mehr entscheiden als „Programm öffnen“, z. B. ob eine Anfrage überhaupt das große Modell
  braucht (spart Kosten und Zeit bei einfachen Befehlen wie „lauter“, „nächstes Lied“)?
