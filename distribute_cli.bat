@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM ---------------------------------------------------------------------------
REM distribute_cli.bat
REM Build vn in release mode and package a self-contained, drop-in-runnable
REM copy of vncli into a folder on the Desktop - same functionality as
REM launching from the repo via run_cli.bat, just relocatable and with no
REM Rust toolchain required on the machine it's copied to.
REM ---------------------------------------------------------------------------

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

set "VN_VERSION="
for /f "tokens=2 delims== " %%V in ('findstr /B /C:"version" cli\crates\vn\Cargo.toml') do (
	set "VN_VERSION=%%V"
)
set "VN_VERSION=%VN_VERSION:"=%"
set "VN_VERSION=%VN_VERSION: =%"
if not defined VN_VERSION set "VN_VERSION=0.0.0"

echo [INFO] Building vn CLI (release) for host target %RUST_HOST%...
cargo build --release --manifest-path cli\Cargo.toml -p vn --target "%RUST_HOST%"
if errorlevel 1 (
	echo [ERROR] Build failed.
	popd >nul
	pause
	exit /b 1
)

set "VN_BIN=cli\target\%RUST_HOST%\release\vn.exe"
if not exist "%VN_BIN%" (
	echo [ERROR] Binary not found: %VN_BIN%
	popd >nul
	pause
	exit /b 1
)

set "DIST_NAME=vncli-%VN_VERSION%-%RUST_HOST%"
set "DIST_DIR=%USERPROFILE%\Desktop\%DIST_NAME%"

if exist "%DIST_DIR%" (
	echo [INFO] Removing previous distribution at "%DIST_DIR%"...
	rmdir /s /q "%DIST_DIR%" >nul 2>nul
)
mkdir "%DIST_DIR%\cli\target\%RUST_HOST%\debug" >nul 2>nul
if errorlevel 1 (
	echo [ERROR] Could not create distribution folder: %DIST_DIR%
	popd >nul
	pause
	exit /b 1
)

echo [INFO] Packaging distribution at "%DIST_DIR%"...

REM run_cli_dist.bat.tmpl expects the binary at cli\target\<host>\debug\vn.exe
REM (matching where run_cli.bat looks in the dev repo); ship the release
REM binary in that same slot so no path changes are needed at launch time.
copy /Y "%VN_BIN%" "%DIST_DIR%\cli\target\%RUST_HOST%\debug\vn.exe" >nul

copy /Y "README.md" "%DIST_DIR%\README.md" >nul
copy /Y "LICENSE" "%DIST_DIR%\LICENSE" >nul
xcopy /E /I /Q /Y "scripts" "%DIST_DIR%\scripts" >nul
xcopy /E /I /Q /Y "docker" "%DIST_DIR%\docker" >nul
xcopy /E /I /Q /Y "docs" "%DIST_DIR%\docs" >nul

powershell -NoProfile -Command ^
	"(Get-Content -Raw 'run_cli_dist.bat.tmpl') -replace '__RUST_HOST__', '%RUST_HOST%' | Set-Content -NoNewline -Encoding ascii '%DIST_DIR%\run_cli.bat'"
if errorlevel 1 (
	echo [ERROR] Failed to generate run_cli.bat for the distribution.
	popd >nul
	pause
	exit /b 1
)

popd >nul

echo.
echo ----------------------------------------
echo  Distribution ready: %DIST_DIR%
echo  Run it with: %DIST_DIR%\run_cli.bat
echo ----------------------------------------
exit /b 0
