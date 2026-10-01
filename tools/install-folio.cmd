@echo off
rem Installiert den aktuellen Folio-Release-Build ausserhalb der Claude-App (siehe install-notch.cmd).
taskkill /im folio.exe /f >nul 2>&1
"D:\Dev\folio\src-tauri\target\release\bundle\nsis\Folio_0.7.0_x64-setup.exe" /S
