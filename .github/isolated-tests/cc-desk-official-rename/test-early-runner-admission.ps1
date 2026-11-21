# Only report a fixed enum; never print native values or raw errors.
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'test-fresh-runner-admission.ps1')
$receipt=[ordered]@{cleanTestProfile=$false;proxyEnvironmentAbsent=$false;directWinHttp=$false;directWinInet=$false;preexistingWebView=$false}
$status='blocked'
$code='ADMISSION_UNKNOWN'
try {
    Assert-FreshRunner $receipt
    $status='passed'
    $code='DIRECT_PROFILE_WEBVIEW'
} catch {
    $what=$_.Exception.Message
    if ($what -cin @('NOT_DISPOSABLE_INTERACTIVE_HOST','UNCLEAN_TEST_PROFILE','EXISTING_APP_OR_PROFILE','EXISTING_APP_OR_CLI','ENVIRONMENT_OVERRIDE_PRESENT',
                    'AUTHENTICATION_ENVIRONMENT_PRESENT','PROXY_SHAPE_QUERY_BLOCKED','PROXY_SHAPE_SCHEMA','WINHTTP_UNVERIFIED','WINHTTP_NOT_DIRECT','WININET_UNVERIFIED',
                    'WININET_POLICY_UNVERIFIED','WININET_MIXED_PROXY','WININET_MANUAL_PROXY','WININET_EXPLICIT_PAC','WININET_AUTO_DETECT',
                    'WININET_SCALAR_CONFLICT','PREEXISTING_WEBVIEW_REQUIRED')) { $code=$what }
}
[ordered]@{status=$status;code=$code} | ConvertTo-Json -Compress
