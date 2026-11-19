# Disposable Windows runner only. The real Codex executable performs a read-only protocol check.
param(
    [Parameter(Mandatory=$true)][string]$Codex
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (!$IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or !$env:RUNNER_TEMP) {
    throw 'BLOCKED_EXTERNAL_TARGET: ephemeral hosted Windows required'
}
$temp = (Get-Item -LiteralPath $env:RUNNER_TEMP).FullName.TrimEnd('\') + '\'
$image = (Get-Item -LiteralPath $Codex -ErrorAction Stop).FullName
if (!$image.StartsWith($temp, [StringComparison]::OrdinalIgnoreCase) -or
    (Get-FileHash -LiteralPath $image).Hash -ine 'f76f94d08b0793bfa129ae62c09341ee32ae96d57e69968ae6b8d7c71a20d8c4') {
    throw 'Pinned official Codex image mismatch'
}
$root = Join-Path $temp ('pr101-history-' + [Guid]::NewGuid().ToString('N'))
$codexHome = Join-Path $root 'codex-home'
$project = Join-Path $root 'project'
$now = [DateTime]::UtcNow
$directory = Join-Path $codexHome ('sessions/' + $now.ToString('yyyy/MM/dd'))
New-Item -ItemType Directory -Path $project,$directory -Force | Out-Null
$id = [Guid]::NewGuid().ToString()
$archivedId = [Guid]::NewGuid().ToString()
$timestamp = $now.ToString("yyyy-MM-ddTHH:mm:ss.fff'Z'")
$stamp = $now.ToString('yyyy-MM-ddTHH-mm-ss')
$rollout = Join-Path $directory "rollout-$stamp-$id.jsonl"
$meta = [ordered]@{ timestamp=$timestamp; type='session_meta'; payload=[ordered]@{ session_id=$id; id=$id; timestamp=$timestamp; cwd=$project; originator='cc_desk_acceptance'; cli_version='0.169.1'; source='cli' } }
function Write-FixtureRollout([string]$File, [string]$SessionId, [string]$Canary) {
    $meta.payload.id = $SessionId
    $meta.payload.session_id = $SessionId
    $message = @{ timestamp=$timestamp; type='event_msg'; payload=@{type='user_message'; message=$Canary; images=@(); local_images=@(); text_elements=@()} }
    $output = @($meta,$message) | ForEach-Object { ConvertTo-Json -InputObject $_ -Compress -Depth 5 }
    [IO.File]::WriteAllText($File, ($output -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
}
$activeCanary='Pr101ActiveCanary'
$archivedCanary='Pr101ArchivedCanary'
$archiveRoot = Join-Path $codexHome 'archived_sessions'
New-Item -ItemType Directory -Path $archiveRoot | Out-Null
$existingArchived = Join-Path $archiveRoot "rollout-$stamp-$archivedId.jsonl"
Write-FixtureRollout $rollout $id $activeCanary
Write-FixtureRollout $existingArchived $archivedId $archivedCanary

$spec=[Diagnostics.ProcessStartInfo]::new($image)
$spec.WorkingDirectory=$project
$spec.UseShellExecute=$false
$spec.CreateNoWindow=$true
$spec.RedirectStandardInput=$true
$spec.RedirectStandardOutput=$true
$spec.RedirectStandardError=$true
$spec.ArgumentList.Add('app-server')
$spec.ArgumentList.Add('--strict-config')
$spec.Environment.Clear()
foreach ($key in @('SystemRoot','WINDIR','SystemDrive','ComSpec')) {
    $value = [Environment]::GetEnvironmentVariable($key)
    if ($value) { $spec.Environment[$key] = $value }
}
$spec.Environment['HOME']=$root
$spec.Environment['USERPROFILE']=$root
$spec.Environment['APPDATA']=$root
$spec.Environment['LOCALAPPDATA']=$root
$spec.Environment['TEMP']=$root
$spec.Environment['TMP']=$root
$spec.Environment['CODEX_HOME']=$codexHome

$process=[Diagnostics.Process]::Start($spec)
if (!$process) { throw 'Could not start pinned app-server' }
$stderr=$process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
$deadline=[DateTime]::UtcNow.AddSeconds(25)
function Send([object]$request) {
    $process.StandardInput.WriteLine((ConvertTo-Json -InputObject $request -Depth 8 -Compress))
    $process.StandardInput.Flush()
}
function Receive([int]$expected) {
    for ($i=0; $i -lt 48; $i++) {
        $remaining=$deadline - [DateTime]::UtcNow
        if ($remaining.TotalMilliseconds -le 0) { throw 'Pinned app-server deadline expired' }
        $pending=$process.StandardOutput.ReadLineAsync()
        if (!$pending.Wait($remaining)) { throw 'Pinned app-server reply timeout' }
        $line=$pending.Result
        if (!$line -or $line.Length -gt 65536) { throw 'Pinned app-server reply bound failed' }
        try { $frame=$line | ConvertFrom-Json -AsHashtable } catch { throw 'Pinned app-server reply format failed' }
        if ($frame.Contains('method') -and !$frame.Contains('id')) { continue }
        if ($frame.id -ne $expected -or $frame.Contains('error') -or !$frame.Contains('result')) {
            throw 'Pinned app-server returned an unexpected reply'
        }
        return $frame.result
    }
    throw 'Pinned app-server frame limit exceeded'
}
try {
    Send @{ id=1; method='initialize'; params=@{ clientInfo=@{name='cc_desk'; version='0.28.0'}; capabilities=@{explicitGatewayOauth=$true; experimentalApi=$false} } }
    $initial = Receive 1
    if (!$initial.codexHome -or [IO.Path]::GetFullPath($initial.codexHome).TrimEnd('\') -ine $codexHome) {
        throw 'Pinned app-server did not bind to fixture CODEX_HOME'
    }
    Send @{ method='initialized' }
    Send @{ id=2; method='thread/read'; params=@{ threadId=$id; includeTurns=$false } }
    $observed = Receive 2
    if ($observed.thread.id -cne $id) { throw 'Pinned app-server did not recognize fixture rollout' }
    Send @{ id=3; method='thread/read'; params=@{ threadId=$archivedId; includeTurns=$false } }
    $archived = Receive 3
    if ($archived.thread.id -cne $archivedId) { throw 'Pinned app-server did not recognize the separate pre-archived rollout' }
    $process.StandardInput.Close()
    if (!$process.WaitForExit(5000) -or $process.ExitCode -ne 0) { throw 'Pinned app-server did not stop normally' }
    [pscustomobject]@{ Home=$codexHome; Project=$project; SessionId=$id; Rollout=$rollout; ActiveCanary=$activeCanary; ArchivedId=$archivedId; ArchiveRollout=$existingArchived; ArchivedCanary=$archivedCanary; AppServerReadVerified=$true }
} finally {
    if (!$process.HasExited) { $process.Kill(); $null=$process.WaitForExit(5000) }
    $process.Dispose()
}
