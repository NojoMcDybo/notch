@echo off
rem Startet Haze aus dem gerade ausgecheckten Branch in D:\Dev\haze zum Testen
rem der Notch-Anbindung. Per explorer.exe starten, nicht aus der Claude-App (MSIX-Virtualisierung).
rem Das installierte Haze wird dafuer beendet (gleiche App, nur eine Instanz). Zurueck zum installierten:
rem diese Haze-Instanz ueber das Tray beenden und Haze normal aus dem Startmenue starten.
rem Die AGENTS.md von Haze erlaubt Installer nur aus main - deshalb hier nur ein Start, keine Installation.
taskkill /im Haze.exe /f >nul 2>&1
timeout /t 2 /nobreak >nul
cd /d D:\Dev\haze
start "" "node_modules\electron\dist\electron.exe" .
