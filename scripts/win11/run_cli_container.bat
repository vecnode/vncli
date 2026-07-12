@echo off
setlocal EnableExtensions

REM ---------------------------------------------------------------------------
REM run_cli_container.bat
REM Build and run the vncli CLI container in interactive mode.
REM ---------------------------------------------------------------------------

for %%I in ("%~dp0..\..") do set "REPO_ROOT=%%~fI"
set "DOCKERFILE_PATH=%REPO_ROOT%\docker\Dockerfile"
set "BUILD_CONTEXT=%REPO_ROOT%"
set "IMAGE_NAME=vncli-cli:latest"
set "CONTAINER_NAME=vncli-cli-session"





echo [INFO] Repository root: %REPO_ROOT%
echo [INFO] Dockerfile: %DOCKERFILE_PATH%
echo [INFO] Build context: %BUILD_CONTEXT%
echo [INFO] Image: %IMAGE_NAME%
echo.

where docker >nul 2>nul
if errorlevel 1 (
    echo [ERROR] Docker is not available or not in PATH
    exit /b 1
)

docker info >nul 2>nul
if errorlevel 1 (
    echo [ERROR] Docker daemon is not running
    exit /b 1
)

if not exist "%DOCKERFILE_PATH%" (
    echo [ERROR] Dockerfile not found: %DOCKERFILE_PATH%
    exit /b 1
)

echo [INFO] Building image...
docker build -t %IMAGE_NAME% -f "%DOCKERFILE_PATH%" "%BUILD_CONTEXT%"
if errorlevel 1 exit /b 1

echo.
echo [INFO] Starting container in interactive mode...
echo [INFO] Container name: %CONTAINER_NAME%
echo [INFO] Opening shell: /bin/bash
echo [INFO] Tools CLI command: bash /app/scripts/tools-cli/alpine/main.sh
echo [INFO] vncli CLI command: bash /app/scripts/ubuntu22/main.sh
echo.

docker rm -f %CONTAINER_NAME% >nul 2>nul

docker run --rm -it --name %CONTAINER_NAME% --entrypoint /bin/bash %IMAGE_NAME%
exit /b %errorlevel%
