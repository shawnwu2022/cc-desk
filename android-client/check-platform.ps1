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
& "$Jdk/bin/javac.exe" -encoding UTF-8 -classpath "$out/platform;$platform" -d "$out/unit" "$root/tests/cn/ccdesk/android/DesktopAliasTest.java"
if ($LASTEXITCODE -ne 0) { throw 'Desktop alias JVM test compile failed' }
& "$Jdk/bin/java.exe" -cp "$out/unit;$out/platform;$platform" cn.ccdesk.android.DesktopAliasTest
if ($LASTEXITCODE -ne 0) { throw 'Desktop alias JVM test failed' }
& "$Jdk/bin/javac.exe" -encoding UTF-8 -classpath "$out/platform;$platform" -d "$out/unit" "$root/tests/cn/ccdesk/android/PairingRecoveryLinkTest.java"
if ($LASTEXITCODE -ne 0) { throw 'Pairing recovery JVM test compile failed' }
& "$Jdk/bin/java.exe" -cp "$out/unit;$out/platform;$platform" cn.ccdesk.android.PairingRecoveryLinkTest
if ($LASTEXITCODE -ne 0) { throw 'Pairing recovery JVM test failed' }
& "$Jdk/bin/javac.exe" -encoding UTF-8 -d "$out/unit" "$root/src/cn/ccdesk/android/TerminalKeyShelfState.java" "$root/tests/cn/ccdesk/android/TerminalKeyShelfStateTest.java"
if ($LASTEXITCODE -ne 0) { throw 'Terminal key shelf JVM test compile failed' }
& "$Jdk/bin/java.exe" -cp "$out/unit" cn.ccdesk.android.TerminalKeyShelfStateTest
if ($LASTEXITCODE -ne 0) { throw 'Terminal key shelf JVM test failed' }

& "$Jdk/bin/javac.exe" -encoding UTF-8 -d "$out/unit" "$root/src/cn/ccdesk/android/TerminalViewportState.java" "$root/tests/cn/ccdesk/android/TerminalViewportStateTest.java"
if ($LASTEXITCODE -ne 0) { throw 'Terminal viewport JVM test compile failed' }
& "$Jdk/bin/java.exe" -cp "$out/unit" cn.ccdesk.android.TerminalViewportStateTest
if ($LASTEXITCODE -ne 0) { throw 'Terminal viewport JVM test failed' }

& "$Jdk/bin/javac.exe" -encoding UTF-8 -d "$out/unit" "$root/src/cn/ccdesk/android/SessionTitleFilter.java" "$root/tests/cn/ccdesk/android/SessionTitleFilterTest.java"
if ($LASTEXITCODE -ne 0) { throw 'Session title filter JVM test compile failed' }
& "$Jdk/bin/java.exe" -cp "$out/unit" cn.ccdesk.android.SessionTitleFilterTest
if ($LASTEXITCODE -ne 0) { throw 'Session title filter JVM test failed' }

& "$Jdk/bin/javac.exe" -encoding UTF-8 -d "$out/unit" "$root/src/cn/ccdesk/android/SessionSearchCoverage.java" "$root/tests/cn/ccdesk/android/SessionSearchCoverageTest.java"
if ($LASTEXITCODE -ne 0) { throw 'Session search coverage JVM test compile failed' }
& "$Jdk/bin/java.exe" -cp "$out/unit" cn.ccdesk.android.SessionSearchCoverageTest
if ($LASTEXITCODE -ne 0) { throw 'Session search coverage JVM test failed' }
