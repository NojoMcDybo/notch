# iPhone als Webcam, Anrufe und Nachrichten (Windows)

Drei Erweiterungen der iPhone-Brücke (`iphone.rs`), alle ohne App auf dem iPhone.

## Nachrichten und Anrufe: Bluetooth (`ancs.rs`)

Der PC verhält sich wie eine Uhr am iPhone und liest dessen Benachrichtigungen über den Apple Notification Center Service
(ANCS, Bluetooth LE). Angezeigt werden Anrufe (mit Annehmen/Ablehnen), verpasste Anrufe und Nachrichten bekannter Messenger
(Liste `MESSENGERS`). Gesprochen wird am iPhone, der Ton läuft nicht über den PC.

- **Einrichten (einmalig):** Windows › Bluetooth › Gerät hinzufügen › iPhone koppeln; am iPhone Bluetooth › (i) neben dem PC
  › „Benachrichtigungen anzeigen“ erlauben; in der Notch Einstellungen › iPhone › „Anrufe und Nachrichten vom iPhone“ an.
- **Aus, bis eingeschaltet** (Nachrichtentexte wären sonst in Bildschirmfreigaben sichtbar).
- **Stand:** Protokoll (Ereignisse, Antworten in Stücken, Auswahl) ist mit Tests abgedeckt. Die Bluetooth-Anbindung ist gebaut,
  aber ohne gekoppeltes iPhone **nicht am Gerät geprüft**. Offene Fragen: ob das iPhone dem PC die ANCS-Merkmale anbietet, wenn
  der PC als Zentrale verbindet, und ob die Annehmen-Aktion bei Drittanbieter-Anrufen (WhatsApp) wirkt.
- **Ersatz ohne Bluetooth:** `POST /k/<schlüssel>/nachricht?von=Name` (Text im Body); iOS-Automation „Wenn ich eine Nachricht
  bekomme“ › „Inhalte von URL abrufen“. Steht auf der Einrichtungsseite unter „Nachrichten am PC“.

## iPhone als Webcam (`cam.rs`, `vcam.rs`, `vcam/`)

Safari auf dem iPhone öffnet `https://<PC-Adresse>:53443/cam/<schlüssel>/` (QR-Code in Einstellungen › iPhone) und schickt je
Bild ein JPEG (1280×720, ~30/s) an `POST …/frame`. Die Notch decodiert, wandelt nach NV12 und schreibt es in
`%PUBLIC%\Notch\vcam.frame`. Daraus liest die virtuelle Kamera **„Notch iPhone“** (Media Foundation, `MFCreateVirtualCamera`,
Windows 11); Teams, Zoom, Chrome und die Kamera-App sehen eine normale Webcam.

- **Aussetzer:** Bleibt ein neues Bild aus (WLAN-Hänger, die Quelle liest gerade, während die Notch schreibt), zeigt die Kamera
  das letzte ganze Bild weiter. Erst nach 5 s ohne neues Bild (`HOLD_MS` in `vcam/src/lib.rs`) kommt ein schwarzes Bild mit dem
  Notch-Schriftzug (`vcam/src/logo.bin`, erzeugt mit `node tools/vcam-logo.mjs` aus `icons/source.png`). Früher brachte jedes
  Lesen, das die Notch mitten beim Schreiben erwischte, sofort ein dunkles Platzhalterbild: das waren die einzelnen schwarzen
  Bilder im laufenden Strom. Die Quelle liest jetzt nur noch, wenn ein neues Bild da ist, in einen eigenen Puffer, und versucht
  es bei einem Zusammenstoß nach 2 ms erneut.
- **Mehrere Geräte, eines live:** Mehrere Geräte dürfen gleichzeitig senden (iPhone und iPad, Safari-Seite und App,
  Bildschirm). Windows sieht weiter eine Kamera; sie zeigt nur das **Live-Gerät**. Wer zum ersten Mal sendet, wird live;
  verstummt das Live-Gerät 5 s, übernimmt ein anderes sendendes. Erkannt wird ein Gerät an `?c=<Kennung>&n=<Name>`
  (Seite und App, je Start neu), sonst an User-Agent und Adresse. Antworten tragen `X-Notch-Live` (0/1) und
  `X-Notch-Rev` (Stand der Einstellungen). Ton der Bildschirmübertragung nur vom Live-Gerät.
- **Karte in der Notch** (`src/camcard.ts`): Vorschau je Gerät (Live-Gerät grün, Klick schaltet live), darunter die
  Einstellungen des Live-Geräts. Befehle `cam_sources`, `cam_live`, `cam_config`. Die Bilder kommen über das Protokoll
  `camprev://localhost/<id>?after=<seq>` (Windows: `http://camprev.localhost/…`, `cam.rs: preview_protocol`): das neueste
  JPEG unverändert, die Anfrage wartet bis zu 0,5 s auf ein neues. Live-Gerät im Takt des Geräts (bis 60/s), andere
  12/s; nur, solange die Notch aufgeklappt ist.
- **Einstellungen beidseitig:** Das Gerät meldet Einstellungen und Fähigkeiten (`POST …/settings`, `{settings, caps}`);
  in der Notch geänderte erhöhen den Stand. Das Gerät nennt seinen Stand bei jedem Bild (`?r=`); ist er älter, kommen
  die Einstellungen gleich mit der Antwort (200 mit JSON, sonst 204). Schlüssel: `front`, `lens` (`ultra`/`wide`/`tele`),
  `quality` (0–2), `rate` (15/30/60), `mirror`, `zoom`, `torch`; Fähigkeiten `lenses`, `zoom`, `torch`, `maxRate`.
- **Bildrate (bis 60/s):** Bis zu drei Bilder sind gleichzeitig unterwegs (`?s=<Nummer>`, ein überholtes verwirft die
  Notch), sonst bestimmte die WLAN-Laufzeit die Rate. Die Notch antwortet sofort und rechnet im Hintergrund um
  (`queue_push`, nur das neueste wartende Bild). Die virtuelle Kamera bietet 30 und 60 Bilder/s an; das öffnende
  Programm wählt. Die Seite taktet mit `requestVideoFrameCallback` und kodiert in drei Workern (OffscreenCanvas,
  `convertToBlob`, Safari ab 16.4; im Haupt-Thread nur `createImageBitmap`), sonst wie bisher mit `toBlob`; die App wählt für 60/s ein passendes Kameraformat
  und verkleinert auf 1280×720. Zoom aus der Notch gleitet (Seite: ~180 ms, App: `ramp(toVideoZoomFactor:)`).
- **Hintergrund:** Die Browserseite sendet nicht, solange Safari sie in den Hintergrund legt (dann kämen nur schwarze
  Bilder; so entstand früher das regelmäßige Blinken bei zwei Instanzen).
- **Einstellungen auf der Browserseite** (wie in der App, gespeichert im Browser): Front-/Rückkamera, Licht, Spiegeln,
  Objektiv (Ultraweit/Weit/Tele, erkannt am Gerätenamen), Zoom, Qualität (JPEG 0,4/0,6/0,8) und Bildrate (15/30/60). Zoom und
  Licht nur, wenn Safari sie anbietet (`getCapabilities`, ab iOS 17). Ein Bild, das länger als 3 s nicht angenommen wird,
  bricht die Seite ab und schickt das nächste (vorher konnte ein hängender Upload den Strom anhalten).

- **HTTPS:** Safari erlaubt die Kamera nur über https. Die Notch erzeugt ein eigenes Zertifikat (`rcgen`, gültig für die LAN-Adresse
  und `<PC>.local`); Safari fragt beim ersten Öffnen einmal „Verbindung nicht privat“ › Details › Trotzdem besuchen. Ändert sich
  die LAN-Adresse, entsteht ein neues Zertifikat.
- **Einmalige Einrichtung mit Administratorrechten:** Der Windows-Kameradienst (Frame-Server, LocalService) lädt die Quelle, also muss
  sie systemweit eingetragen sein (HKCU geht nicht, geprüft). Beim ersten Einschalten startet die Notch sich selbst mit
  `--vcam-setup` und Windows fragt nach Administratorrechten. Der Schritt legt `C:\Program Files\Notch\vcam\notch_vcam-r<Kennung>.dll` ab
  (nur für Administratoren beschreibbar), trägt `HKLM\Software\Classes\CLSID\{7C0C9B3E-5A1D-4F6B-9E47-2D6A1B8C3F10}\InprocServer32`
  ein und gibt `%PUBLIC%\Notch` für den Dienst und Benutzer frei. Die Kennung ist eine Prüfsumme der DLL (`build.rs`);
  ändert sich die Quelle, fragt Windows noch einmal. **Sicherheit:** die Abfrage startet die `notch.exe` aus dem Benutzerordner
  (für Benutzerprozesse beschreibbar); wer sie austauscht, bekommt bei Bestätigung Administratorrechte. Bekannte Grenze,
  nicht behoben (Abhilfe: Notch signieren oder per Installer nach Programme legen).
- **Quelle (`vcam/`):** eigenes Paket, wird von `build.rs` mit eigenem Zielordner gebaut und in `notch.exe` eingebettet.
  Im Dienst gibt es kein Protokoll (nur mit `--features log` beim Fehlersuchen) und die Bilddatei darf keine Verknüpfung sein.
- **Grenzen:** Safari muss im Vordergrund bleiben und der Bildschirm an (Wake Lock hilft). Quer halten; Hochkant wird mittig
  beschnitten. Die iPhone-Seite (Kamerazugriff, Zertifikat-Ausnahme, Bildrate)
  ist **noch nicht am iPhone geprüft**; die Strecke PC-Seite bis zur virtuellen Kamera ist mit einem JPEG per HTTPS bis in
  Media Foundation geprüft.
- **Deinstallieren:** die Notch entfernt die systemweite Eintragung nicht selbst. Von Hand (Administrator-PowerShell):
  `Remove-Item -Recurse 'C:\Program Files\Notch'; reg delete 'HKLM\Software\Classes\CLSID\{7C0C9B3E-5A1D-4F6B-9E47-2D6A1B8C3F10}' /f`.

## Prüfen am Rechner

```
cargo test --lib                                   # Protokoll, Farbumrechnung, Seitenplatzhalter
cargo test --manifest-path vcam/Cargo.toml         # Quelle: letztes Bild halten, Platzhalter nach 5 s
cargo test --lib vcam::tests::vcam_ende -- --ignored --nocapture   # virtuelle Kamera Ende zu Ende (NOTCH_EXE = gebaute notch.exe)
```

## Herkunft und Abhängigkeiten

- Eigener Code. Als Anhaltspunkt diente die README von smourier/VCamSample (MIT): nur die Erkenntnis, dass die Quelle
  in HKLM stehen muss und der Frame-Server sie als `IMFActivate` anfordert (bestätigt am eigenen Rechner). Kein Code übernommen.
- Neu: `rcgen` 0.13 (Zertifikat, nur Windows), `tiny_http` mit Feature `ssl-rustls` (HTTPS-Server, nur Windows). Für `vcam/`:
  `windows`/`windows-core` 0.62 (wie die Notch).
