@echo off
rem Installiert den neuesten Folio-Release-Build ausserhalb der Claude-App (per explorer.exe starten).
rem Folio wird nur sanft geschlossen: hat ein Dokument ungesicherte Markierungen, bricht das Skript ab.
rem Aufruf: install-folio.cmd [PDF, das danach wieder geoeffnet wird]
setlocal
set "LOG=D:\Dev\notch\tools\install-folio.log"
set "SETUP="
for /f "delims=" %%f in ('dir /b /o:d "D:\Dev\folio\src-tauri\target\release\bundle\nsis\Folio_*_x64-setup.exe"') do set "SETUP=D:\Dev\folio\src-tauri\target\release\bundle\nsis\%%f"
if not defined SETUP (echo kein Setup gefunden > "%LOG%" & exit /b 1)

taskkill /im folio.exe >nul 2>&1
timeout /t 4 /nobreak >nul
tasklist /fi "imagename eq folio.exe" | find /i "folio.exe" >nul
if not errorlevel 1 (echo Folio laeuft noch - ungesicherte Aenderungen? Abgebrochen. > "%LOG%" & exit /b 2)

"%SETUP%" /S
echo installiert: %SETUP% > "%LOG%"
if not "%~1"=="" start "" "%LOCALAPPDATA%\Folio\folio.exe" "%~1"
