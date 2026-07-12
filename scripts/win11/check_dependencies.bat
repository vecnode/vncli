@echo off
REM ---------------------------------------------------------------------------
REM check_dependencies.bat
REM Comprehensive dependency checker and installer for vncli CLI.
REM
REM Checks for: git, curl, jq, docker, winget, rustscan
REM Offers automatic installation if any are missing.
REM
REM Usage:
REM   check_dependencies.bat
REM ---------------------------------------------------------------------------

setlocal EnableExtensions EnableDelayedExpansion


REM Initialize variables
set "DEPENDENCIES=git curl jq docker winget rustscan"
set /a MISSING_COUNT=0
set "MISSING_LIST="

REM ---------------------------------------------------------------------------
REM DEPENDENCY CHECK PHASE
REM ---------------------------------------------------------------------------

echo Checking for required dependencies...
echo.

for %%D in (%DEPENDENCIES%) do (
    set "DEP=%%D"
    set "FOUND=0"
    set "VERSION="
    
    echo [Checking !DEP!]
    
    where !DEP! >nul 2>nul
    if !errorlevel! equ 0 (
        set "FOUND=1"
        
        if "!DEP!"=="git" (
            for /f "tokens=*" %%V in ('git --version 2^>nul') do set "VERSION=%%V"
            echo   [OK] !VERSION!
        ) else if "!DEP!"=="curl" (
            set "VERSION="
            for /f "tokens=* delims=" %%V in ('curl --version 2^>nul') do (
                if not defined VERSION set "VERSION=%%V"
            )
            echo   [OK] !VERSION!
        ) else if "!DEP!"=="jq" (
            for /f "tokens=*" %%V in ('jq --version 2^>nul') do set "VERSION=%%V"
            echo   [OK] !VERSION!
        ) else if "!DEP!"=="docker" (
            docker ps >nul 2>nul
            if !errorlevel! equ 0 (
                for /f "tokens=*" %%V in ('docker --version 2^>nul') do set "VERSION=%%V"
                echo   [OK] !VERSION!
            ) else (
                echo   [WARNING] Found but daemon may not be running
                set "FOUND=1"
            )
        ) else if "!DEP!"=="winget" (
            for /f "tokens=*" %%V in ('winget --version 2^>nul') do set "VERSION=%%V"
            echo   [OK] winget !VERSION!
        ) else if "!DEP!"=="rustscan" (
            for /f "tokens=*" %%V in ('rustscan --version 2^>nul') do set "VERSION=%%V"
            echo   [OK] !VERSION!
        )
    ) else (
        echo   [MISSING]
        set /a MISSING_COUNT+=1
        set "MISSING_LIST=!MISSING_LIST! !DEP!"
    )
    echo.
)

REM ---------------------------------------------------------------------------
REM SUMMARY ^& INSTALLATION PROMPT
REM ---------------------------------------------------------------------------

if !MISSING_COUNT! equ 0 (
    echo [SUCCESS] All dependencies are installed!
    echo.
    exit /b 0
)

echo [WARNING] The following dependencies are missing or not accessible:
for %%M in (!MISSING_LIST!) do (
    echo   - %%M
)
echo.

:install_prompt
set "INSTALL_CHOICE="
set /p INSTALL_CHOICE="Would you like to install the missing dependencies? (y/n): "

if /i "!INSTALL_CHOICE!"=="y" (
    goto :install_phase
) else if /i "!INSTALL_CHOICE!"=="n" (
    echo.
    echo [INFO] Skipping installation.
    exit /b 0
) else (
    echo [ERROR] Invalid choice. Please enter 'y' or 'n'.
    goto :install_prompt
)

REM ---------------------------------------------------------------------------
REM INSTALLATION PHASE
REM ---------------------------------------------------------------------------

:install_phase


set "WINGET_MISSING=0"
for %%M in (!MISSING_LIST!) do (
    if "%%M"=="winget" set "WINGET_MISSING=1"
)

if "!WINGET_MISSING!"=="1" (
    echo [ERROR] winget is missing and is required to install other dependencies automatically.
    echo Install App Installer from Microsoft Store:
    echo   https://aka.ms/getwinget
    exit /b 1
)


where winget >nul 2>nul
if !errorlevel! equ 0 (
    echo [INFO] Using winget package manager
    echo.
    
    for %%M in (!MISSING_LIST!) do (
        if "%%M"=="git" (
            echo [INFO] Installing Git...
            winget install -e --id Git.Git --accept-package-agreements --accept-source-agreements >nul 2>nul
            if !errorlevel! equ 0 (
                echo [OK] git installed
            ) else (
                echo [WARNING] git installation may require manual action
            )
        ) else if "%%M"=="curl" (
            echo [INFO] Installing curl...
            winget install -e --id cURL.cURL --accept-package-agreements --accept-source-agreements >nul 2>nul
            if !errorlevel! equ 0 (
                echo [OK] curl installed
            ) else (
                echo [WARNING] curl installation may require manual action
            )
        ) else if "%%M"=="jq" (
            echo [INFO] Installing jq...
            winget install -e --id jqlang.jq --accept-package-agreements --accept-source-agreements >nul 2>nul
            if !errorlevel! equ 0 (
                echo [OK] jq installed
            ) else (
                echo [WARNING] jq installation may require manual action
            )
        ) else if "%%M"=="docker" (
            echo [INFO] Installing Docker...
            winget install -e --id Docker.DockerDesktop --accept-package-agreements --accept-source-agreements >nul 2>nul
            if !errorlevel! equ 0 (
                echo [OK] Docker installed
            ) else (
                echo [WARNING] Docker installation may require manual steps
            )
        ) else if "%%M"=="rustscan" (
            echo [INFO] Installing rustscan via cargo...
            cargo install rustscan >nul 2>nul
            if !errorlevel! equ 0 (
                echo [OK] rustscan installed
            ) else (
                echo [WARNING] rustscan install failed ^(requires Rust/cargo: https://rustup.rs/^)
            )
        ) else (
            echo [INFO] Installing %%M...
            winget install -e --id %%M --accept-package-agreements --accept-source-agreements >nul 2>nul
            if !errorlevel! equ 0 (
                echo [OK] %%M installed
            ) else (
                echo [WARNING] %%M installation may require manual action
            )
        )
    )
) else (
    echo [INFO] winget not found. Installation requires manual action or alternative package manager.
    echo.
    echo Please install the missing dependencies:
    for %%M in (!MISSING_LIST!) do (
        if "%%M"=="docker" (
            echo   - Docker: https://docs.docker.com/engine/install/
        ) else if "%%M"=="git" (
            echo   - Git: https://git-scm.com/download/win
        ) else if "%%M"=="curl" (
            echo   - Curl: https://curl.se/download.html
        ) else if "%%M"=="jq" (
            echo   - jq: https://stedolan.github.io/jq/download/
        ) else if "%%M"=="winget" (
            echo   - winget (App Installer): https://aka.ms/getwinget
        ) else if "%%M"=="rustscan" (
            echo   - rustscan: cargo install rustscan (install Rust from https://rustup.rs/)
        )
    )
    exit /b 1
)

REM ---------------------------------------------------------------------------
REM VERIFICATION PHASE
REM ---------------------------------------------------------------------------


set /a VERIFICATION_FAILED=0

for %%M in (!MISSING_LIST!) do (
    set "DEP=%%M"
    
    where !DEP! >nul 2>nul
    if !errorlevel! equ 0 (
        if "!DEP!"=="git" (
            for /f "tokens=*" %%V in ('git --version 2^>nul') do set "VERSION=%%V"
            echo   Verifying !DEP!... [OK] !VERSION!
        ) else if "!DEP!"=="curl" (
            set "VERSION="
            for /f "tokens=* delims=" %%V in ('curl --version 2^>nul') do (
                if not defined VERSION set "VERSION=%%V"
            )
            echo   Verifying !DEP!... [OK] !VERSION!
        ) else if "!DEP!"=="jq" (
            for /f "tokens=*" %%V in ('jq --version 2^>nul') do set "VERSION=%%V"
            echo   Verifying !DEP!... [OK] !VERSION!
        ) else if "!DEP!"=="docker" (
            for /f "tokens=*" %%V in ('docker --version 2^>nul') do set "VERSION=%%V"
            echo   Verifying !DEP!... [OK] !VERSION!
        ) else if "!DEP!"=="winget" (
            for /f "tokens=*" %%V in ('winget --version 2^>nul') do set "VERSION=%%V"
            echo   Verifying !DEP!... [OK] winget !VERSION!
        ) else if "!DEP!"=="rustscan" (
            for /f "tokens=*" %%V in ('rustscan --version 2^>nul') do set "VERSION=%%V"
            echo   Verifying !DEP!... [OK] !VERSION!
        )
    ) else (
        echo   Verifying !DEP!... [FAILED]
        set /a VERIFICATION_FAILED=1
    )
)

echo.

if !VERIFICATION_FAILED! equ 0 (
    echo [SUCCESS] All dependencies verified successfully!
    echo.
    exit /b 0
) else (
    echo [ERROR] Some dependencies failed verification. Please try manual installation.
    echo.
    exit /b 1
)

endlocal
