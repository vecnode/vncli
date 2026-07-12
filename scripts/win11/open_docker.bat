@echo off
setlocal EnableExtensions

tasklist /FI "IMAGENAME eq Docker Desktop.exe" 2>nul | find /I "Docker Desktop.exe" >nul
if not errorlevel 1 (
    echo [INFO] Docker Desktop is already running.
    exit /b 0
)

if exist "C:\Program Files\Docker\Docker\Docker Desktop.exe" (
    start "Docker Desktop" "C:\Program Files\Docker\Docker\Docker Desktop.exe"
    echo [INFO] Docker Desktop launch requested.
    exit /b 0
)

echo [ERROR] Docker Desktop executable not found at:
echo   C:\Program Files\Docker\Docker\Docker Desktop.exe
exit /b 1
