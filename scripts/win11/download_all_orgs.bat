@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM ---------------------------------------------------------------------------
REM download_all_orgs.bat
REM Clone all public repositories from a list of GitHub organizations.
REM
REM Usage:
REM   download_all_orgs.bat <org1> [org2 org3 ...]
REM   At least one organization name is required - there is no default list.
REM
REM Downloads into: %%VNCLI_TARGET_DIR%% or %%USERPROFILE%%\Desktop\git-backup-orgs-DD-MM-YYYY-HH-MM-SS\
REM Requirements (Windows):
REM   - git
REM   - curl
REM   - jq
REM ---------------------------------------------------------------------------

REM ---------------------------------------------------------------------------
REM CONFIGURATION
REM ---------------------------------------------------------------------------

set "PER_PAGE=100"
if "%~1"=="" (
    echo [ERROR] Usage: download_all_orgs.bat ^<org1^> [org2 org3 ...]
    exit /b 1
) else (
    set "ORG_LINKS=%*"
)

set "TS_FILE=%TEMP%\vncli-ts-%RANDOM%-%RANDOM%.txt"
powershell -NoProfile -Command "Get-Date -Format 'dd-MM-yyyy-HH-mm-ss'" > "%TS_FILE%"
set "TIMESTAMP="
if exist "%TS_FILE%" set /p TIMESTAMP=<"%TS_FILE%"
if exist "%TS_FILE%" del /q "%TS_FILE%" >nul 2>nul
if not defined TIMESTAMP set "TIMESTAMP=%RANDOM%-%RANDOM%"

if defined VNCLI_TARGET_DIR (
    set "TARGET_DIR=%VNCLI_TARGET_DIR%"
) else (
    set "TARGET_DIR=%USERPROFILE%\Desktop\git-backup-orgs-%TIMESTAMP%"
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
echo [INFO] Syncing hardcoded organizations into '%TARGET_DIR%'

set /a CLONED=0
set /a PULLED=0
set /a FAILED=0
set /a ORGS_COUNT=0
set /a ORG_REPOS_TOTAL=0
set /a ORGS_TOTAL=0

for %%O in (%ORG_LINKS%) do set /a ORGS_TOTAL+=1

set "TMP_BASE=%TEMP%\vncli-orgs-%RANDOM%-%RANDOM%"
if not exist "%TMP_BASE%" mkdir "%TMP_BASE%"

for %%O in (%ORG_LINKS%) do (
    set /a ORGS_COUNT+=1
    call :process_org "%%~O"
)

echo.
echo ----------------------------------------
echo  Done. cloned=%CLONED% pulled=%PULLED% failed=%FAILED%
echo        orgs=%ORGS_COUNT% org_repos=%ORG_REPOS_TOTAL%
echo ----------------------------------------

if exist "%TMP_BASE%" rmdir /s /q "%TMP_BASE%" >nul 2>nul
endlocal
exit /b 0

:process_org
set "ORG_NAME=%~1"
set "ORG_DIR=%TARGET_DIR%\%ORG_NAME%"
if not exist "%ORG_DIR%" mkdir "%ORG_DIR%"

echo.
echo [ORG %ORGS_COUNT%/%ORGS_TOTAL%] %ORG_NAME%

set "ORG_LIST_FILE=%TMP_BASE%\%ORG_NAME%-repos.txt"
if exist "%ORG_LIST_FILE%" del /q "%ORG_LIST_FILE%"

set /a PAGE=1

:org_page_loop
set "JSON_FILE=%TMP_BASE%\%ORG_NAME%-%PAGE%.json"

curl -fsSL -H "Accept: application/vnd.github+json" "https://api.github.com/orgs/%ORG_NAME%/repos?per_page=%PER_PAGE%^&page=%PAGE%^&type=public" > "%JSON_FILE%"
if errorlevel 1 (
    echo          [FAIL] API request failed for %ORG_NAME% ^(page %PAGE%^)
    set /a FAILED+=1
    goto :org_done
)

for /f %%B in ('jq -r ".[].clone_url" "%JSON_FILE%" ^| find /c /v ""') do set "BATCH_COUNT=%%B"
if "%BATCH_COUNT%"=="0" goto :org_done

jq -r ".[].clone_url" "%JSON_FILE%" >> "%ORG_LIST_FILE%"
if errorlevel 1 (
    echo          [FAIL] Failed parsing JSON for %ORG_NAME%
    set /a FAILED+=1
    goto :org_done
)

for /f %%C in ('jq "length" "%JSON_FILE%"') do set "COUNT=%%C"
if %COUNT% LSS %PER_PAGE% goto :org_done

set /a PAGE+=1
goto :org_page_loop

:org_done
if not exist "%ORG_LIST_FILE%" (
    echo          no public repositories
    exit /b 0
)

for /f %%T in ('find /c /v "" ^< "%ORG_LIST_FILE%"') do set "ORG_TOTAL=%%T"
if "%ORG_TOTAL%"=="0" (
    echo          no public repositories
    exit /b 0
)

set /a ORG_REPOS_TOTAL+=ORG_TOTAL
set /a ORG_IDX=0

for /f "usebackq delims=" %%R in ("%ORG_LIST_FILE%") do (
    set /a ORG_IDX+=1
    for %%N in (%%~nR) do set "REPO_NAME=%%~N"
    set "REPO_PATH=%ORG_DIR%\!REPO_NAME!"

    echo          [!ORG_IDX!/%ORG_TOTAL%] !REPO_NAME!

    if exist "!REPO_PATH!\.git" (
        git -C "!REPO_PATH!" pull --ff-only --quiet >nul 2>&1
        if errorlevel 1 (
            echo                   [FAIL] pull failed ^(skipped^)
            set /a FAILED+=1
        ) else (
            echo                   [OK] pulled
            set /a PULLED+=1
        )
    ) else (
        git clone --quiet "%%R" "!REPO_PATH!" >nul 2>&1
        if errorlevel 1 (
            echo                   [FAIL] clone failed ^(skipped^)
            set /a FAILED+=1
        ) else (
            echo                   [OK] cloned
            set /a CLONED+=1
        )
    )
)

exit /b 0
