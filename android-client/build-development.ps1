param(
    [Parameter(Mandatory=$true)][string]$Sdk,
    [Parameter(Mandatory=$true)][string]$Jdk,
    [Parameter(Mandatory=$true)][string]$ZxingCore
)
$ErrorActionPreference = 'Stop'
$ZxingCore = (Resolve-Path -LiteralPath $ZxingCore).Path
if ((Get-FileHash -LiteralPath $ZxingCore -Algorithm SHA256).Hash -ne '8D8064C1636FDAEF7189DD9055C7D59950A8940A12F2293956446EC3C109FD82') {
    throw 'Expected the pinned ZXing Core 3.5.3 Maven artifact'
}
$root = $PSScriptRoot
$out = Join-Path $root ('out/development-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force "$out/classes", "$out/dex" | Out-Null
$platform = Join-Path $Sdk 'platforms/android-35/android.jar'
$bt = Join-Path $Sdk 'build-tools/35.0.0'
$sources = @(Get-ChildItem "$root/src" -Recurse -File -Filter '*.java' |
    Select-Object -ExpandProperty FullName)
function Run([string]$exe, [string[]]$params) {
    & $exe @params
    if ($LASTEXITCODE -ne 0) { throw "Development build failed: $exe ($LASTEXITCODE)" }
}
$oldJava = $env:JAVA_HOME
try {
    $env:JAVA_HOME = $Jdk
    Run "$Jdk/bin/javac.exe" (@('-encoding', 'UTF-8', '--release', '8',
        '-Xlint:-options', '-classpath', ("$platform;$ZxingCore"), '-d', "$out/classes") + $sources)
    Run "$Jdk/bin/jar.exe" @('cf', "$out/classes.jar", '-C', "$out/classes", '.')
    Run "$bt/d8.bat" @('--min-api', '31', '--lib', $platform, '--output', "$out/dex", "$out/classes.jar", $ZxingCore)
    Run "$bt/aapt.exe" @('package', '-f', '-M', "$root/AndroidManifest.xml", '-I', $platform, '-A', "$root/assets",
        '-F', "$out/development-unsigned.apk")
    Push-Location "$out/dex"
    try { Run "$bt/aapt.exe" @('add', "$out/development-unsigned.apk", 'classes.dex') }
    finally { Pop-Location }
    Run "$bt/zipalign.exe" @('-f', '4', "$out/development-unsigned.apk", "$out/development-unsigned-aligned.apk")
    Write-Output "Development build only. Not signed, installable or deployable: $out/development-unsigned-aligned.apk"
} finally {
    $env:JAVA_HOME = $oldJava
}
