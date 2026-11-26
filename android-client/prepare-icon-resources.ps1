param([Parameter(Mandatory=$true)][string]$WorkDirectory)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$res = Join-Path $WorkDirectory 'icon-res'
$source = Join-Path $repo 'src-tauri/icons/source/png/ccdesk-256.png'
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw 'Approved desktop icon is missing' }
foreach ($folder in @('drawable-nodpi', 'drawable', 'mipmap-anydpi-v26')) {
    New-Item -ItemType Directory -Force -Path (Join-Path $res $folder) | Out-Null
}
Copy-Item -LiteralPath $source -Destination (Join-Path $res 'drawable-nodpi/ccdesk_desktop_art.png')
$xml = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText((Join-Path $res 'drawable/ccdesk_icon_inset.xml'), '<inset xmlns:android="http://schemas.android.com/apk/res/android" android:drawable="@drawable/ccdesk_desktop_art" android:inset="22%" />', $xml)
[IO.File]::WriteAllText((Join-Path $res 'mipmap-anydpi-v26/ccdesk_launcher.xml'), '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android"><background android:drawable="@android:color/white" /><foreground android:drawable="@drawable/ccdesk_icon_inset" /></adaptive-icon>', $xml)
return $res
