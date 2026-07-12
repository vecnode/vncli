@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM ---------------------------------------------------------------------------
REM setup_dotfiles.bat - Copy dotfiles to Windows 11 user profile
REM ---------------------------------------------------------------------------

REM Elevate to Administrator if not already running as admin.
REM Re-launches this exact script with the same working directory via UAC.
net session >nul 2>nul
if errorlevel 1 (
    echo [INFO] Requesting Administrator rights...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process cmd -ArgumentList '/C \"%~f0\"' -Verb RunAs -Wait"
    exit /b 0
)

echo [INFO] Running as Administrator.
set "DOTFILES_SOURCE=%~dp0"
set "SSH_CONFIG_SOURCE=%DOTFILES_SOURCE%ssh\config"
set "SSH_CONFIG_DEST=%USERPROFILE%\.ssh\config"

REM Ensure .ssh directory exists
if not exist "%USERPROFILE%\.ssh" (
    mkdir "%USERPROFILE%\.ssh"
    echo [INFO] Created directory: %USERPROFILE%\.ssh
)

REM Backup existing SSH config if it exists
if exist "%SSH_CONFIG_DEST%" (
    set "BACKUP_FILE=%SSH_CONFIG_DEST%.backup_%RANDOM%"
    copy "%SSH_CONFIG_DEST%" "!BACKUP_FILE!" >nul
    echo [INFO] Backed up existing SSH config to: !BACKUP_FILE!
)

REM Copy SSH config from dotfiles to destination
if exist "%SSH_CONFIG_SOURCE%" (
    copy "%SSH_CONFIG_SOURCE%" "%SSH_CONFIG_DEST%" >nul
    echo [INFO] Copied SSH config to: %SSH_CONFIG_DEST%
) else (
    echo [WARNING] SSH config source not found: %SSH_CONFIG_SOURCE%
)

REM ---------------------------------------------------------------------------
REM Run global_configs.ps1
REM ---------------------------------------------------------------------------
set "GLOBAL_CONFIGS=%DOTFILES_SOURCE%global_configs.ps1"

if exist "%GLOBAL_CONFIGS%" (
    echo [INFO] Running global_configs.ps1...
    powershell -NoProfile -ExecutionPolicy Bypass -File "%GLOBAL_CONFIGS%"
    if errorlevel 1 (
        echo [ERROR] global_configs.ps1 exited with an error.
        exit /b 1
    )
) else (
    echo [WARNING] global_configs.ps1 not found: %GLOBAL_CONFIGS%
)

echo [INFO] Dotfiles setup complete.
exit /b 0
