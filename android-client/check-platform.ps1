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
$out = Join-Path $root ('out/' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force "$out/platform", "$out/unit" | Out-Null
$platform = Join-Path $Sdk 'platforms/android-35/android.jar'
$signature = "$root/src/cn/ccdesk/android/P256Signature.java"
$platformSources = @(Get-ChildItem "$root/src" -Recurse -Filter "*.java" -File | Select-Object -ExpandProperty FullName)
& "$Jdk/bin/javac.exe" -encoding UTF-8 --release 8 -Xlint:-options -classpath "$platform;$ZxingCore" -d "$out/platform" @platformSources
if ($LASTEXITCODE -ne 0) { throw 'Android API compile failed' }
& "$Jdk/bin/javac.exe" -encoding UTF-8 -d "$out/unit" $signature "$root/tests/cn/ccdesk/android/P256SignatureTest.java"
if ($LASTEXITCODE -ne 0) { throw 'JVM test compile failed' }
& "$Jdk/bin/java.exe" -cp "$out/unit" cn.ccdesk.android.P256SignatureTest
if ($LASTEXITCODE -ne 0) { throw 'JVM test failed' }
