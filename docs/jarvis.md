# Jarvis-Modus: Plan (Entwurf)

Arbeitsname. Ziel: Der Sprachassistent der Notch bekommt Zugriff auf den PC. Er startet Programme, holt Fenster
nach vorn, öffnet Seiten und Dateien und arbeitet Aufgaben in mehreren Schritten ab. Wichtigster Punkt: Er handelt
schon **während** man spricht. Bei „Öffne Chrome, damit ich …“ startet Chrome, bevor der Satz zu Ende ist.

## Stand (13.10.2026)

- **M1 umgesetzt** (`src-tauri/src/jarvis.rs`): App-Katalog aus `shell:AppsFolder` (auf diesem PC 229 Programme),
  Werkzeuge `programm_starten`, `fenster_nach_vorn`, `seite_oeffnen` im Sprachassistenten, kurze Bestätigungszeile
  in der Notch. Noch ohne Rückgängig-× und ohne Schalter in den Einstellungen.
- **M2a gemessen** (Abschnitt 2a): Laya erkennt die App sicher, aber Absicht, „fertig gesagt“ und Verneinung
  ohne Feintuning nicht. Entwurf deshalb angepasst: App, „fertig“ und Verneinung über Regeln + Katalog,
  Laya nur noch für die Absicht — und erst nach Feintuning (M2b).
- **Teiltexte:** nur `gpt-live-transcribe` liefert Text *während* des Sprechens, und laut Doku nur in einer
  eigenen Transkriptions-Sitzung (siehe 2b). Entschieden: diesen Weg nehmen (Deutsch ist wichtig; App-Namen als
  `keywords`), keine lokale Erkennung.
- **M2b erledigt (13.10.2026):** Trainingssatz `tools/jarvis/make_train.py` (3121 Sätze, nur die Frage `absicht`),
  Notebook `tools/jarvis/make_notebook.py` (aus dem Laya-Notebook, mehrsprachiges Modell), privater Kaggle-Datensatz
  `jarvis-absicht-de`, Training auf 2×T4 in ~10 Min. Ergebnis siehe 2c. Modell lokal unter
  `D:\Dev\_downloads\laya-build\model-ml-ft` (ONNX) bzw. `model-ml-ft-int4`, noch nicht veröffentlicht.
- **M2c gebaut (13.10.2026), wartet auf den ersten echten Test:** Mitschrift beim Sprechen, Katalog-Regel, Laya im
  Worker auf der GPU, Vorab-Start, Rückgängig, Schalter in den Einstellungen. Ergebnisse und Abweichungen in 2d.
  Das Modellpaket liegt lokal fertig (`D:\Dev\_downloads\laya-build\pkg-jarvis`), ist aber **noch nicht** als
  Release `laya-jarvis-1` hochgeladen — bis dahin meldet der Schalter einen Download-Fehler.
- **Sprachassistent:** Standard jetzt `gpt-realtime-2.1-mini`; Claude (Haiku/Sonnet/Opus 5.5) als Alternative über
  eine Kette Sprache→Text→Claude→Sprache (`src/voice-claude.ts`). Beide nutzen dieselben PC-Werkzeuge aus M1.

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

### 2a. Messung M2a (13.10.2026, `tools/jarvis/`)

51 deutsche Sätze (`tools/jarvis/saetze.json`: Befehle, Wechseln, Suchen, Fragen ohne PC-Bezug, Verneinungen,
abgeschnittene Anfänge), die vier Fragen aus 2. in einem Durchlauf, `laya-multilingual` ohne Feintuning
(`node tools/jarvis/eval.mjs <modellordner>`):

| | fp32 (1,3 GB) | INT4 (250 MB) |
|---|---|---|
| App richtig | 63 % — bei genanntem Namen fast immer, Sicherheit 0,99–1,00 | 65 % |
| Absicht richtig | 59 % (verwechselt Programm/Fenster/Seite) | 67 % |
| „fertig gesagt“ | 14 % — sagt fast immer „nein“ | 12 % |
| Verneinung erkannt | 0 von 5 | 0 von 5 |
| würde richtig starten | 0 von 17 | 0 von 17, 2 Fehlstarts |

Andere Frageformen helfen nicht: Ja/Nein „Fordert er auf, ein Programm zu öffnen?“ auf Deutsch 21 Fehlstarts
(„Ich hab gestern Chrome deinstalliert“ = ja), auf Englisch 12 verpasste Befehle.

**Zeit je Durchlauf** (Ryzen 7 5700X3D / Radeon RX 6700 XT; Encoder, der teure Teil):

| Laufzeit | 64 Tokens | 160 Tokens (typisch) | 256 Tokens |
|---|---|---|---|
| CPU nativ (onnxruntime-node), ganzes Laya, fp32 / INT4 | – | 236 / 302 ms | – |
| WASM, 8 Threads, INT4 | 164 ms | 317 ms | 487 ms |
| **WebGPU, INT4** | 146 ms | **146 ms** | 200 ms |
| WebGPU, fp16 | 221 ms | 236 ms | 269 ms |

- WebGPU läuft nur mit `graphOptimizationLevel: "basic"`; mit voller Optimierung bricht ONNX Runtime ab
  („SkipLayerNormalization: Beta must be 1D“).
- Die Zeit hängt kaum an der Länge: fester Aufwand, vermutlich rechnet ONNX Runtime einzelne Knoten nicht auf der
  GPU und kopiert hin und her. Nachsehen, welche (ausführliches Log), bevor man weiter optimiert.
- Gemessen im Chromium des Browser-Bereichs (`tools/jarvis/webgpu.html`, `serve-bench.mjs`), AMD RDNA 2, kein
  Software-Ersatz. In der WebView2 der Notch noch einmal prüfen.

**Folgerung — Entwurf angepasst:**
- **Regeln statt Laya für App, „fertig“, Verneinung:** Der Katalogabgleich (`jarvis::find`) erkennt die App nur
  bei ganzem Wort — „Öffne Chr“ trifft nichts, „Öffne Chrome“ trifft. Damit ist „fertig gesagt“ erledigt.
  Verneinung über feste Wörter (nicht, nein, doch nicht, stattdessen, lass mal, ich meine, äh … ich meine).
- **Laya nur für die Absicht:** eine einzige Frage („Befehl, das Programm jetzt zu öffnen“ vs. „nur erwähnt“ vs.
  „Fenster“ vs. „Seite“), und nur dann, wenn der Katalog im Teiltext eine App findet. Das spart Durchläufe:
  ~150 ms je Treffer statt je Teiltext.
- **Ohne Feintuning nicht live schalten.** Bis M2b fertig ist, entscheidet das große Modell nach dem Satzende
  (M1, funktioniert schon).

### 2c. Ergebnis M2b (Feintuning, nur die Frage `absicht`)

Gleiche 51 Testsätze (nie im Training), `ABSICHT=1 node tools/jarvis/eval.mjs <modell>`:

| | richtig | „starten“, obwohl nicht gemeint | CPU je Satz | GPU (T4) | Größe |
|---|---|---|---|---|---|
| vorher | 28/51 (55 %) | 12 | 88 ms | 33 ms | 1,3 GB |
| **nachher, fp32** | **46/51 (90 %)** | 4 | 82 ms | 35 ms | 1,3 GB |
| nachher, INT4 | 46/51 (90 %) | 5 | 96 ms | – | 240 MB |

Die Fehlstarts im Einzelnen: „Kannst du bitte“, „Mach mal“, „Starte einen Pomodoro“ (kein App-Name → die
Katalog-Regel hält sie auf), „Öffne Steam, äh, ich meine Epic“ (→ **„ich meine“ in die Korrekturwörter**),
bei INT4 zusätzlich „Öffne YouTube“ (würde die installierte App *YouTube Music* treffen statt der Seite).
Mit den Regeln: fp32 0 Fehlstarts, INT4 1. Nächster Datensatz: mehr Seiten mit gleichnamiger App (YouTube,
Spotify Web, Discord im Browser), Füllwörter („äh“, „ich meine“), längere Sätze.

### 2d. Umsetzung M2c (13.10.2026)

**Ablauf je Teiltext** (`src/jarvis-live.ts`, Klasse `Turn` ohne Seiteneffekte):
1. Korrekturwort (nicht, nein, nee, doch nicht, stattdessen, lass mal, lieber, ich meine, sondern, warte, halt) →
   in diesem Satz nichts mehr vorab; schon vorab Gestartetes schließen (`jarvis_undo`: WM_CLOSE an die Fenster des
   Programms, nur wenn Jarvis es gestartet hat und der Start < 15 s her ist).
2. `jarvis_hint` (Rust, `jarvis::mention`): Programm *ganz* genannt — Katalogname, Name ohne Hersteller („Chrome“
   für „Google Chrome“) oder Alias; Allerweltswörter („Editor“, „Einstellungen“, „Musik“ …) zählen nicht.
3. Laya, nur die trainierte Frage `absicht` (Wortlaut in `src/laya/worker.ts`).
4. „starten“/„wechseln“ ab 0,8 sofort, ab 0,6 erst beim zweiten gleichen Teiltext. Ein Vorab-Schritt je Satz.

Der Assistent bekommt danach den ganzen Satz wie bisher. Startet er dasselbe Programm, merkt `open_app`, dass es
frisch gestartet ist (kein zweites Fenster). Die Mitschrift ist eine eigene WebRTC-Sitzung (`voice_token` mit
`transcribe`), Satzende = Server-VAD bzw. Loslassen bzw. Satzende im Claude-Weg (`input_audio_buffer.commit`);
App-Namen gehen als `keywords` mit (`jarvis_keywords`, höchstens 100).

**Schwelle** (`node --experimental-transform-types tools/jarvis/early-eval.mjs <modell>`: jeder Satz Wort für Wort,
dazu der Endtext): vorab richtig 21/22, davon 8 vor dem letzten Wort, ein „Fehlstart“ („Starte League of …“ →
League of Legends, gemeint war es auch), 3 zurückgenommen („Öffne Chrome, nein, …“, „Starte Discord, ach nee …“,
„Öffne Steam, äh, ich meine Epic“). Mit 0,9 kämen 4 Starts erst mit dem Endtext, mit 0,7 wird öfter zurückgenommen.
Nur 51 Sätze — mit echten Mitschriften nachmessen.

**WebGPU-Fehler gefunden:** In M2a war nur die Zeit gemessen, nicht das Ergebnis. Mit dem INT4-Modell rechnet der
Encoder auf WebGPU (onnxruntime-web 1.30, JSEP) **falsch** (Abweichung bis 23 bei Werten um 1,3, Laya rät dann nur
noch). Ursache: die quantisierte Einbettungstabelle (`GatherBlockQuantized`); `MatMulNBits` stimmt exakt. Lösung
(`tools/jarvis/quantize.py`): Tabelle als int8 je Zeile aus Grundbausteinen (Gather → Cast → Mul), Rest INT4.
Gleiche Genauigkeit (46/51), Encoder 269 MB. Das Paket `onnxruntime-web/webgpu` (native WebGPU, asyncify-WASM) kennt
`GatherBlockQuantized` gar nicht; die Notch nimmt `onnxruntime-web/all` (JSEP) wie in M2a.

**Zeit im Browser** (Chromium wie WebView2, RX 6700 XT; `tools/jarvis/worker-test.html`): Laden + Aufwärmen ~4 s
(einmal, das Modell bleibt geladen, solange der Schalter an ist), je Teiltext ~210 ms: Encoder GPU ~120 ms, Head
WASM ~90 ms. Der Head liefe auf der GPU in ~25 ms, weicht dort aber leicht ab (0,80 statt 0,84) — erst umstellen,
wenn die Schwellen mit GPU-Werten neu gemessen sind.

### 2b. Teiltexte (Spracherkennung beim Sprechen)

Laya braucht Text. Stand der OpenAI-Doku (Realtime Transcription):
- **`gpt-live-transcribe`** liefert `conversation.item.input_audio_transcription.delta` *während* des Sprechens,
  Verzögerung einstellbar (`delay`: minimal … xhigh). Dokumentiert nur für Sitzungen vom Typ `transcription`,
  ohne Server-VAD (selbst `input_audio_buffer.commit` senden). Praktisch: `keywords` nimmt Begriffe auf — dort
  die App-Namen aus dem Katalog mitgeben.
- `gpt-transcribe` (auch im Sprachassistenten nutzbar) liefert erst nach dem Satzende — für Jarvis zu spät.
- Also: **zweite, parallele Sitzung** nur zum Mitschreiben, dasselbe Mikrofon, Commit, wenn der Assistent
  „speech_stopped“ meldet. Kostet zusätzlich je Minute. Alternative ohne Kosten: lokale Streaming-Erkennung
  (sherpa-onnx, Moonshine) — aufwendiger, Deutsch-Qualität offen.

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
2. ~~**M2a – Laya messen**~~ ✓ (2a): App gut, Absicht/fertig/Verneinung ohne Feintuning unbrauchbar;
   WebGPU INT4 ~150 ms.
3. ~~**M2b – Feintunen (nur Absicht)**~~ ✓ (2c): 90 % statt 55 %, mit Regeln keine Fehlstarts (fp32).
   Ursprünglicher Plan: Datensatz aus dem App-Katalog erzeugen (Befehle, bloße Erwähnungen,
   Fenster, Seiten, Verneinungen; Vorlagen + Umformulierungen vom großen Modell, einige Tausend Sätze), auf Kaggle
   trainieren (Notebook im Laya-Repo, kostenloses Konto nötig), Schwellen kalibrieren, mit `tools/jarvis/eval.mjs`
   gegenprüfen. Ziel: Fehlstarts < 1 %. Parallel: WebGPU-Rückfälle in ONNX Runtime finden (Ziel < 60 ms).
4. **M2c – Handeln beim Sprechen** (gebaut, siehe 2d; offen: Paket hochladen, echter Test, Zeit Name → Fenster messen): zweite Sitzung mit `gpt-live-transcribe` (2b), Katalog + Regeln + Laya auf
   jedem Teiltext mit App-Treffer, Modellpaket wie in Arena laden (`arena/src-tauri/src/laya.rs`, Worker aus
   `arena/src/laya/`), Widerruf, Chips in der Notch. Ziel: App-Name gesprochen → Fenster < 600 ms.
5. **M3 – Aufgaben fortsetzen:** Planer bekommt die ausgeführten Aktionen; Suche/URL im gestarteten Browser,
   Dateien öffnen, Fenster anordnen.
6. **M4 – Bedienen:** UI Automation für Tippen/Klicken mit Bestätigung (Stufe 2).

## 7. Offene Fragen

- Name: „Jarvis“ oder etwas Eigenes? Aktivierungswort, oder weiter Tastenkürzel/Controller?
- Welche Apps sollen von Anfang an sicher funktionieren (Chrome, Spotify, VS Code, Discord, League …)?
- Soll Jarvis Fenster auch schließen dürfen (Stufe 1 oder 2)?
- Kaggle-Konto für das Feintunen (M2b)? Und ist die zweite, kostenpflichtige Mitschreib-Sitzung in Ordnung,
  oder lieber lokale Spracherkennung?
- Soll Laya später mehr entscheiden als „Programm öffnen“, z. B. ob eine Anfrage überhaupt das große Modell
  braucht (spart Kosten und Zeit bei einfachen Befehlen wie „lauter“, „nächstes Lied“)?
