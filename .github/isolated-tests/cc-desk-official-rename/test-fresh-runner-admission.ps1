# Inspect only a dedicated disposable test session. Never modify network or registry settings.
function Assert-FreshRunner($Receipt) {
    if (!$IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or
        !$env:RUNNER_TEMP -or ![Environment]::UserInteractive -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'NOT_DISPOSABLE_INTERACTIVE_HOST' }
    $stash=[Environment]::GetFolderPath('UserProfile')
    $local=[Environment]::GetFolderPath('LocalApplicationData')
    if (!$stash -or !$local -or $env:USERPROFILE -ine $stash -or ($env:HOME -and $env:HOME -ine $stash) -or
        $env:LOCALAPPDATA -ine $local -or $env:APPDATA -ine [Environment]::GetFolderPath('ApplicationData') -or
        @(@($stash,$local,$env:RUNNER_TEMP) | Where-Object { (Get-Item -LiteralPath $_).Attributes -band [IO.FileAttributes]::ReparsePoint }).Count) { throw 'UNCLEAN_TEST_PROFILE' }
    foreach ($location in @(
        (Join-Path $stash '.cc-box'),(Join-Path $stash '.codex'),(Join-Path $local 'io.github.shawnwu2022.ccdesk'),(Join-Path $local 'CC Desk'),
        'HKCU:\Software\shawnwu2022\CC Desk','HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CC Desk',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CC Desk','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\CC Desk',
        'HKCU:\Software\Classes\Directory\shell\cc-desk','HKCU:\Software\Classes\Directory\Background\shell\cc-desk',
        'HKCU:\Software\Classes\Directory\shell\cc-box','HKCU:\Software\Classes\Directory\Background\shell\cc-box'
    )) { if (Test-Path -LiteralPath $location) { throw 'EXISTING_APP_OR_PROFILE' } }
    if (@(Get-Process -Name cc-desk,codex -ErrorAction SilentlyContinue).Count) { throw 'EXISTING_APP_OR_CLI' }
    $Receipt.cleanTestProfile=$true
    $vars=@(Get-ChildItem Env: | ForEach-Object Name)
    if (@($vars | Where-Object { $_ -match 'PROXY|^WEBVIEW2_|^(OPENAI_|CHATGPT_|CODEX_)' }).Count) { throw 'ENVIRONMENT_OVERRIDE_PRESENT' }
    $Receipt.proxyEnvironmentAbsent=$true
    if (@($vars | Where-Object { $_ -ne 'GITHUB_ACTIONS' -and $_ -match '^(ACTIONS_|GITHUB_|GH_)|TOKEN|SECRET|API_KEY|PASSWORD|CREDENTIAL' }).Count) { throw 'AUTHENTICATION_ENVIRONMENT_PRESENT' }
    # Fixed English response from the hosted US image; unknown/localized output fails closed.
    $winHttp=@(& (Join-Path $env:SystemRoot 'System32\netsh.exe') winhttp show proxy 2>$null)
    if ($LASTEXITCODE -ne 0 -or @($winHttp | Where-Object { $_ -match '^\s*Direct access \(no proxy server\)\.\s*$' }).Count -ne 1) { throw 'WINHTTP_NOT_DIRECT' }
    $Receipt.directWinHttp=$true
    $inet='HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    $settings=Get-Item -LiteralPath $inet
    if ([int]$settings.GetValue('ProxyEnable',0) -ne 0 -or [int]$settings.GetValue('AutoDetect',0) -ne 0 -or
        $settings.GetValue('ProxyServer') -or $settings.GetValue('AutoConfigURL')) { throw 'WININET_PROXY_OR_PAC' }
    $connections=Get-Item -LiteralPath (Join-Path $inet 'Connections') -ErrorAction SilentlyContinue
    if (!$connections) { throw 'WININET_CONNECTIONS_UNVERIFIED' }
    if ($connections) {
        $flags=$connections.GetValue('DefaultConnectionSettings')
        if ($flags -isnot [byte[]] -or $flags.Length -lt 12 -or [BitConverter]::ToUInt32($flags,8) -ne 1) { throw 'WININET_NOT_DIRECT' }
    }
    foreach ($policy in @('HKLM:\Software\Policies\Microsoft\Windows\CurrentVersion\Internet Settings','HKCU:\Software\Policies\Microsoft\Windows\CurrentVersion\Internet Settings')) {
        if (Test-Path -LiteralPath $policy) { throw 'WININET_POLICY_UNVERIFIED' }
    }
    $Receipt.directWinInet=$true
    $guid='{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    $keys=@("HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$guid","HKCU:\Software\Microsoft\EdgeUpdate\Clients\$guid")
    foreach ($key in $keys) {
        if (Test-Path -LiteralPath $key) {
            $raw=(Get-Item -LiteralPath $key).GetValue('pv')
            $version=$null
            if ([version]::TryParse($raw,[ref]$version) -and $version -gt [version]'0.0.0.0') { $Receipt.preexistingWebView=$true }
        }
    }
    if (!$Receipt.preexistingWebView) { throw 'PREEXISTING_WEBVIEW_REQUIRED' }
}
