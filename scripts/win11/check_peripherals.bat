@echo off
setlocal EnableExtensions EnableDelayedExpansion

REM ---------------------------------------------------------------------------
REM check_peripherals.bat
REM Show currently connected peripherals grouped by type (monitor, mouse,
REM keyboard, camera, audio, printer, bluetooth, storage) so the output reads
REM as clear rows instead of a flat list of device/vendor names.
REM ---------------------------------------------------------------------------

echo [Connected Peripherals]
echo.

REM Row prints each device under a category label. Monitor/Mouse/Keyboard are
REM always shown (with "(none detected)" when empty); other categories appear
REM only when something is connected, to keep the list short and readable.
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ErrorActionPreference='SilentlyContinue'; function Row($label,$classes,$always){ $d = Get-PnpDevice -PresentOnly -Class $classes -Status OK | Select-Object -ExpandProperty FriendlyName | Where-Object {$_} | Sort-Object -Unique; if($d){ foreach($x in $d){ '{0,-12}{1}' -f ($label+':'), $x } } elseif($always){ '{0,-12}{1}' -f ($label+':'), '(none detected)' } }; Row 'Monitor' 'Monitor' $true; Row 'Mouse' 'Mouse' $true; Row 'Keyboard' 'Keyboard' $true; Row 'Camera' @('Camera','Image') $false; Row 'Audio' 'AudioEndpoint' $false; Row 'Printer' 'Printer' $false; Row 'Bluetooth' 'Bluetooth' $false; Row 'Storage' 'DiskDrive' $false"

echo.
endlocal
exit /b %ERRORLEVEL%
