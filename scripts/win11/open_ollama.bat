@echo off
setlocal EnableExtensions

REM Prefer the actual installed Ollama app ("ollama app.exe" - the tray app
REM that starts the server itself) over the bare "ollama.exe" CLI: launching
REM the CLI with no arguments just prints usage and exits, it doesn't start
REM anything. Only fall back to "ollama.exe serve" if the app isn't installed.

tasklist /FI "IMAGENAME eq ollama app.exe" 2>nul | find /I "ollama app.exe" >nul
if not errorlevel 1 (
    echo [INFO] Ollama is already running.
    exit /b 0
)

if exist "%LOCALAPPDATA%\Programs\Ollama\ollama app.exe" (
    start "" "%LOCALAPPDATA%\Programs\Ollama\ollama app.exe"
    echo [INFO] Ollama app launch requested.
    exit /b 0
)

if exist "C:\Program Files\Ollama\ollama app.exe" (
    start "" "C:\Program Files\Ollama\ollama app.exe"
    echo [INFO] Ollama app launch requested.
    exit /b 0
)

REM No tray app found - fall back to starting the server directly via the CLI.
tasklist /FI "IMAGENAME eq ollama.exe" 2>nul | find /I "ollama.exe" >nul
if not errorlevel 1 (
    echo [INFO] Ollama server is already running.
    exit /b 0
)

if exist "%LOCALAPPDATA%\Programs\Ollama\ollama.exe" (
    start "" "%LOCALAPPDATA%\Programs\Ollama\ollama.exe" serve
    echo [INFO] Ollama server launch requested.
    exit /b 0
)

if exist "C:\Program Files\Ollama\ollama.exe" (
    start "" "C:\Program Files\Ollama\ollama.exe" serve
    echo [INFO] Ollama server launch requested.
    exit /b 0
)

echo [ERROR] Ollama executable not found. Is it installed?
exit /b 1
