@echo off
chcp 65001 >nul 2>nul
setlocal
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd /d "%~dp0.."
title SChat - Build

where cargo >nul 2>nul || (echo [ERROR] cargo not found & pause & exit /b 1)
where npm   >nul 2>nul || (echo [ERROR] npm not found   & pause & exit /b 1)

REM --- Read version ---
if not exist "scripts\version.txt" (echo [ERROR] scripts\version.txt not found & pause & exit /b 1)
set /p VER=<"scripts\version.txt"
set VER=%VER:v=%
echo [SChat] Version: %VER%

REM --- Sync version ---
node scripts\sync-version.cjs "%VER%"
if errorlevel 1 (echo [ERROR] version sync failed & pause & exit /b 1)

REM --- Install deps if needed ---
if not exist node_modules (echo [SChat] Installing frontend deps... & call npm install)

REM --- Clean ---
echo [SChat] Cleaning old build artifacts...
if exist "dist" rd /s /q "dist" >nul 2>nul
if exist "src-tauri\target\release" rd /s /q "src-tauri\target\release" >nul 2>nul
if exist "src-tauri\target\bundle" rd /s /q "src-tauri\target\bundle" >nul 2>nul
echo [SChat] Clean done.

echo.
echo ============================================
echo   SChat Build  v%VER%
echo ============================================

REM --- Build ---
call npm run tauri build
if errorlevel 1 (
    echo.
    echo [SChat] Build failed, see log above.
    pause
    exit /b 1
)

set "BUNDLE_DIR=src-tauri\target\release\bundle\nsis"
echo.
echo ============================================
echo   Build OK! Installer:
echo --------------------------------------------
for %%f in ("%BUNDLE_DIR%\*.exe") do echo   %%f
echo ============================================
start "" explorer "%BUNDLE_DIR%"
pause
