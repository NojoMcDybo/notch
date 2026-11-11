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
Windows 11); Teams, Zoom, Chrome und die Kamera-App sehen eine normale Webcam. Ohne frisches Bild zeigt sie ein dunkles Bild
mit laufendem Balken.

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
  beschnitten. Kein Objektivwechsel auf Ultraweitwinkel. Die iPhone-Seite (Kamerazugriff, Zertifikat-Ausnahme, Bildrate)
  ist **noch nicht am iPhone geprüft**; die Strecke PC-Seite bis zur virtuellen Kamera ist mit einem JPEG per HTTPS bis in
  Media Foundation geprüft.
- **Deinstallieren:** die Notch entfernt die systemweite Eintragung nicht selbst. Von Hand (Administrator-PowerShell):
  `Remove-Item -Recurse 'C:\Program Files\Notch'; reg delete 'HKLM\Software\Classes\CLSID\{7C0C9B3E-5A1D-4F6B-9E47-2D6A1B8C3F10}' /f`.

## Prüfen am Rechner

```
cargo test --lib                                   # Protokoll, Farbumrechnung, Seitenplatzhalter
cargo test --lib vcam::tests::vcam_ende -- --ignored --nocapture   # virtuelle Kamera Ende zu Ende (NOTCH_EXE = gebaute notch.exe)
```

## Herkunft und Abhängigkeiten

- Eigener Code. Als Anhaltspunkt diente die README von smourier/VCamSample (MIT): nur die Erkenntnis, dass die Quelle
  in HKLM stehen muss und der Frame-Server sie als `IMFActivate` anfordert (bestätigt am eigenen Rechner). Kein Code übernommen.
- Neu: `rcgen` 0.13 (Zertifikat, nur Windows), `tiny_http` mit Feature `ssl-rustls` (HTTPS-Server, nur Windows). Für `vcam/`:
  `windows`/`windows-core` 0.62 (wie die Notch).
