@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM ---------------------------------------------------------------------------
REM Unblock scripts: strip Zone.Identifier (Mark of the Web) from all .bat
REM and .sh files under ./scripts/ so Windows Defender stops flagging them as
REM untrusted internet downloads. Unblock-File does not require admin rights -
REM any user can unblock files they own. Safe and idempotent.
REM ---------------------------------------------------------------------------
powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-ChildItem -Path '%~dp0scripts' -Filter '*.bat' -Recurse -ErrorAction SilentlyContinue | Unblock-File -ErrorAction SilentlyContinue; Get-ChildItem -Path '%~dp0scripts' -Filter '*.sh' -Recurse -ErrorAction SilentlyContinue | Unblock-File -ErrorAction SilentlyContinue" >nul 2>nul

REM Always run relative to this file's directory (repo root expected).
pushd "%~dp0" >nul 2>nul
if errorlevel 1 (
	echo [ERROR] Unable to enter script directory.
	pause
	exit /b 1
)

where cargo >nul 2>nul
if errorlevel 1 (
	echo [ERROR] cargo not found in PATH.
	echo Install Rust first: https://rustup.rs/
	popd >nul
	pause
	exit /b 1
)

set "RUST_HOST="
for /f "tokens=1,* delims=:" %%A in ('rustc -vV ^| findstr /B /C:"host:"') do set "RUST_HOST=%%B"
for /f "tokens=* delims= " %%H in ("%RUST_HOST%") do set "RUST_HOST=%%H"

if not defined RUST_HOST (
	echo [ERROR] Unable to detect rustc host target.
	echo Run "rustc -vV" and ensure Rust is installed correctly.
	popd >nul
	pause
	exit /b 1
)

set "VN_BIN=.\cli\target\%RUST_HOST%\debug\vn.exe"
set "REPO_ROOT=%~dp0"
if "%REPO_ROOT:~-1%"=="\" set "REPO_ROOT=%REPO_ROOT:~0,-1%"

echo [INFO] Building vn CLI for host target %RUST_HOST%...
cargo build --manifest-path cli/Cargo.toml -p vn --target "%RUST_HOST%"
if errorlevel 1 (
	echo [ERROR] Build failed.
	popd >nul
	pause
	exit /b 1
)

if not exist "%VN_BIN%" (
	echo [ERROR] Binary not found: %VN_BIN%
	popd >nul
	pause
	exit /b 1
)

REM RustScan powers "vn net scan" (open-port scanning). Install it once via
REM cargo if it is not already on PATH; failure is non-fatal so the CLI still
REM launches.
where rustscan >nul 2>nul
if errorlevel 1 (
	echo [INFO] rustscan not found. Installing via cargo ^(one-time^)...
	cargo install rustscan
	if errorlevel 1 (
		echo [WARNING] Failed to install rustscan. "vn net scan" will not work until it is installed.
	)
)

echo [INFO] Launching vn...
echo [INFO] Starting vncli tray icon...
set "TRAY_BIN=%TEMP%\vncli-vn-tray-%RUST_HOST%.exe"
copy /Y "%VN_BIN%" "%TRAY_BIN%" >nul 2>nul
if errorlevel 1 (
	echo [WARNING] Could not prepare tray binary copy. Falling back to main executable.
	start "vncli tray" /MIN "%VN_BIN%" --repo-root "%REPO_ROOT%" tray
) else (
	start "vncli tray" /MIN "%TRAY_BIN%" --repo-root "%REPO_ROOT%" tray
)

"%VN_BIN%" --repo-root "%REPO_ROOT%" %*
set "VN_EXIT=%ERRORLEVEL%"

popd >nul
if not "%VN_EXIT%"=="0" (
	echo [ERROR] vn exited with code %VN_EXIT%.
	pause
)

exit /b %VN_EXIT%
