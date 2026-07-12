@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM ---------------------------------------------------------------------------
REM download_all_repos.bat
REM Clone all public repositories from a GitHub account.
REM
REM Usage:
REM   download_all_repos.bat <username>
REM   A username is required - there is no default.
REM
REM Downloads into: %%VNCLI_TARGET_DIR%% or %%USERPROFILE%%\Desktop\git-backup-DD-MM-YYYY-HH-MM-SS\
REM Requirements (Windows):
REM   - git
REM   - curl
REM   - jq
REM ---------------------------------------------------------------------------

REM ---------------------------------------------------------------------------
REM CONFIGURATION
REM ---------------------------------------------------------------------------

set "GITHUB_USER=%~1"
if not defined GITHUB_USER (
    echo [ERROR] Usage: download_all_repos.bat ^<username^>
    exit /b 1
)
set "PER_PAGE=100"

set "TS_FILE=%TEMP%\vncli-ts-%RANDOM%-%RANDOM%.txt"
powershell -NoProfile -Command "Get-Date -Format 'dd-MM-yyyy-HH-mm-ss'" > "%TS_FILE%"
set "TIMESTAMP="
if exist "%TS_FILE%" set /p TIMESTAMP=<"%TS_FILE%"
if exist "%TS_FILE%" del /q "%TS_FILE%" >nul 2>nul
if not defined TIMESTAMP set "TIMESTAMP=%RANDOM%-%RANDOM%"

if defined VNCLI_TARGET_DIR (
    set "TARGET_DIR=%VNCLI_TARGET_DIR%"
) else (
    set "TARGET_DIR=%USERPROFILE%\Desktop\git-backup-%TIMESTAMP%"
)

REM ---------------------------------------------------------------------------
REM OS CHECK
REM ---------------------------------------------------------------------------

if /i not "%OS%"=="Windows_NT" (
    echo [ERROR] This script is designed for Windows ^(detected: %OS%^).
    exit /b 1
)

REM ---------------------------------------------------------------------------
REM DEPENDENCY CHECK
REM ---------------------------------------------------------------------------

for %%C in (git curl jq) do (
    where %%C >nul 2>nul
    if errorlevel 1 (
        echo [ERROR] Required command not found: %%C
        echo         Install it with:  winget install %%C
        exit /b 1
    )
)

if not exist "%TARGET_DIR%" mkdir "%TARGET_DIR%"
echo [INFO] Syncing repos for '%GITHUB_USER%' into '%TARGET_DIR%'
echo.

set "TMP_BASE=%TEMP%\vncli-repos-%RANDOM%-%RANDOM%"
if not exist "%TMP_BASE%" mkdir "%TMP_BASE%"
set "REPO_LIST_FILE=%TMP_BASE%\repos.txt"

set /a PAGE=1
:fetch_pages
set "JSON_FILE=%TMP_BASE%\repos-!PAGE!.json"

curl -fsSL -H "Accept: application/vnd.github+json" "https://api.github.com/users/%GITHUB_USER%/repos?per_page=%PER_PAGE%^&page=!PAGE!^&type=owner" > "!JSON_FILE!"
if errorlevel 1 (
    echo [ERROR] API request failed on page !PAGE!.
    if exist "%TMP_BASE%" rmdir /s /q "%TMP_BASE%" >nul 2>nul
    exit /b 1
)

for /f %%B in ('jq -r ".[].clone_url" "!JSON_FILE!" ^| find /c /v ""') do set "BATCH_COUNT=%%B"
if "!BATCH_COUNT!"=="0" goto :done_fetch

jq -r ".[].clone_url" "!JSON_FILE!" >> "%REPO_LIST_FILE%"
if errorlevel 1 (
    echo [ERROR] Failed parsing JSON response.
    if exist "%TMP_BASE%" rmdir /s /q "%TMP_BASE%" >nul 2>nul
    exit /b 1
)

for /f %%C in ('jq "length" "!JSON_FILE!"') do set "COUNT=%%C"
if !COUNT! LSS %PER_PAGE% goto :done_fetch

set /a PAGE+=1
goto :fetch_pages

:done_fetch
set /a CLONED=0
set /a PULLED=0
set /a FAILED=0
set "ORGS_RAN=0"
set "ORGS_STATUS=SKIPPED"

if not exist "%REPO_LIST_FILE%" (
    echo [INFO] No personal repositories found for '%GITHUB_USER%'.
    goto :prompt_orgs
)

for /f %%T in ('find /c /v "" ^< "%REPO_LIST_FILE%"') do set "TOTAL=%%T"
if "%TOTAL%"=="0" (
    echo [INFO] No personal repositories found for '%GITHUB_USER%'.
    goto :prompt_orgs
)

set /a IDX=0
for /f "usebackq delims=" %%R in ("%REPO_LIST_FILE%") do (
    set /a IDX+=1
    for %%N in (%%~nR) do set "REPO_NAME=%%N"
    set "REPO_PATH=%TARGET_DIR%\!REPO_NAME!"

    echo [!IDX!/!TOTAL!] !REPO_NAME!

    if exist "!REPO_PATH!\.git" (
        git -C "!REPO_PATH!" pull --ff-only --quiet >nul 2>&1
        if errorlevel 1 (
            echo          [FAIL] pull failed ^(skipped^)
            set /a FAILED+=1
        ) else (
            echo          [OK] pulled
            set /a PULLED+=1
        )
    ) else (
        git clone --quiet "%%R" "!REPO_PATH!" >nul 2>&1
        if errorlevel 1 (
            echo          [FAIL] clone failed ^(skipped^)
            set /a FAILED+=1
        ) else (
            echo          [OK] cloned
            set /a CLONED+=1
        )
    )
)

:prompt_orgs
echo.
:prompt_orgs_choice
set "INCLUDE_ORGS="
set /p INCLUDE_ORGS="Also download organization repositories? (y/n): "

if /i "%INCLUDE_ORGS%"=="y" (
    set "ORG_NAMES="
    set /p ORG_NAMES="Organization name(s), space-separated: "
    if not defined ORG_NAMES (
        echo [ERROR] No organization names given; skipping.
        set "ORGS_STATUS=SKIPPED"
        goto :cleanup_and_summary
    )
    set "ORGS_RAN=1"
    echo [INFO] Downloading organization repositories into '%TARGET_DIR%'
    set "VNCLI_TARGET_DIR=%TARGET_DIR%"
    call "%~dp0download_all_orgs.bat" !ORG_NAMES!
    if errorlevel 1 (
        set "ORGS_STATUS=FAILED"
    ) else (
        set "ORGS_STATUS=OK"
    )
    goto :cleanup_and_summary
)

if /i "%INCLUDE_ORGS%"=="n" (
    set "ORGS_STATUS=SKIPPED"
    goto :cleanup_and_summary
)

echo [ERROR] Invalid choice. Please enter 'y' or 'n'.
goto :prompt_orgs_choice

:cleanup_and_summary
echo.
echo ----------------------------------------
echo  Done. cloned=%CLONED% pulled=%PULLED% failed=%FAILED%
if "%ORGS_RAN%"=="1" echo        org_sync=%ORGS_STATUS%
echo ----------------------------------------

if exist "%TMP_BASE%" rmdir /s /q "%TMP_BASE%" >nul 2>nul
endlocal
exit /b 0
