@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM ---------------------------------------------------------------------------
REM check_internet.bat
REM Plain-language internet connectivity check for the vncli TUI.
REM Tells the user, in clear terms, whether they are online and what was tested.
REM ---------------------------------------------------------------------------

set "ADAPTER_STATE=DOWN"
set "ADAPTER_NAME=none"
set "RX_HR=NA"
set "TX_HR=NA"
set "PING_OK=0"
set "DNS_OK=0"

REM Gather adapter state, the active adapter name, and human-readable data totals.
set "NET_TMP=%TEMP%\vncli-net-%RANDOM%-%RANDOM%.txt"
powershell -NoProfile -Command "$ErrorActionPreference='SilentlyContinue'; function HR($b){ $u='B','KB','MB','GB','TB'; $i=0; $v=[double]$b; while($v -ge 1024 -and $i -lt 4){$v/=1024;$i++}; ('{0:N1} {1}' -f $v,$u[$i]) }; $up=Get-NetAdapter | Where-Object {$_.Status -eq 'Up'}; if($up){$state='UP'; $name=($up | Select-Object -First 1 -ExpandProperty Name)}else{$state='DOWN'; $name='none'}; $stats=Get-NetAdapterStatistics; if($stats){$rx=HR([int64](($stats|Measure-Object -Property ReceivedBytes -Sum).Sum)); $tx=HR([int64](($stats|Measure-Object -Property SentBytes -Sum).Sum))}else{$rx='NA';$tx='NA'}; Write-Output ('ADAPTER_STATE='+$state); Write-Output ('ADAPTER_NAME='+$name); Write-Output ('RX_HR='+$rx); Write-Output ('TX_HR='+$tx)" > "%NET_TMP%" 2>nul

if exist "%NET_TMP%" (
    for /f "usebackq tokens=1,* delims==" %%K in ("%NET_TMP%") do (
        if /i "%%K"=="ADAPTER_STATE" set "ADAPTER_STATE=%%L"
        if /i "%%K"=="ADAPTER_NAME" set "ADAPTER_NAME=%%L"
        if /i "%%K"=="RX_HR" set "RX_HR=%%L"
        if /i "%%K"=="TX_HR" set "TX_HR=%%L"
    )
    del /q "%NET_TMP%" >nul 2>nul
)

ping -n 1 -w 1500 1.1.1.1 >nul 2>nul
if not errorlevel 1 set "PING_OK=1"

nslookup www.microsoft.com >nul 2>nul
if not errorlevel 1 set "DNS_OK=1"

echo.
echo [Internet Connection Check]
echo.

REM --- Plain-language verdict ---
if "%PING_OK%"=="1" if "%DNS_OK%"=="1" (
    echo   ==^> YES - you are connected to the internet.
    goto :verdict_done
)
if "%PING_OK%"=="1" (
    echo   ==^> PARTIAL - the internet is reachable, but website names are not
    echo       resolving. This usually means a DNS problem.
    goto :verdict_done
)
if "%DNS_OK%"=="1" (
    echo   ==^> PARTIAL - name lookups work, but the internet is not reachable.
    echo       This usually means a firewall or routing problem.
    goto :verdict_done
)
echo   ==^> NO - you are not connected to the internet.
:verdict_done

echo.
echo   What was checked:
if /i "%ADAPTER_STATE%"=="UP" (
    echo     - Network connection : Active ^(via "%ADAPTER_NAME%"^)
) else (
    echo     - Network connection : No active network adapter found
)
if "%PING_OK%"=="1" (
    echo     - Reach the internet : Yes ^(pinged 1.1.1.1^)
) else (
    echo     - Reach the internet : No ^(could not ping 1.1.1.1^)
)
if "%DNS_OK%"=="1" (
    echo     - Look up websites   : Yes ^(resolved www.microsoft.com^)
) else (
    echo     - Look up websites   : No ^(could not resolve www.microsoft.com^)
)

echo.
echo   Data used since last restart ^(all adapters combined^):
if /i "%RX_HR%"=="NA" (
    echo     - Could not read the data counters.
) else (
    echo     - Downloaded : %RX_HR%
    echo     - Uploaded   : %TX_HR%
)

echo.
endlocal
exit /b 0
