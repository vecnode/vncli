# Global Windows 11 Configs
# Run as current user - no hardcoded usernames, HKCU: maps to whoever is running this.

# ---------------------------------------------------------------------------
# Disable key logging and transmission to Microsoft (TIPC)
# TIPC (Text Intelligence Platform Client) collects what you type and sends it
# to Microsoft for Bing, autocomplete, and typing suggestions. Setting
# Enabled = 0 stops the collection and upload.
# ---------------------------------------------------------------------------
$tipcPath = "HKCU:\SOFTWARE\Microsoft\Input\TIPC"
if (!(Test-Path $tipcPath)) {
    New-Item -Path $tipcPath -Force | Out-Null
}
Set-ItemProperty -Path $tipcPath -Name "Enabled" -Value 0 -Type DWord
Write-Host "[INFO] Disabled TIPC key logging transmission."

# ---------------------------------------------------------------------------
# Opt out of language list exposure to websites
# Windows can pass your Accept-Language list to websites via the Windows API.
# This is a fingerprinting vector — your specific language order is a stable
# signal trackers use to identify you across sites even without cookies.
# Note: does not affect browser-level Accept-Language headers.
# ---------------------------------------------------------------------------
$langProfilePath = "HKCU:\Control Panel\International\User Profile"
Set-ItemProperty -Path $langProfilePath -Name "HttpAcceptLanguageOptOut" -Value 1 -Type DWord
Write-Host "[INFO] Opted out of language list exposure to websites."

# ---------------------------------------------------------------------------
# Disable suggested content (ads) inside the Windows Settings app
# 338393 - Suggested apps in Settings > Apps
# 338394 - Tips and suggestions in Settings > System > Notifications
# 338396 - Promotional banners inside Settings panels ("Get more out of Windows")
# ---------------------------------------------------------------------------
$cdmPath = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\ContentDeliveryManager"
Set-ItemProperty -Path $cdmPath -Name "SubscribedContent-338393Enabled" -Value 0 -Type DWord
Set-ItemProperty -Path $cdmPath -Name "SubscribedContent-338394Enabled" -Value 0 -Type DWord
Set-ItemProperty -Path $cdmPath -Name "SubscribedContent-338396Enabled" -Value 0 -Type DWord
Write-Host "[INFO] Disabled suggested content in Settings app."

# ---------------------------------------------------------------------------
# Disable "Getting to know me" - Speech, Inking & Typing personalization
# Windows passively collects your typed text, vocabulary, contacts, and
# handwriting to build a personal dictionary used by autocomplete, handwriting
# recognition, and Cortana. These two keys disable both text and ink harvesting.
# ---------------------------------------------------------------------------
$inputPersonPath = "HKCU:\SOFTWARE\Microsoft\InputPersonalization"
if (!(Test-Path $inputPersonPath)) {
    New-Item -Path $inputPersonPath -Force | Out-Null
}
Set-ItemProperty -Path $inputPersonPath -Name "RestrictImplicitTextCollection" -Value 1 -Type DWord
Set-ItemProperty -Path $inputPersonPath -Name "RestrictImplicitInkCollection" -Value 1 -Type DWord

$trainedDataPath = "HKCU:\SOFTWARE\Microsoft\InputPersonalization\TrainedDataStore"
if (!(Test-Path $trainedDataPath)) {
    New-Item -Path $trainedDataPath -Force | Out-Null
}
Set-ItemProperty -Path $trainedDataPath -Name "HarvestContacts" -Value 0 -Type DWord

$personSettingsPath = "HKCU:\SOFTWARE\Microsoft\Personalization\Settings"
if (!(Test-Path $personSettingsPath)) {
    New-Item -Path $personSettingsPath -Force | Out-Null
}
Set-ItemProperty -Path $personSettingsPath -Name "AcceptedPrivacyPolicy" -Value 0 -Type DWord
Write-Host "[INFO] Disabled Speech, Inking and Typing personalization collection."

# ---------------------------------------------------------------------------
# Disable Windows feedback prompts (SIUF)
# SIUF (System Initiated User Feedback) is the mechanism behind "How are you
# enjoying Windows?" and similar survey popups. Setting NumberOfSIUFInPeriod
# to 0 sets the allowed feedback prompts per period to zero, silencing them.
# ---------------------------------------------------------------------------
$siufPath = "HKCU:\SOFTWARE\Microsoft\Siuf\Rules"
if (!(Test-Path $siufPath)) {
    New-Item -Path $siufPath -Force | Out-Null
}
Set-ItemProperty -Path $siufPath -Name "NumberOfSIUFInPeriod" -Value 0 -Type DWord
Write-Host "[INFO] Disabled Windows feedback prompts (SIUF)."

# ---------------------------------------------------------------------------
# Disable Start menu app suggestions (sponsored tiles and promoted apps)
# 338388 controls "Occasionally show suggestions in Start" - the paid app
# promotions that appear as tiles in the Start menu.
# ---------------------------------------------------------------------------
Set-ItemProperty -Path $cdmPath -Name "SubscribedContent-338388Enabled" -Value 0 -Type DWord
Write-Host "[INFO] Disabled Start menu app suggestions."

# ---------------------------------------------------------------------------
# Set diagnostic telemetry to Basic (minimum allowed on Home/Pro)
# 0 = Security only (Enterprise/Education only, ignored on Home/Pro)
# 1 = Basic (device info and crash data only) <-- minimum for Home/Pro
# 3 = Full (Windows default)
# NOTE: This key is in HKLM and requires Administrator rights.
#       If not running as admin this will fail silently.
# ---------------------------------------------------------------------------
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if ($isAdmin) {
    $telemetryPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection"
    Set-ItemProperty -Path $telemetryPath -Name "AllowTelemetry" -Value 1 -Type DWord
    Write-Host "[INFO] Set diagnostic telemetry to Basic (minimum level)."
} else {
    Write-Host "[WARNING] Skipped telemetry setting - requires Administrator rights. Re-run as admin to apply."
}

# ---------------------------------------------------------------------------
# Show file extensions in Explorer
# Windows hides extensions by default (e.g. "document" instead of "document.pdf").
# Setting HideFileExt = 0 forces Explorer to always show them.
# Security benefit: prevents the common "invoice.pdf.exe" trick where malware
# disguises itself behind a visible-looking extension.
# ---------------------------------------------------------------------------
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "HideFileExt" -Value 0 -Type DWord
Write-Host "[INFO] File extensions are now visible in Explorer."

# ---------------------------------------------------------------------------
# Disable Thumbs.db creation on network volumes
# Explorer caches thumbnail previews in Thumbs.db files inside each folder.
# On network/shared drives this pollutes shared folders, causes permission
# errors, and interferes with backup tools. Local caching is unaffected.
# ---------------------------------------------------------------------------
$explorerPoliciesPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer"
if (!(Test-Path $explorerPoliciesPath)) {
    New-Item -Path $explorerPoliciesPath -Force | Out-Null
}
Set-ItemProperty -Path $explorerPoliciesPath -Name "DisableThumbnailsOnNetworkFolders" -Value 1 -Type DWord
Write-Host "[INFO] Disabled Thumbs.db creation on network volumes."

# ---------------------------------------------------------------------------
# Disable Bing in Start menu search
# By default, Start menu search sends queries to Bing and shows web results.
# Setting BingSearchEnabled = 0 makes search local-only — faster, no queries
# sent to Microsoft.
# ---------------------------------------------------------------------------
$searchPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search"
if (!(Test-Path $searchPath)) {
    New-Item -Path $searchPath -Force | Out-Null
}
Set-ItemProperty -Path $searchPath -Name "BingSearchEnabled" -Value 0 -Type DWord
Write-Host "[INFO] Disabled Bing in Start menu search."

# ---------------------------------------------------------------------------
# Remove preinstalled Bing/bloatware apps
# Each app is removed for all existing users AND from the Windows image so it
# won't reinstall for new user accounts. Both lines per app are required.
# Requires no admin for AppxPackage removal, but provisioned removal needs admin.
# ---------------------------------------------------------------------------
$bloatApps = @(
    "Microsoft.BingFinance",
    "Microsoft.BingNews",
    "Microsoft.BingSports",
    "Microsoft.BingWeather",
    "Microsoft.MicrosoftOfficeHub",
    "Microsoft.GetStarted"
)

foreach ($app in $bloatApps) {
    Get-AppxPackage $app -AllUsers -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue
    Get-AppXProvisionedPackage -Online -ErrorAction SilentlyContinue | Where-Object DisplayName -like $app | Remove-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue
    Write-Host "[INFO] Removed $app."
}

# ---------------------------------------------------------------------------
# Prevent Windows from silently reinstalling suggested/promoted apps
# Without this, CloudContent will restore bloatware even after uninstalling.
# DisableWindowsConsumerFeatures blocks that reinstall mechanism entirely.
# Requires Administrator rights (HKLM).
# ---------------------------------------------------------------------------
if ($isAdmin) {
    $cloudContentPath = "HKLM:\Software\Policies\Microsoft\Windows\CloudContent"
    if (!(Test-Path $cloudContentPath)) {
        New-Item -Path $cloudContentPath -Force | Out-Null
    }
    Set-ItemProperty -Path $cloudContentPath -Name "DisableWindowsConsumerFeatures" -Value 1 -Type DWord
    Write-Host "[INFO] Disabled Windows consumer features (auto app reinstall)."
} else {
    Write-Host "[WARNING] Skipped DisableWindowsConsumerFeatures - requires Administrator rights. Re-run as admin to apply."
}

# ---------------------------------------------------------------------------
# Disable Windows Defender cloud-based protection and sample submission
# MAPSReporting = 0: Defender works locally only, no file hashes sent to
# Microsoft's cloud (MAPS) for real-time verdicts.
# SubmitSamplesConsent = 2: Suspicious files are never uploaded to Microsoft.
# Tradeoff: you lose zero-day cloud intelligence but gain full privacy.
# Signature-based (known threat) scanning remains fully active.
# ---------------------------------------------------------------------------
Set-MpPreference -MAPSReporting 0
Write-Host "[INFO] Disabled Defender cloud-based protection (MAPS)."
Set-MpPreference -SubmitSamplesConsent 2
Write-Host "[INFO] Disabled automatic sample submission to Microsoft."

# ---------------------------------------------------------------------------
# Show hidden files in Explorer
# Hidden = 1 displays files with the H attribute (like .bashrc, .gitignore).
# By default these are invisible. Safe and useful for developers.
# ---------------------------------------------------------------------------
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" -Name "Hidden" -Value 1 -Type DWord
Write-Host "[INFO] Hidden files are now visible in Explorer."

# ---------------------------------------------------------------------------
# Show full path in Explorer title bar
# By default, Explorer title shows just the folder name ("Desktop").
# FullPath = 1 shows the full path ("C:\Users\user\Desktop").
# ---------------------------------------------------------------------------
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState" -Name "FullPath" -Value 1 -Type DWord
Write-Host "[INFO] Full path now displayed in Explorer title bar."

# ---------------------------------------------------------------------------
# Disable Windows Narrator hotkey (Win+Enter)
# WinEnterLaunchEnabled = 0 disables the hotkey that launches Narrator
# (screen reader/text-to-speech). Prevents accidental activation.
# ---------------------------------------------------------------------------
Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Narrator\NoRoam" -Name "WinEnterLaunchEnabled" -Value 0 -Type DWord
Write-Host "[INFO] Disabled Windows Narrator hotkey (Win+Enter)."

# ---------------------------------------------------------------------------
# Extra guards against Windows consumer features / app reinstall
# These are belt-and-suspenders alongside DisableWindowsConsumerFeatures
# already set earlier. All three together provide maximum protection against
# involuntary app reinstalls from CloudContent.
# Requires Administrator rights (HKLM).
# ---------------------------------------------------------------------------
if ($isAdmin) {
    $cloudContentPath = "HKLM:\Software\Policies\Microsoft\Windows\CloudContent"
    if (!(Test-Path $cloudContentPath)) {
        New-Item -Path $cloudContentPath -Force | Out-Null
    }
    Set-ItemProperty -Path $cloudContentPath -Name "DisableCloudOptimizedContent" -Value 1 -Type DWord
    Set-ItemProperty -Path $cloudContentPath -Name "DisableConsumerAccountStateContent" -Value 1 -Type DWord
    Write-Host "[INFO] Added extra CloudContent protection (DisableCloudOptimizedContent, DisableConsumerAccountStateContent)."
}

# ---------------------------------------------------------------------------
# Block OneDrive from syncing and starting setup
# DisableFileSyncNGSC is the official "Prevent the usage of OneDrive for file
# storage" policy — OneDrive won't start, sync, or integrate with Explorer.
# DisableLibrariesDefaultSaveToOneDrive stops Windows from defaulting saves to
# OneDrive. DisableAutoConfig blocks silent sign-in during setup prompts.
# HKCU keys hide OneDrive from Explorer and remove the startup Run entry.
# Requires Administrator rights for HKLM policies (HKCU parts apply either way).
# ---------------------------------------------------------------------------
$oneDrivePolicyPath = "HKLM:\SOFTWARE\Policies\Microsoft\Windows\OneDrive"
$oneDriveSyncPolicyPath = "HKLM:\SOFTWARE\Policies\Microsoft\OneDrive"
if ($isAdmin) {
    if (!(Test-Path $oneDrivePolicyPath)) {
        New-Item -Path $oneDrivePolicyPath -Force | Out-Null
    }
    Set-ItemProperty -Path $oneDrivePolicyPath -Name "DisableFileSyncNGSC" -Value 1 -Type DWord
    Set-ItemProperty -Path $oneDrivePolicyPath -Name "DisableLibrariesDefaultSaveToOneDrive" -Value 1 -Type DWord

    if (!(Test-Path $oneDriveSyncPolicyPath)) {
        New-Item -Path $oneDriveSyncPolicyPath -Force | Out-Null
    }
    Set-ItemProperty -Path $oneDriveSyncPolicyPath -Name "DisableAutoConfig" -Value 1 -Type DWord

    Get-Process -Name "OneDrive" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Write-Host "[INFO] Blocked OneDrive sync (DisableFileSyncNGSC, default-save, auto-config)."
} else {
    Write-Host "[WARNING] Skipped OneDrive sync block - requires Administrator rights. Re-run as admin to apply."
}

$oneDriveNamespacePath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\{018D5C66-4533-4307-9B53-224DE2ED1FE6}"
if (!(Test-Path $oneDriveNamespacePath)) {
    New-Item -Path $oneDriveNamespacePath -Force | Out-Null
}
Set-ItemProperty -Path $oneDriveNamespacePath -Name "System.IsPinnedToNameSpaceTree" -Value 0 -Type DWord

$runPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
if (Get-ItemProperty -Path $runPath -Name "OneDrive" -ErrorAction SilentlyContinue) {
    Remove-ItemProperty -Path $runPath -Name "OneDrive" -ErrorAction SilentlyContinue
}
Write-Host "[INFO] Hidden OneDrive from Explorer and removed startup entry."

# ---------------------------------------------------------------------------
# Never sleep, never turn off monitor
# Sets sleep and monitor timeouts to 0 (never) for both AC and battery power.
# Requires Administrator rights (powercfg writes power scheme settings).
# ---------------------------------------------------------------------------
if ($isAdmin) {
    powercfg /change standby-timeout-ac 0
    powercfg /change standby-timeout-dc 0
    powercfg /change monitor-timeout-ac 0
    powercfg /change monitor-timeout-dc 0
    Write-Host "[INFO] Sleep and monitor timeout disabled (AC and battery)."
    
    # Disable hibernation: prevents Windows from writing entire RAM to hiberfil.sys
    # On modern SSDs, regular sleep (suspend-to-RAM) is fast enough. Hibernation
    # is a legacy HDD feature; disabling it frees several GB of disk space.
    powercfg /hibernate off
    Write-Host "[INFO] Hibernation disabled."
} else {
    Write-Host "[WARNING] Skipped sleep settings - requires Administrator rights."
}

# ---------------------------------------------------------------------------
# Disable screensaver
# ScreenSaveActive = 0 turns the screensaver off entirely.
# ScreenSaverIsSecure = 0 removes the lock-on-resume requirement.
# ---------------------------------------------------------------------------
Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "ScreenSaveActive" -Value "0" -Type String
Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "ScreenSaverIsSecure" -Value "0" -Type String
Write-Host "[INFO] Screensaver disabled."

# ---------------------------------------------------------------------------
# Set desktop background to solid black
# Windows 11 requires an actual image file for SystemParametersInfo to apply
# the change immediately. We generate a 1x1 black BMP via System.Drawing,
# save it to the Themes folder, point the wallpaper registry at it, set
# BackgroundType = 1 (solid color in Personalization), then call
# SystemParametersInfo to apply it in the current session without logoff.
# ---------------------------------------------------------------------------
Add-Type -AssemblyName System.Drawing

$themesPath = "$env:APPDATA\Microsoft\Windows\Themes"
if (!(Test-Path $themesPath)) {
    New-Item -Path $themesPath -Force | Out-Null
}
$blackBmpPath = Join-Path $themesPath "black.bmp"

$bmp = New-Object System.Drawing.Bitmap(1, 1)
$bmp.SetPixel(0, 0, [System.Drawing.Color]::Black)
$bmp.Save($blackBmpPath, [System.Drawing.Imaging.ImageFormat]::Bmp)
$bmp.Dispose()

Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "Wallpaper" -Value $blackBmpPath -Type String
Set-ItemProperty -Path "HKCU:\Control Panel\Colors" -Name "Background" -Value "0 0 0" -Type String
$wallpapersPath = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Wallpapers"
if (!(Test-Path $wallpapersPath)) {
    New-Item -Path $wallpapersPath -Force | Out-Null
}
Set-ItemProperty -Path $wallpapersPath -Name "BackgroundType" -Value 1 -Type DWord

if (-not ([System.Management.Automation.PSTypeName]'Desktop').Type) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class Desktop {
    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    public static extern int SystemParametersInfo(int uAction, int uParam, string lpvParam, int fuWinIni);
}
"@
}
# SPI_SETDESKWALLPAPER=0x14, SPIF_UPDATEINIFILE=0x01, SPIF_SENDCHANGE=0x02
[Desktop]::SystemParametersInfo(0x0014, 0, $blackBmpPath, 0x01 -bor 0x02) | Out-Null
Write-Host "[INFO] Desktop background set to solid black (applied immediately)."


# ---------------------------------------------------------------------------
# Enable Win32 long paths support (helps Unreal UAT/UBT packaging on deep paths)
# Windows legacy MAX_PATH is 260 chars. Setting LongPathsEnabled = 1 allows
# long path-aware tools to go beyond that limit.
# Requires Administrator rights (HKLM).
# Note: Reboot is recommended after applying.
# ---------------------------------------------------------------------------
if ($isAdmin) {
    $fileSystemPath = "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem"
    if (!(Test-Path $fileSystemPath)) {
        New-Item -Path $fileSystemPath -Force | Out-Null
    }
    Set-ItemProperty -Path $fileSystemPath -Name "LongPathsEnabled" -Value 1 -Type DWord
    Write-Host "[INFO] Enabled Win32 long paths (LongPathsEnabled=1). Reboot recommended."
} else {
    Write-Host "[WARNING] Skipped long paths setting - requires Administrator rights. Re-run as admin to apply."
}

# ---------------------------------------------------------------------------
# Disable scheduled Windows license validation tasks
# Windows uses Task Scheduler to re-check activation on a timer, at logon,
# and when network connectivity changes. Disabling these tasks stops those
# automatic re-validation cycles. Task state persists across reboots.
# Tradeoff: digital/KMS licenses may not renew automatically; activate once
# manually (Settings > Activation or slmgr /ato) before relying on this.
# Requires Administrator rights.
# ---------------------------------------------------------------------------
if ($isAdmin) {
    $licenseScheduledTasks = @(
        @{ Path = '\Microsoft\Windows\License Manager\'; Name = 'TempSignedLicenseExchange' },
        @{ Path = '\Microsoft\Windows\Clip\'; Name = 'License Validation' },
        @{ Path = '\Microsoft\Windows\Subscription\'; Name = 'EnableLicenseAcquisition' },
        @{ Path = '\Microsoft\Windows\Subscription\'; Name = 'LicenseAcquisition' },
        @{ Path = '\Microsoft\Windows\SoftwareProtectionPlatform\'; Name = 'SvcRestartTask' },
        @{ Path = '\Microsoft\Windows\SoftwareProtectionPlatform\'; Name = 'SvcRestartTaskLogon' },
        @{ Path = '\Microsoft\Windows\SoftwareProtectionPlatform\'; Name = 'SvcRestartTaskNetwork' }
    )

    foreach ($task in $licenseScheduledTasks) {
        $existing = Get-ScheduledTask -TaskPath $task.Path -TaskName $task.Name -ErrorAction SilentlyContinue
        if ($null -eq $existing) {
            continue
        }
        if ($existing.State -ne 'Disabled') {
            Disable-ScheduledTask -TaskPath $task.Path -TaskName $task.Name -ErrorAction SilentlyContinue | Out-Null
            Write-Host "[INFO] Disabled scheduled task: $($task.Path)$($task.Name)"
        }
    }
    Write-Host "[INFO] License validation scheduled tasks disabled (persists across reboot)."
} else {
    Write-Host "[WARNING] Skipped license scheduled-task disable - requires Administrator rights. Re-run as admin to apply."
}

# ---------------------------------------------------------------------------
# Block network-dependent Windows activation checks
# sppsvc (Software Protection Platform) phones home to Microsoft for online
# validation and KMS renewal. An outbound firewall rule for sppsvc.exe blocks
# those checks while leaving local license state untouched. Windows Firewall
# rules persist across reboots. sppsvc is set to Manual so it is not restarted
# automatically at boot by the service control manager alone.
# Tradeoff: online re-activation and KMS renewal will fail until this block
# is removed. Pair with the scheduled-task section above for full effect.
# Requires Administrator rights.
# ---------------------------------------------------------------------------
if ($isAdmin) {
    $sppServiceName = 'sppsvc'
    $sppProgramPath = Join-Path $env:SystemRoot 'System32\sppsvc.exe'
    $firewallRuleName = 'vecnode-block-sppsvc-outbound'

    $existingRule = Get-NetFirewallRule -DisplayName $firewallRuleName -ErrorAction SilentlyContinue
    if ($null -eq $existingRule) {
        New-NetFirewallRule `
            -DisplayName $firewallRuleName `
            -Description 'Block Software Protection Platform outbound activation traffic (vecnode dotfiles)' `
            -Direction Outbound `
            -Program $sppProgramPath `
            -Action Block `
            -Enabled True `
            -ErrorAction SilentlyContinue | Out-Null
        Write-Host "[INFO] Created outbound firewall block for sppsvc.exe."
    } else {
        Set-NetFirewallRule -DisplayName $firewallRuleName -Enabled True -Action Block -ErrorAction SilentlyContinue | Out-Null
        Write-Host "[INFO] Outbound firewall block for sppsvc.exe already present (re-enabled)."
    }

    Set-Service -Name $sppServiceName -StartupType Manual -ErrorAction SilentlyContinue
    Stop-Service -Name $sppServiceName -Force -ErrorAction SilentlyContinue
    Write-Host "[INFO] Blocked network-dependent activation checks (firewall + sppsvc Manual)."
} else {
    Write-Host "[WARNING] Skipped activation network block - requires Administrator rights. Re-run as admin to apply."
}

# vecnode 2026


