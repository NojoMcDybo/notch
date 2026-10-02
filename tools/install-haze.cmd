@echo off
rem Installiert den aktuellen Haze-Installer (nur aus main gebaut!) und startet Haze wieder.
rem Per Explorer starten - NICHT aus der Claude-App heraus (sonst landet alles im virtualisierten Paketordner).
set SETUP=D:\Dev\haze\release\Haze-Setup-1.3.0.exe
if not exist "%SETUP%" (echo Installer fehlt: %SETUP% & pause & exit /b 1)
rem erst sanft beenden (Haze nimmt dabei ihren Eintrag aus der Notch), dann sicherheitshalber hart
taskkill /im Haze.exe >nul 2>&1
timeout /t 3 /nobreak >nul
taskkill /im Haze.exe /f >nul 2>&1
"%SETUP%" /S
timeout /t 3 /nobreak >nul
start "" "%LOCALAPPDATA%\Programs\Haze\Haze.exe" --autostart
