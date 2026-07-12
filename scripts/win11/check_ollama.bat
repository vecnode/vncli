@echo off
setlocal EnableExtensions

tasklist /FI "IMAGENAME eq ollama.exe" 2>nul | find /I "ollama.exe" >nul
if not errorlevel 1 (
    echo [INFO] Ollama is running.
    exit /b 0
)

sc query "ollama" 2>nul | find /I "RUNNING" >nul
if not errorlevel 1 (
    echo [INFO] Ollama service is running.
    exit /b 0
)

echo [WARN] Ollama is NOT running.
exit /b 1
