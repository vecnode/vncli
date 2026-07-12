Windows 11 dotfiles

Running `setup_dotfiles.bat` will:

- Request Administrator privileges through UAC (and relaunch itself as admin when needed).
- Ensure the user SSH folder exists at `%USERPROFILE%\.ssh`.
- Backup any existing SSH config to a randomized backup file.
- Copy `dotfiles/win11/ssh/config` to `%USERPROFILE%\.ssh\config`.
- Execute `global_configs.ps1` to apply Windows user and system preferences.

`global_configs.ps1` applies a full baseline that includes:

- Configure TIPC by setting `HKCU\SOFTWARE\Microsoft\Input\TIPC\Enabled` to `0`.
- Configure language privacy by setting `HKCU\Control Panel\International\User Profile\HttpAcceptLanguageOptOut` to `1`.
- Configure Settings ad suppression by setting `SubscribedContent-338393Enabled` to `0`.
- Configure Settings tip suppression by setting `SubscribedContent-338394Enabled` to `0`.
- Configure Settings promotional banner suppression by setting `SubscribedContent-338396Enabled` to `0`.
- Configure text personalization restriction by setting `RestrictImplicitTextCollection` to `1`.
- Configure ink personalization restriction by setting `RestrictImplicitInkCollection` to `1`.
- Configure contact harvesting restriction by setting `HarvestContacts` to `0`.
- Configure personalization policy reset by setting `AcceptedPrivacyPolicy` to `0`.
- Configure feedback prompt suppression by setting `HKCU\SOFTWARE\Microsoft\Siuf\Rules\NumberOfSIUFInPeriod` to `0`.
- Configure Start menu suggestion suppression by setting `SubscribedContent-338388Enabled` to `0`.
- Configure diagnostic telemetry minimum level by setting `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection\AllowTelemetry` to `1` when running as Administrator.
- Configure Explorer to show file extensions by setting `HideFileExt` to `0`.
- Configure Explorer network thumbnail policy by setting `DisableThumbnailsOnNetworkFolders` to `1`.
- Configure Start search to disable Bing web results by setting `BingSearchEnabled` to `0`.
- Configure app removal by uninstalling `Microsoft.BingFinance` for users and provisioned images.
- Configure app removal by uninstalling `Microsoft.BingNews` for users and provisioned images.
- Configure app removal by uninstalling `Microsoft.BingSports` for users and provisioned images.
- Configure app removal by uninstalling `Microsoft.BingWeather` for users and provisioned images.
- Configure app removal by uninstalling `Microsoft.MicrosoftOfficeHub` for users and provisioned images.
- Configure app removal by uninstalling `Microsoft.GetStarted` for users and provisioned images.
- Configure consumer feature blocking by setting `HKLM\Software\Policies\Microsoft\Windows\CloudContent\DisableWindowsConsumerFeatures` to `1` when running as Administrator.
- Configure Defender cloud protection by setting `MAPSReporting` to `0`.
- Configure Defender sample upload policy by setting `SubmitSamplesConsent` to `2`.
- Configure Explorer to show hidden files by setting `Hidden` to `1`.
- Configure Explorer title behavior by setting `CabinetState\FullPath` to `1`.
- Configure Narrator shortcut behavior by setting `WinEnterLaunchEnabled` to `0`.
- Configure extra CloudContent hardening by setting `DisableCloudOptimizedContent` to `1` when running as Administrator.
- Configure extra CloudContent hardening by setting `DisableConsumerAccountStateContent` to `1` when running as Administrator.
- Configure AC sleep timeout to never by running `powercfg /change standby-timeout-ac 0` when running as Administrator.
- Configure battery sleep timeout to never by running `powercfg /change standby-timeout-dc 0` when running as Administrator.
- Configure AC monitor timeout to never by running `powercfg /change monitor-timeout-ac 0` when running as Administrator.
- Configure battery monitor timeout to never by running `powercfg /change monitor-timeout-dc 0` when running as Administrator.
- Configure hibernation to disabled by running `powercfg /hibernate off` when running as Administrator.
- Configure screensaver state by setting `ScreenSaveActive` to `0`.
- Configure screensaver unlock behavior by setting `ScreenSaverIsSecure` to `0`.
- Configure desktop wallpaper source by generating and assigning a `black.bmp` file in `%APPDATA%\Microsoft\Windows\Themes`.
- Configure desktop solid background color by setting `HKCU\Control Panel\Colors\Background` to `0 0 0`.
- Configure wallpaper mode by setting `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Wallpapers\BackgroundType` to `1`.
- Configure immediate wallpaper application by calling `SystemParametersInfo` with `SPI_SETDESKWALLPAPER`.