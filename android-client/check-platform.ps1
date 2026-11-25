param(
    [Parameter(Mandatory=$true)][string]$Sdk,
    [Parameter(Mandatory=$true)][string]$Jdk
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$out = Join-Path $root ('out/' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force "$out/platform", "$out/unit" | Out-Null
$platform = Join-Path $Sdk 'platforms/android-35/android.jar'
$signature = "$root/src/cn/ccdesk/android/P256Signature.java"
& "$Jdk/bin/javac.exe" -encoding UTF-8 -source 8 -target 8 -Xlint:-options -bootclasspath $platform -d "$out/platform" $signature "$root/src/cn/ccdesk/android/HardwareIdentity.java"
if ($LASTEXITCODE -ne 0) { throw 'Android API compile failed' }
& "$Jdk/bin/javac.exe" -encoding UTF-8 -d "$out/unit" $signature "$root/tests/cn/ccdesk/android/P256SignatureTest.java"
if ($LASTEXITCODE -ne 0) { throw 'JVM test compile failed' }
& "$Jdk/bin/java.exe" -cp "$out/unit" cn.ccdesk.android.P256SignatureTest
if ($LASTEXITCODE -ne 0) { throw 'JVM test failed' }
