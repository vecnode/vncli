@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM ---------------------------------------------------------------------------
REM main.bat
REM Entry point for vncli CLI
REM Usage:
REM   main.bat
REM
REM Requirements (Windows):
REM   - git
REM   - curl
REM   - jq
REM ---------------------------------------------------------------------------

cls


for %%C in (git curl jq) do (
    where %%C >nul 2>nul
    if errorlevel 1 (
        echo [ERROR] Required command not found: %%C
        echo Please install: git, curl, and jq
        exit /b 1
    )
)

REM ---------------------------------------------------------------------------
REM MAIN MENU - CHOOSE OPERATION
REM ---------------------------------------------------------------------------

:main_menu
echo.
echo What would you like to do?
echo   1 = Docker
echo   2 = GitHub
echo   3 = Silverbullet
echo   4 = Settings
echo   5 = Quit
echo.
set "MAIN_CHOICE="
set /p MAIN_CHOICE="Enter your choice (1, 2, 3, 4, or 5): "

if "%MAIN_CHOICE%"=="1" (
    echo.
    goto :docker_menu
)

if "%MAIN_CHOICE%"=="2" (
    echo.
    goto :github_menu
)

if "%MAIN_CHOICE%"=="3" (
    echo.
    goto :silverbullet_menu
)

if "%MAIN_CHOICE%"=="4" (
    echo.
    goto :settings_menu
)

if "%MAIN_CHOICE%"=="5" (
    echo.
    echo [INFO] Exiting.
    exit /b 0
)

echo [ERROR] Invalid choice. Please enter 1, 2, 3, 4, or 5.
goto :main_menu

REM ---------------------------------------------------------------------------
REM GITHUB MENU
REM ---------------------------------------------------------------------------

:github_menu
echo What would you like to do?
echo   1 = Backup GitHub
echo   2 = Menu
echo   3 = Quit
echo.
set "GITHUB_MENU_CHOICE="
set /p GITHUB_MENU_CHOICE="Enter your choice (1, 2, or 3): "

if "%GITHUB_MENU_CHOICE%"=="1" (
    echo.
    goto :github_header
)

if "%GITHUB_MENU_CHOICE%"=="2" (
    echo.
    goto :main_menu
)

if "%GITHUB_MENU_CHOICE%"=="3" (
    echo.
    echo [INFO] Exiting.
    exit /b 0
)

echo [ERROR] Invalid choice. Please enter 1, 2, or 3.
echo.
goto :github_menu

REM ---------------------------------------------------------------------------
REM SETTINGS MENU
REM ---------------------------------------------------------------------------

:settings_menu
echo What would you like to do?
echo   1 = Check Internet
echo   2 = CLI Dependencies
echo   3 = Menu
echo   4 = Quit
echo.
set "SETTINGS_CHOICE="
set /p SETTINGS_CHOICE="Enter your choice (1, 2, 3, or 4): "

if "%SETTINGS_CHOICE%"=="1" goto :check_internet

if "%SETTINGS_CHOICE%"=="2" (
    echo.
    call "%~dp0check_dependencies.bat"
    echo.
    goto :settings_menu
)

if "%SETTINGS_CHOICE%"=="3" (
    echo.
    goto :main_menu
)

if "%SETTINGS_CHOICE%"=="4" (
    echo.
    echo [INFO] Exiting.
    exit /b 0
)

echo [ERROR] Invalid choice. Please enter 1, 2, 3, or 4.
echo.
goto :settings_menu

REM ---------------------------------------------------------------------------
REM SETTINGS - INTERNET CHECK
REM ---------------------------------------------------------------------------

:check_internet
call "%~dp0check_internet.bat"
goto :settings_menu

REM ---------------------------------------------------------------------------
REM DOCKER MENU
REM ---------------------------------------------------------------------------

:docker_menu
echo What would you like to do?
echo   1 = Open Docker Desktop (Win)
echo   2 = Clear Containers and Images
echo   3 = Start CLI Container
echo   4 = Menu
echo   5 = Quit
echo.
set "DOCKER_CHOICE="
set /p DOCKER_CHOICE="Enter your choice (1, 2, 3, 4, or 5): "

if "%DOCKER_CHOICE%"=="1" (
    echo.
    tasklist /FI "IMAGENAME eq Docker Desktop.exe" 2>nul | find /I "Docker Desktop.exe" >nul
    if not errorlevel 1 (
        echo [INFO] Docker Desktop is already running.
        echo.
        goto :docker_menu
    )

    if exist "C:\Program Files\Docker\Docker\Docker Desktop.exe" (
        start "Docker Desktop" "C:\Program Files\Docker\Docker\Docker Desktop.exe"
        echo [INFO] Docker Desktop launch requested.
    ) else (
        echo [ERROR] Docker Desktop executable not found at:
        echo   C:\Program Files\Docker\Docker\Docker Desktop.exe
    )

    echo.
    goto :docker_menu
)

if "%DOCKER_CHOICE%"=="2" (
    echo.
    where docker >nul 2>nul
    if errorlevel 1 (
        echo [ERROR] Docker is not available or not in PATH
        echo Please install Docker Engine/Desktop from:
        echo   https://docs.docker.com/engine/install/
        echo.
        goto :docker_menu
    )

    set "HAS_CONTAINERS="
    for /f "usebackq delims=" %%C in (`docker ps -aq 2^>nul`) do (
        set "HAS_CONTAINERS=1"
        docker stop %%C >nul 2>nul
    )
    if not defined HAS_CONTAINERS echo No containers to stop

    set "HAS_CONTAINERS="
    for /f "usebackq delims=" %%C in (`docker ps -aq 2^>nul`) do (
        set "HAS_CONTAINERS=1"
        docker rm -f %%C >nul 2>nul
    )
    if not defined HAS_CONTAINERS echo No containers to remove

    set "HAS_IMAGES="
    for /f "usebackq delims=" %%I in (`docker images -aq 2^>nul`) do (
        set "HAS_IMAGES=1"
        docker rmi -f %%I >nul 2>nul
    )
    if not defined HAS_IMAGES echo No images to remove

    echo.
    goto :docker_menu
)

if "%DOCKER_CHOICE%"=="3" (
    echo.
    where docker >nul 2>nul
    if errorlevel 1 (
        echo [ERROR] Docker is not available or not in PATH
        echo Please install Docker Engine/Desktop from:
        echo   https://docs.docker.com/engine/install/
        echo.
        goto :docker_menu
    )

    start "vncli CLI Container" cmd /k call "%~dp0run_cli_container.bat"
    echo [INFO] Opened CLI container in a new terminal window.
    echo.
    goto :docker_menu
)

if "%DOCKER_CHOICE%"=="4" (
    echo.
    goto :main_menu
)

if "%DOCKER_CHOICE%"=="5" (
    echo.
    echo [INFO] Exiting.
    exit /b 0
)

echo [ERROR] Invalid choice. Please enter 1, 2, 3, 4, or 5.
echo.
goto :docker_menu

REM ---------------------------------------------------------------------------
REM SILVERBULLET MENU
REM ---------------------------------------------------------------------------

:silverbullet_menu
echo What would you like to do?
echo   1 = Run Silverbullet
echo   2 = Menu
echo   3 = Quit
echo.
set "SILVERBULLET_CHOICE="
set /p SILVERBULLET_CHOICE="Enter your choice (1, 2, or 3): "

if "%SILVERBULLET_CHOICE%"=="1" (
    echo.
    echo [INFO] SilverBullet is now launched natively by vn.
    echo [INFO] Run:  vn app open silverbullet
    echo.
    goto :silverbullet_menu
)

if "%SILVERBULLET_CHOICE%"=="2" (
    echo.
    goto :main_menu
)

if "%SILVERBULLET_CHOICE%"=="3" (
    echo.
    echo [INFO] Exiting.
    exit /b 0
)

echo [ERROR] Invalid choice. Please enter 1, 2, or 3.
echo.
goto :silverbullet_menu

REM ---------------------------------------------------------------------------
REM GITHUB BACKUP - USERNAME PROMPT
REM ---------------------------------------------------------------------------

:github_header



:prompt_username
echo.
set "GITHUB_USERNAME="
set /p GITHUB_USERNAME="Enter GitHub username: "

if not defined GITHUB_USERNAME (
    echo [ERROR] GitHub username cannot be empty.
    goto :prompt_username
)

echo [INFO] GitHub username set to: %GITHUB_USERNAME%
echo.

REM ---------------------------------------------------------------------------
REM GITHUB BACKUP - SOURCE CHOICE
REM ---------------------------------------------------------------------------

:prompt_source
echo.
echo What would you like to download?
echo   1 = Personal repositories only
echo   2 = Organizations only
echo   3 = Both personal repositories and organizations
echo   4 = Menu
echo   5 = Quit
echo.
set "SOURCE_CHOICE="
set /p SOURCE_CHOICE="Enter your choice (1, 2, 3, 4, or 5): "

if "%SOURCE_CHOICE%"=="1" goto :prompt_target_dir

if "%SOURCE_CHOICE%"=="2" goto :prompt_target_dir

if "%SOURCE_CHOICE%"=="3" goto :prompt_target_dir

if "%SOURCE_CHOICE%"=="4" (
    echo.
    goto :main_menu
)

if "%SOURCE_CHOICE%"=="5" (
    echo.
    echo [INFO] Exiting.
    exit /b 0
)

echo [ERROR] Invalid choice. Please enter 1, 2, 3, 4, or 5.
goto :prompt_source

REM ---------------------------------------------------------------------------
REM GITHUB BACKUP - TARGET DIRECTORY PROMPT
REM ---------------------------------------------------------------------------

:prompt_target_dir
set "TS_FILE=%TEMP%\vncli-ts-%RANDOM%-%RANDOM%.txt"
powershell -NoProfile -Command "Get-Date -Format 'dd-MM-yyyy-HH-mm-ss'" > "%TS_FILE%"
set "TIMESTAMP="
if exist "%TS_FILE%" set /p TIMESTAMP=<"%TS_FILE%"
if exist "%TS_FILE%" del /q "%TS_FILE%" >nul 2>nul
if not defined TIMESTAMP set "TIMESTAMP=%RANDOM%-%RANDOM%"

if "%SOURCE_CHOICE%"=="2" (
    set "DEFAULT_DOWNLOAD_TARGET=%USERPROFILE%\Desktop\git-backup-orgs-%TIMESTAMP%"
) else (
    set "DEFAULT_DOWNLOAD_TARGET=%USERPROFILE%\Desktop\git-backup-%TIMESTAMP%"
)

echo.
set "DOWNLOAD_TARGET_INPUT="
set /p DOWNLOAD_TARGET_INPUT="Where should the repositories be downloaded? (press Enter for default: %DEFAULT_DOWNLOAD_TARGET%): "

if not defined DOWNLOAD_TARGET_INPUT (
    set "DOWNLOAD_TARGET_DIR=%DEFAULT_DOWNLOAD_TARGET%"
) else (
    set "DOWNLOAD_TARGET_DIR=%DOWNLOAD_TARGET_INPUT:"=%"
)

echo [INFO] Download target set to: %DOWNLOAD_TARGET_DIR%
echo.

if "%SOURCE_CHOICE%"=="1" (
    echo.
    echo [INFO] Downloading personal repositories for "%GITHUB_USERNAME%"
    echo.
    set "VNCLI_TARGET_DIR=%DOWNLOAD_TARGET_DIR%"
    call "%~dp0download_all_repos.bat" "%GITHUB_USERNAME%"
    goto :summary
)

if "%SOURCE_CHOICE%"=="2" (
    echo.
    set "ORG_NAMES="
    set /p ORG_NAMES="Organization name(s), space-separated: "
    if not defined ORG_NAMES (
        echo [ERROR] No organization names given.
        goto :prompt_source
    )
    echo [INFO] Downloading organization repositories
    echo.
    set "VNCLI_TARGET_DIR=%DOWNLOAD_TARGET_DIR%"
    call "%~dp0download_all_orgs.bat" %ORG_NAMES%
    goto :summary
)

if "%SOURCE_CHOICE%"=="3" (
    echo.
    echo [INFO] Downloading personal repositories for "%GITHUB_USERNAME%"
    echo.
    set "VNCLI_TARGET_DIR=%DOWNLOAD_TARGET_DIR%"
    call "%~dp0download_all_repos.bat" "%GITHUB_USERNAME%"
    echo.
    set "ORG_NAMES="
    set /p ORG_NAMES="Organization name(s), space-separated: "
    if not defined ORG_NAMES (
        echo [ERROR] No organization names given; skipping organizations.
        goto :summary
    )
    echo [INFO] Downloading organization repositories
    echo.
    call "%~dp0download_all_orgs.bat" %ORG_NAMES%
    goto :summary
)

REM ---------------------------------------------------------------------------
REM COMPLETION
REM ---------------------------------------------------------------------------

:summary


endlocal
exit /b 0
