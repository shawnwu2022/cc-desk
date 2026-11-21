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
    # Native APIs classify effective settings; no use of private registry blobs,
    # no WinHTTP request/auto-proxy resolution, and no fallback when unknown.
    try {
        $rawShape=& (Join-Path $PSScriptRoot 'read-win-proxy-shape.ps1')
        if (@($rawShape).Count -ne 1 -or ([string]$rawShape).Length -gt 750) { throw 'PROXY_SHAPE_SCHEMA' }
        $shape=$rawShape | ConvertFrom-Json -AsHashtable
    } catch { throw 'PROXY_SHAPE_QUERY_BLOCKED' }
    if ($shape -isnot [System.Collections.IDictionary] -or $shape.Count -ne 8 -or
        $shape.winHttpDefaultQueryOk -isnot [bool] -or $shape.currentUserQueryOk -isnot [bool] -or
        $shape.winHttpDefaultAccessType -cnotin @('unknown','no-proxy','named-proxy','other') -or
        @(@('currentUserAutoDetect','currentUserManualProxy','currentUserPac','currentUserPolicyKey','machinePolicyKey') |
            Where-Object { $shape[$_] -cnotin @('unknown','present','absent') }).Count) { throw 'PROXY_SHAPE_SCHEMA' }
    if (!$shape.winHttpDefaultQueryOk -or $shape.winHttpDefaultAccessType -ceq 'unknown') { throw 'WINHTTP_UNVERIFIED' }
    if ($shape.winHttpDefaultAccessType -cne 'no-proxy') { throw 'WINHTTP_NOT_DIRECT' }
    $Receipt.directWinHttp=$true
    if (!$shape.currentUserQueryOk) { throw 'WININET_UNVERIFIED' }
    if ($shape.machinePolicyKey -cne 'absent' -or $shape.currentUserPolicyKey -cne 'absent') { throw 'WININET_POLICY_UNVERIFIED' }
    $manual=$shape.currentUserManualProxy -ceq 'present'
    $pac=$shape.currentUserPac -ceq 'present'
    $auto=$shape.currentUserAutoDetect -ceq 'present'
    if (@($shape.currentUserManualProxy,$shape.currentUserPac,$shape.currentUserAutoDetect) -ccontains 'unknown') { throw 'WININET_UNVERIFIED' }
    if (([int]$manual + [int]$pac + [int]$auto) -gt 1) { throw 'WININET_MIXED_PROXY' }
    if ($manual) { throw 'WININET_MANUAL_PROXY' }
    if ($pac) { throw 'WININET_EXPLICIT_PAC' }
    if ($auto) { throw 'WININET_AUTO_DETECT' }
    # Keep the old explicit HKCU scalar checks as a conservative consistency
    # tripwire. A native direct result does not override stale/contradictory data.
    $inet=Get-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    if ([int]$inet.GetValue('ProxyEnable',0) -ne 0 -or [int]$inet.GetValue('AutoDetect',0) -ne 0 -or
        $inet.GetValue('ProxyServer') -or $inet.GetValue('AutoConfigURL')) { throw 'WININET_SCALAR_CONFLICT' }
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
