$ErrorActionPreference = 'Stop'
if (-not $env:RUNNER_TEMP -or -not $env:GITHUB_ENV) { throw 'This preparation step requires an isolated CI runner' }
$root = Join-Path $env:RUNNER_TEMP 'cc-desk-browser-tools-21.1.8-0.2.129'
New-Item -ItemType Directory -Path $root -Force | Out-Null
function Get-VerifiedArchive($Name, $Url, $Sha) {
    $file = Join-Path $root $Name
    if (-not (Test-Path -LiteralPath $file)) { Invoke-WebRequest -Uri $Url -OutFile $file }
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $Sha) { throw "Tool archive hash mismatch: $Name" }
    return $file
}
function Invoke-Checked($Command, $Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Tool preparation failed: $Command" }
}
$llvmArchive = Get-VerifiedArchive 'clang+llvm-21.1.8-x86_64-pc-windows-msvc.tar.xz' 'https://github.com/llvm/llvm-project/releases/download/llvmorg-21.1.8/clang%2Bllvm-21.1.8-x86_64-pc-windows-msvc.tar.xz' '749d22f565fcd5718dbed06512572d0e5353b502c03fe1f7f17ee8b8aca21a47'
$bindgenArchive = Get-VerifiedArchive 'wasm-bindgen-0.2.129-x86_64-pc-windows-msvc.tar.gz' 'https://github.com/wasm-bindgen/wasm-bindgen/releases/download/0.2.129/wasm-bindgen-0.2.129-x86_64-pc-windows-msvc.tar.gz' '79e348c169d0c10c0b287647f1e753d0985353ca96800bc29be8f4261baf13e6'
$llvm = Join-Path $root 'llvm'
$bindgen = Join-Path $root 'bindgen'
New-Item -ItemType Directory -Path $llvm, $bindgen -Force | Out-Null
# 先校验官方压缩包，再直接解包；新 PowerShell 原样传输二进制流。
if ($PSVersionTable.PSVersion -ge [version]'7.4') {
    & {
        $PSNativeCommandUseErrorActionPreference = $true
        7z x -so $llvmArchive | tar -xf - -C $llvm --strip-components=1
        if ($LASTEXITCODE -ne 0) { throw 'LLVM extraction failed' }
    }
} else {
    # 旧版 PowerShell 会把原生命令管道转换为文本，保留原双步解包。
    Invoke-Checked '7z' @('x', '-y', $llvmArchive, "-o$root")
    Invoke-Checked 'tar' @('-xf', (Join-Path $root 'clang+llvm-21.1.8-x86_64-pc-windows-msvc.tar'), '-C', $llvm, '--strip-components=1')
}
Invoke-Checked 'tar' @('-xf', $bindgenArchive, '-C', $bindgen, '--strip-components=1')
$clang = Join-Path $llvm 'bin/clang.exe'
$ar = Join-Path $llvm 'bin/llvm-ar.exe'
$wasmBindgen = Join-Path $bindgen 'wasm-bindgen.exe'
Invoke-Checked $clang @('--version')
Invoke-Checked $ar @('--version')
Invoke-Checked $wasmBindgen @('--version')
# 只传给后续构建子进程，不改用户 PATH 或系统工具链。
"CC_wasm32_unknown_unknown=$clang" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
"AR_wasm32_unknown_unknown=$ar" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
"WASM_BINDGEN=$wasmBindgen" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
