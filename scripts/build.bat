@echo off
chcp 65001 >nul 2>nul
setlocal
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd /d "%~dp0.."
title SChat - Build

REM --- Read version ---
if not exist "scripts\version.txt" (echo [ERROR] scripts\version.txt not found & pause & exit /b 1)
set /p VER=<"scripts\version.txt"
set VER=%VER:v=%
echo [SChat] Version: %VER%

REM --- Sync version to package.json ---
npm version %VER% --no-git-tag-version --allow-same-version >nul 2>nul

REM --- Sync version to tauri.conf.json ---
powershell -NoProfile -Command ^
  "$f='src-tauri/tauri.conf.json'; $c=Get-Content $f -Raw; $c=$c -replace '\"version\"\s*:\s*\"[^\"]*\"', '\"version\": \"%VER%\"'; Set-Content $f $c -NoNewline"

REM --- Sync version to Cargo.toml (top-level only) ---
powershell -NoProfile -Command ^
  "$f='src-tauri/Cargo.toml'; $c=Get-Content $f -Raw; $c=$c -replace '(?m)^version\s*=\s*\"[^\"]*\"', 'version = \"%VER%\"'; Set-Content $f $c -NoNewline"

echo [SChat] package.json / tauri.conf.json / Cargo.toml synced to %VER%
echo.
echo ============================================
echo   SChat Build  v%VER%
echo ============================================
where cargo >nul 2>nul || (echo [ERROR] cargo not found & pause & exit /b 1)
where npm   >nul 2>nul || (echo [ERROR] npm not found   & pause & exit /b 1)
if not exist node_modules (echo [SChat] Installing frontend deps... & call npm install)

echo [SChat] Cleaning old build artifacts...
if exist "dist" rd /s /q "dist" >nul 2>nul
if exist "src-tauri\target\release" rd /s /q "src-tauri\target\release" >nul 2>nul
if exist "src-tauri\target\bundle" rd /s /q "src-tauri\target\bundle" >nul 2>nul
echo [SChat] Clean done.

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
