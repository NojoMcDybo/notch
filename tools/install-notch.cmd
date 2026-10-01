@echo off
rem Installiert den aktuellen Release-Build der Notch und startet sie.
rem Per Doppelklick bzw. ueber den Explorer starten - NICHT aus der Claude-App heraus:
rem Prozesse, die die Claude-App startet, schreiben in einen umgeleiteten AppData-Ordner.
taskkill /im notch.exe /f >nul 2>&1
"%~dp0..\src-tauri\target\release\bundle\nsis\Notch_0.1.0_x64-setup.exe" /S
timeout /t 2 /nobreak >nul
start "" "%LOCALAPPDATA%\Notch\notch.exe"
