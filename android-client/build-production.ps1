
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Build','Sign')][string]$Phase,
    [Parameter(Mandatory)][string]$WorkDirectory,
    [Parameter(Mandatory)][string]$SourceSha,
    [Parameter(Mandatory)][int]$VersionCode,
    [Parameter(Mandatory)][string]$Sdk,
    [string]$Jdk,
    [string]$ZxingCore
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$repo = Split-Path -Parent $root
$toolchain = '1.100.0'
$ndkRevision = '27.3.13750724'
$abi = 'arm64-v8a'
$triple = 'aarch64-linux-android'
$nativeName = 'libcc_desk_android_native.so'
$nativePath = "lib/$abi/$nativeName"
$package = 'cn.ccdesk.android'
$androidNs = 'http://schemas.android.com/apk/res/android'
$bt = Join-Path $Sdk 'build-tools/35.0.0'
$aapt = Join-Path $bt 'aapt.exe'
$align = Join-Path $bt 'zipalign.exe'
$signer = Join-Path $bt 'apksigner.bat'
$platform = Join-Path $Sdk 'platforms/android-35/android.jar'
$env:GIT_TERMINAL_PROMPT = '0'

function Require($condition, [string]$message) {
    if (-not $condition) { throw $message }
}
function Run([string]$exe, [string[]]$argv) {
    & $exe @argv
    if ($LASTEXITCODE -ne 0) { throw "Android $Phase command failed: $(Split-Path -Leaf $exe) ($LASTEXITCODE)" }
}
function Hash([string]$file) {
    return (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
}

Require ($SourceSha -cmatch '^[0-9a-f]{40}$') 'Expected one exact source commit SHA'
Require ($VersionCode -ge 1 -and $VersionCode -le 2100000000) 'Android versionCode is out of range'
$head = Run 'git' @('-C', $repo, 'rev-parse', 'HEAD')
Require ($head -ceq $SourceSha) 'Checkout is not the admitted SHA'
Require (-not (Run 'git' @('-C', $repo, 'status', '--porcelain', '--untracked-files=no'))) 'Tracked source differs from the admitted SHA'
$version = (Get-Content -LiteralPath "$repo/package.json" -Raw | ConvertFrom-Json).version
Require ($version -cmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(?:rc|beta|preview)\.[1-9][0-9]*)?$') 'Unrecognized CC Desk release version'
$WorkDirectory = [IO.Path]::GetFullPath($WorkDirectory)
$unaligned = Join-Path $WorkDirectory 'unsigned.apk'
$aligned = Join-Path $WorkDirectory 'unsigned-aligned.apk'
$proofPath = Join-Path $WorkDirectory 'build-proof.json'
$outputDir = Join-Path $WorkDirectory 'publishable-candidate'
foreach ($p in @($aapt,$align,$signer,$platform)) {
    Require (Test-Path -LiteralPath $p -PathType Leaf) "Missing pinned Android SDK 35 tool: $(Split-Path -Leaf $p)"
}

# aapt's verbose archive listing reports the actual compression method.
function AssertApk([string]$apk, [string]$expectedSoSha) {
    Run $align @('-c','-P','16','-v','4',$apk) | Out-Null
    $archive = @(Run $aapt @('list','-v',$apk))
    $storedRows = @($archive | Where-Object { $_ -match ('(?i)\bStored\b.*\s' + [regex]::Escape($nativePath) + '$') })
    Require ($storedRows.Count -eq 1) 'APK JNI library is not a single uncompressed arm64 member'
    $zip = [IO.Compression.ZipFile]::OpenRead($apk)
    try {
        $lib = @($zip.Entries | Where-Object { $_.FullName.EndsWith('.so', [StringComparison]::OrdinalIgnoreCase) })
        Require ($lib.Count -eq 1 -and $lib[0].FullName -ceq $nativePath) 'Unexpected APK native libraries or ABI'
        Require (@($zip.Entries | Where-Object FullName -CEQ 'classes.dex').Count -eq 1) 'Expected exactly one classes.dex'
        $stream = $lib[0].Open()
        try { $actual = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant() }
        finally { $stream.Dispose() }
        Require ($actual -ceq $expectedSoSha) 'Packaged library differs from the checked ELF'
    } finally { $zip.Dispose() }
    $badging = @(Run $aapt @('dump','badging',$apk))
    $versionLine = "^package: name='$([regex]::Escape($package))' versionCode='$VersionCode' versionName='$([regex]::Escape($version))'"
    Require (@($badging | Where-Object { $_ -cmatch $versionLine }).Count -eq 1) 'APK identity or exact release version not as admitted'
    Require (@($badging | Where-Object { $_ -cmatch "^uses-permission: name='android\.permission\.INTERNET'$" }).Count -eq 1) 'APK lacks Internet permission'
}

if ($Phase -ceq 'Build') {
    Require (-not (Test-Path -LiteralPath $WorkDirectory)) 'Android working directory already exists'
    Require ($Jdk -and (Test-Path -LiteralPath "$Jdk/bin/javac.exe" -PathType Leaf)) 'Missing preinstalled JDK'
    Require ($ZxingCore -and (Hash $ZxingCore) -ceq '8d8064c1636fdaef7189dd9055c7d59950a8940a12f2293956446ec3c109fd82') 'Wrong ZXing Core 3.5.3 SHA-256'
    $ndk = Join-Path $Sdk "ndk/$ndkRevision"
    Require ((Get-Content -LiteralPath "$ndk/source.properties" -Raw) -match "(?m)^Pkg\.Revision\s*=\s*$([regex]::Escape($ndkRevision))\s*$") 'Pinned preinstalled Android NDK r27 is unavailable'
    $llvm = Join-Path $ndk 'toolchains/llvm/prebuilt/windows-x86_64'
    $bin = Join-Path $llvm 'bin'
    foreach ($exe in @('clang.exe','llvm-ar.exe','llvm-readelf.exe','llvm-nm.exe')) {
        Require (Test-Path -LiteralPath "$bin/$exe" -PathType Leaf) 'Required NDK LLVM tool is missing'
    }
    Require (Test-Path -LiteralPath "$llvm/lib/clang" -PathType Container) 'NDK compiler runtime is missing'
    $nativeCargo = Join-Path $root 'native/Cargo.toml'
    $nativeLock = Join-Path $root 'native/Cargo.lock'
    Require ((Test-Path -LiteralPath $nativeCargo -PathType Leaf) -and (Test-Path -LiteralPath $nativeLock -PathType Leaf)) 'Integrated native crate and exact Cargo.lock are required'
    $lockSha = Hash $nativeLock

    $java = Join-Path $root 'src/cn/ccdesk/android/NativeDesktopBackend.java'
    Require (Test-Path -LiteralPath $java -PathType Leaf) 'Integrated Android Java JNI backend is required'
    $javaSource = Get-Content -LiteralPath $java -Raw
    Require ($javaSource -cmatch 'System\.loadLibrary\(\s*"cc_desk_android_native"\s*\)') 'Java does not load the packaged JNI library'
    $methods = @([regex]::Matches($javaSource, '\bnative\s+[A-Za-z0-9_.]+(?:\[\])?\s+(native[A-Za-z0-9]+)\s*\(') |
        ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
    $requiredMethods = @('nativeAndroidAbiVersion','nativeBeginPairing','nativePairingState','nativePairingHostId','nativeCancelPairing',
        'nativeStartPairedObserver','nativeReadPairedHostId','nativeObserverState','nativeCancelObserver','nativeTakeObserverText')
    Require (@($requiredMethods | Where-Object { $_ -cnotin $methods }).Count -eq 0) 'Incomplete integrated Java pairing/Observer JNI surface'

    $manifest = [xml](Get-Content -LiteralPath "$root/AndroidManifest.xml" -Raw)
    Require ($manifest.DocumentElement.GetAttribute('package') -ceq $package) 'Android package ID changed'
    $app = $manifest.DocumentElement.SelectSingleNode('application')
    $usesSdk = $manifest.DocumentElement.SelectSingleNode('uses-sdk')
    Require ($usesSdk.GetAttribute('minSdkVersion',$androidNs) -ceq '31' -and $usesSdk.GetAttribute('targetSdkVersion',$androidNs) -ceq '35') 'SDK admission changed'
    Require ($app.GetAttribute('usesCleartextTraffic',$androidNs) -ceq 'false' -and $app.GetAttribute('allowBackup',$androidNs) -ceq 'false') 'Manifest must keep transport and backup restrictions'
    Require ($app.GetAttribute('extractNativeLibs',$androidNs) -ceq 'false') 'Manifest must load aligned uncompressed JNI library'
    Require (@($manifest.DocumentElement.SelectNodes('uses-permission') | Where-Object {$_.GetAttribute('name',$androidNs) -ceq 'android.permission.INTERNET'}).Count -eq 1) 'Manifest needs exactly one INTERNET permission'
    # The tracked manifest is authoritative; only release display/version metadata is derived.
    $manifest.DocumentElement.SetAttribute('versionCode',$androidNs,[string]$VersionCode)
    $manifest.DocumentElement.SetAttribute('versionName',$androidNs,$version)
    $app.SetAttribute('label',$androidNs,'CC Desk')

    New-Item -ItemType Directory -Path "$WorkDirectory/classes", "$WorkDirectory/dex", "$WorkDirectory/lib/$abi" -Force | Out-Null
    $manifestFile = Join-Path $WorkDirectory 'AndroidManifest.xml'
    $manifest.Save($manifestFile)
    $target = Join-Path $WorkDirectory 'cargo-target'
    $env:JAVA_HOME = $Jdk
    $env:CC_aarch64_linux_android = "$bin/clang.exe"
    $env:AR_aarch64_linux_android = "$bin/llvm-ar.exe"
    $env:CFLAGS_aarch64_linux_android = '--target=aarch64-linux-android31'
    $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = "$bin/clang.exe"
    $env:CARGO_ENCODED_RUSTFLAGS = @(
        '-Clink-arg=--target=aarch64-linux-android31',
        '-Clink-arg=-Wl,-z,max-page-size=16384',
        '-Clink-arg=-Wl,-z,common-page-size=16384'
    ) -join [char]31
    Push-Location $repo
    try {
        Run 'rustup' @('target','add','--toolchain',$toolchain,$triple)
        Run 'cargo' @("+$toolchain",'build','--release','--locked','--target',$triple,'--target-dir',$target,'--manifest-path',$nativeCargo)
    } finally { Pop-Location }
    Require ((Hash $nativeLock) -ceq $lockSha) 'Cargo.lock changed during locked build'
    $so = Join-Path $WorkDirectory $nativePath
    Copy-Item -LiteralPath "$target/$triple/release/$nativeName" -Destination $so
    $soSha = Hash $so
    $elf = @(Run "$bin/llvm-readelf.exe" @('--file-header','--program-headers','--wide',$so))
    Require (@($elf | Where-Object { $_ -match '^\s*Class:\s+ELF64\s*$' }).Count -eq 1 -and
        @($elf | Where-Object { $_ -match '^\s*Machine:\s+AArch64\s*$' }).Count -eq 1 -and
        @($elf | Where-Object { $_ -match '^\s*Type:\s+DYN\b' }).Count -eq 1) 'Expected a 64-bit AArch64 ELF shared object'
    $segments = @($elf | Where-Object { $_ -match '^\s*LOAD\s+' })
    Require ($segments.Count -gt 0) 'ELF contains no program-load segments'
    foreach ($line in $segments) {
        Require ($line -match '^\s*LOAD\s+0x([0-9a-f]+)\s+0x([0-9a-f]+)\s+.*\s0x([0-9a-f]+)\s*$') 'Unrecognized ELF LOAD header'
        $offset = [Convert]::ToUInt64($Matches[1],16)
        $addr = [Convert]::ToUInt64($Matches[2],16)
        $alignment = [Convert]::ToUInt64($Matches[3],16)
        Require ($alignment -ge 16384 -and (($alignment -band ($alignment - 1)) -eq 0) -and ($offset % 16384 -eq $addr % 16384)) 'ELF LOAD segment is not 16 KiB aligned'
    }
    $symbols = @(Run "$bin/llvm-nm.exe" @('--dynamic','--defined-only',$so))
    foreach ($method in $methods) {
        Require (@($symbols | Where-Object { $_ -cmatch ("\sJava_cn_ccdesk_android_NativeDesktopBackend_" + [regex]::Escape($method) + '$') }).Count -eq 1) "Missing exact JNI export for $method"
    }

    $sources = @(Get-ChildItem "$root/src" -Recurse -File -Filter '*.java' | ForEach-Object FullName)
    Run "$Jdk/bin/javac.exe" (@('-encoding','UTF-8','--release','8','-Xlint:-options','-classpath',"$platform;$ZxingCore",'-d',"$WorkDirectory/classes") + $sources)
    Run "$Jdk/bin/jar.exe" @('cf',"$WorkDirectory/classes.jar",'-C',"$WorkDirectory/classes",'.')
    Run "$bt/d8.bat" @('--release','--min-api','31','--lib',$platform,'--output',"$WorkDirectory/dex","$WorkDirectory/classes.jar",$ZxingCore)
    Run $aapt @('package','-f','-M',$manifestFile,'-I',$platform,'-A',"$root/assets",'-F',$unaligned)
    Run "$Jdk/bin/jar.exe" @('--update','--no-compress','--no-manifest','--file',$unaligned,'-C',"$WorkDirectory/dex",'classes.dex','-C',$WorkDirectory,'lib')
    Run $align @('-P','16','-f','4',$unaligned,$aligned)
    AssertApk $aligned $soSha
    @{ sourceSha=$SourceSha; version=$version; versionCode=$VersionCode; abi=$abi;
       rustToolchain=$toolchain; ndkRevision=$ndkRevision; sdkBuildTools='35.0.0';
       minSdk=31; targetSdk=35; lockSha256=$lockSha; elfSha256=$soSha;
       unsignedSha256=(Hash $aligned); elf16KiB=$true; apk16KiB=$true; jniExports=$methods
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $proofPath -Encoding utf8NoBOM
    return
}

$signerPin = $env:ANDROID_RELEASE_CERT_SHA256
Require ($signerPin -cmatch '^[a-fA-F0-9]{64}$' -and $signerPin -notmatch '^0+$') 'Missing pinned production Android signing certificate SHA-256'
Require ($env:ANDROID_RELEASE_KEY_ALIAS -cmatch '^[A-Za-z0-9._-]{1,100}$' -and $env:ANDROID_RELEASE_KEY_ALIAS -ine 'androiddebugkey') 'Missing non-debug production signing alias'
Require ($env:ANDROID_RELEASE_KEYSTORE_B64 -and $env:ANDROID_RELEASE_KEYSTORE_PASSWORD -and $env:ANDROID_RELEASE_KEY_PASSWORD) 'Protected android-production signer is not configured; no APK will be signed'
Require (-not (Test-Path -LiteralPath $outputDir)) 'Android candidate output already exists'
$proof = Get-Content -LiteralPath $proofPath -Raw | ConvertFrom-Json -AsHashtable
Require ($proof.sourceSha -ceq $SourceSha -and $proof.version -ceq $version -and $proof.versionCode -eq $VersionCode -and
    $proof.ndkRevision -ceq $ndkRevision -and $proof.abi -ceq $abi -and $proof.unsignedSha256 -ceq (Hash $aligned)) 'Unsigned Android artifact does not match admitted build proof'
AssertApk $aligned $proof.elfSha256
$keystore = Join-Path $WorkDirectory ('signer-' + [guid]::NewGuid().ToString('N') + '.ks')
$stagedSigned = Join-Path $WorkDirectory 'staged-signed.apk'
try {
    try { [IO.File]::WriteAllBytes($keystore, [Convert]::FromBase64String($env:ANDROID_RELEASE_KEYSTORE_B64)) }
    catch { throw 'Production Android keystore could not be materialized; no secret bytes were returned' }
    Run $signer @('sign','--ks',$keystore,'--ks-key-alias',$env:ANDROID_RELEASE_KEY_ALIAS,
        '--ks-pass','env:ANDROID_RELEASE_KEYSTORE_PASSWORD','--key-pass','env:ANDROID_RELEASE_KEY_PASSWORD',
        '--min-sdk-version','31','--v2-signing-enabled','true','--v3-signing-enabled','true','--v4-signing-enabled','false',
        '--out',$stagedSigned,$aligned) | Out-Null
} finally {
    if (Test-Path -LiteralPath $keystore -PathType Leaf) { Remove-Item -LiteralPath $keystore -Force }
}
$signed = @(Run $signer @('verify','--verbose','--print-certs','--min-sdk-version','31',$stagedSigned))
$certs = @($signed | Where-Object { $_ -cmatch '^Signer #[0-9]+ certificate SHA-256 digest: ([a-fA-F0-9]{64})$' })
Require ($certs.Count -eq 1 -and ($certs[0] -match ': ([a-fA-F0-9]{64})$') -and $Matches[1] -ieq $signerPin) 'APK signer differs from the pinned authorized production signer'
Require (-not @($signed | Where-Object { $_ -match 'certificate DN:.*(?:Android Debug|CN=debug)' })) 'Debug certificate cannot sign a release candidate'
Require (@($signed | Where-Object { $_ -match '^Verified using v2 scheme .*: true$' }).Count -eq 1 -and
    @($signed | Where-Object { $_ -match '^Verified using v3 scheme .*: true$' }).Count -eq 1) 'Expected valid APK v2 and v3 signatures'
AssertApk $stagedSigned $proof.elfSha256
$proof.signedSha256 = Hash $stagedSigned
$proof.signerCertSha256 = $signerPin.ToLowerInvariant()
$proof.workflowRunId = $env:GITHUB_RUN_ID
$proof.workflowRunAttempt = $env:GITHUB_RUN_ATTEMPT
$proof.ordinaryCiRunId = $env:ANDROID_ADMITTED_CI_RUN
$proof.ordinaryCiRunAttempt = $env:ANDROID_ADMITTED_CI_ATTEMPT
$proof.deviceAcceptance = 'unverified'
$proof.publication = 'not-published'
New-Item -ItemType Directory -Path $outputDir | Out-Null
$finalApk = Join-Path $outputDir "cc-desk-$version-android-$abi.apk"
Copy-Item -LiteralPath $stagedSigned -Destination $finalApk
Require ((Hash $finalApk) -ceq $proof.signedSha256) 'Candidate bytes differ after copy'
$proof | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath "$outputDir/android-release-build-proof.json" -Encoding utf8NoBOM
