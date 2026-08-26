@echo off
chcp 65001 >nul 2>nul
setlocal
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd /d "%~dp0.."
title SChat - Dev
echo ============================================
echo   SChat Dev Mode (close window or Ctrl+C)
echo ============================================
where cargo >nul 2>nul || (echo [ERROR] cargo not found & pause & exit /b 1)
where npm   >nul 2>nul || (echo [ERROR] npm not found & pause & exit /b 1)
if not exist node_modules (echo [SChat] Installing frontend deps... & call npm install)
call npm run tauri dev
pause
