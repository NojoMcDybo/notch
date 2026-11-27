# Notch signieren (Windows)

Die Intelligente App-Steuerung von Windows 11 (Smart App Control) blockiert Programme, die weder signiert noch bei
Microsoft bekannt sind. Jede neue Notch-Version ist zunächst unbekannt: der Updater und der Installer werden dann
blockiert (Ereignisanzeige › Anwendungs- und Dienstprotokolle › Microsoft › Windows › CodeIntegrity › Operational,
Ereignis 3077 „did not meet the Enterprise signing level“). Abhilfe ist eine Code-Signatur von einer anerkannten Stelle;
ein selbst erstelltes Zertifikat genügt nicht.

Die Notch nutzt dafür die **SignPath Foundation** (kostenlos für Open-Source-Projekte). Der Release-Workflow
(`.github/workflows/release.yml`) signiert, sobald das Secret `SIGNPATH_API_TOKEN` gesetzt ist; ohne bleibt alles
unsigniert wie bisher.

## Was signiert wird

1. die Kamera-DLL (`vcam/`), weil sie in `notch.exe` eingebettet ist und vom Windows-Frame-Server geladen wird
   (`build.rs` übernimmt die signierte Fassung über `NOTCH_VCAM_DLL`),
2. `notch.exe`, weil sie im Installer steckt,
3. der Installer `Notch_<version>_x64-setup.exe`; danach wird die Update-Signatur (`.sig`) neu berechnet.

Jeder Schritt ist eine Signieranfrage an SignPath. Ist im Projekt eine manuelle Freigabe eingestellt, wartet der Lauf
bis zu einer Stunde je Anfrage auf die Freigabe in SignPath.

Nicht signiert: der Deinstaller im Installer (baut NSIS selbst) und die Mac-App (eigene Apple-Regeln).

## Einrichten (einmalig)

1. Bei der SignPath Foundation bewerben: <https://signpath.org> › Apply. Angaben: Repository `NojoMcDybo/notch`,
   Lizenz MIT, Build über GitHub Actions. Die Foundation prüft das Projekt; die Aufnahme ist nicht garantiert.
2. Nach der Aufnahme in SignPath:
   - Projekt-Slug notieren (Vorgabe im Workflow: `notch`),
   - Signierrichtlinie: Slug notieren (Vorgabe: `release-signing`),
   - Artefaktkonfiguration mit Slug `windows` anlegen, Inhalt aus `.signpath/artifact-configuration.xml`,
   - GitHub als vertrauenswürdiges Build-System verbinden (SignPath-Anleitung „GitHub trusted build system“),
   - API-Token für einen CI-Benutzer erzeugen.
3. In GitHub › Repository › Settings › Secrets and variables › Actions:
   - Secret `SIGNPATH_API_TOKEN` = API-Token,
   - Variable `SIGNPATH_ORGANIZATION_ID` = Organisations-ID aus SignPath,
   - optional Variablen `SIGNPATH_PROJECT_SLUG`, `SIGNPATH_POLICY_SLUG`, `SIGNPATH_ARTIFACT_CONFIG`, falls die Slugs
     von den Vorgaben abweichen.
4. Release wie gewohnt per Tag `v<version>` auslösen. Der letzte Schritt prüft mit `Get-AuthenticodeSignature`, dass
   Installer, `notch.exe` und DLL gültig signiert sind, sonst entsteht kein Release.

Die geänderte, signierte Kamera-DLL hat eine neue Kennung: beim ersten Einschalten der Kamera fragt Windows einmal nach
Administratorrechten.
