# Health-Export (iPhone → Notch → Analyseprogramm)

Die iPhone-App liest Apple Health (Zepp/Amazfit, Essens-App, iPhone selbst) und schickt neue Werte an die Notch am PC.
Die Notch legt sie als Dateien ab und stellt sie lokal über HTTP bereit. Beides nur auf diesem PC, nichts im Internet.

## Wo liegen die Daten

`Dokumente\Notch\Health\`

| Datei | Inhalt |
|---|---|
| `JJJJ-MM-TT.jsonl` | alle Werte eines Tages, eine JSON-Zeile je Wert |
| `_status.json` | Übersicht je Wertart: `count`, `sources` (Zahl je Quelle), `last` (Ende des jüngsten Werts); `updated` = Unix-Sekunden des letzten Abgleichs |

Die Dateien werden nur angehängt. Ein Wert mit bekannter `id` wird nie doppelt abgelegt.

## Ein Wert (eine Zeile)

```json
{"id":"6F1C…","t":"heartRate","s":"2026-11-18T08:00:00+01:00","e":"2026-11-18T08:00:00+01:00",
 "day":"2026-11-18","v":72.0,"u":"count/min","src":"Zepp","dev":"Amazfit GTR 4","m":{"HKMetadataKeyHeartRateMotionContext":1}}
```

| Feld | Bedeutung |
|---|---|
| `id` | UUID des Werts in Apple Health (eindeutig) |
| `t` | Wertart (Liste unten) |
| `s`, `e` | Beginn, Ende (ISO 8601 mit Zeitzone des iPhones) |
| `day` | Tag der Ablage (lokales Datum). Beginn-Tag; **Schlaf zählt zum Aufwach-Tag** (Ende) |
| `v`, `u` | Zahlenwert und Einheit (feste Einheit je Wertart, Tabelle unten) |
| `src` | Quelle (App/Gerät), z. B. `Zepp`, `iPhone von …`, Name der Essens-App |
| `dev` | Gerätename und Modell, falls Health es kennt |
| `m` | Metadaten der Quelle (z. B. Lebensmittelname, Mahlzeit), nur wenn vorhanden |

Sonderfälle:
- `t = "sleep"`: `v` = Phase (0 inBed, 1 asleep, 2 awake, 3 core, 4 deep, 5 rem), `vl` = Name der Phase, keine Einheit.
- `t = "workout"`: `v` = Dauer in Sekunden (`u = "s"`), `wt` = Sportart (`running`, `walking`, `cycling`, …, sonst `type<Nummer>`), `kcal` = aktive Energie, `dist` = Strecke in m (nur wenn vorhanden).
- `t = "mindfulSession"`: `v` = Dauer in Sekunden.
- Prozentwerte (`%`) sind 0–100, nicht 0–1.

## Wertarten und Einheiten

- Bewegung: `stepCount` (count), `distanceWalkingRunning`, `distanceCycling`, `distanceSwimming` (m), `flightsClimbed` (count), `activeEnergyBurned`, `basalEnergyBurned` (kcal), `exerciseTime`, `standTime` (min)
- Herz und Körper: `heartRate`, `restingHeartRate`, `walkingHeartRateAverage`, `heartRateRecoveryOneMinute`, `respiratoryRate` (count/min), `heartRateVariabilitySDNN` (ms), `oxygenSaturation` (%), `bodyTemperature`, `wristTemperature` (degC), `vo2Max` (ml/kg/min), `bloodPressureSystolic`, `bloodPressureDiastolic` (mmHg), `bloodGlucose` (mg/dL), `insulinDelivery` (IU), `bodyMass`, `leanBodyMass` (kg), `height`, `waistCircumference` (m), `bodyMassIndex` (kg/m2), `bodyFatPercentage` (%)
- Gang und Umwelt: `walkingSpeed` (m/s), `walkingStepLength`, `sixMinuteWalkDistance` (m), `walkingAsymmetry`, `walkingDoubleSupport`, `walkingSteadiness` (%), `environmentalAudioExposure`, `headphoneAudioExposure` (dBASPL)
- Ernährung: `dietaryEnergy` (kcal), `dietaryProtein`, `dietaryCarbohydrates`, `dietaryFat`, `dietaryFatSaturated`, `dietaryFatMonounsaturated`, `dietaryFatPolyunsaturated`, `dietarySugar`, `dietaryFiber` (g), `dietaryCholesterol`, `dietarySodium`, `dietaryPotassium`, `dietaryCalcium`, `dietaryIron`, `dietaryVitaminC`, `dietaryCaffeine` (mg), `dietaryVitaminD` (mcg), `dietaryWater` (mL), `alcoholicBeverages` (count)
- Sonstiges: `sleep`, `workout`, `mindfulSession`

Nicht vorhanden: was Zepp nur in der eigenen App behält (die iPhone-App zeigt unter „Was in Health ankommt“, welche Quelle welche Wertart liefert), Elektrokardiogramme und Herzrhythmus-Meldungen.

## Lokale Schnittstelle (nur 127.0.0.1)

Die Notch läuft auf `http://127.0.0.1:47800`. Nur lesen:

| Anfrage | Antwort |
|---|---|
| `GET /health-data/overview` | der Inhalt von `_status.json` |
| `GET /health-data/days` | `["2026-11-17","2026-11-18",…]` aufsteigend |
| `GET /health-data/records?from=2026-11-01&to=2026-11-18&type=heartRate` | Werte (JSON-Liste); `to` fehlt = nur `from`; `type` fehlt = alle Wertarten |

Zeitraum, Wertart und Tage sind die einzigen Filter. Größere Zeiträume mit allen Wertarten liefern viele Zeilen (Puls ungefähr minütlich): nach `type` getrennt abfragen oder die Dateien direkt lesen.

## Abgleich vom iPhone

- Die App gleicht beim Öffnen automatisch ab (höchstens alle 5 Minuten, abschaltbar) oder auf Knopfdruck. Beim ersten Mal die letzten 180 Tage, danach nur Neues oder Geändertes.
- Gesperrtes iPhone: Health ist dann nicht lesbar, der Abgleich wartet bis zum Entsperren. Im Hintergrund läuft nichts.
- Löschungen in Health werden nicht nachgezogen: ein einmal abgelegter Wert bleibt am PC. Korrigierte Werte kommen mit neuer `id` dazu; bei Bedarf nach `src`, `s` und `v` zusammenfassen.
- „Alles neu senden“ in der App ist ungefährlich, der PC erkennt die `id`.
