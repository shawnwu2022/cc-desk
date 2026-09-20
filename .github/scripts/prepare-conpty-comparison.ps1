# One-variable field experiment. No application rebuild, installation or release.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$exeHash = '461B34E747E007D0288F53B23F7452C4D090A818A8BB25CFF730766593EA3DBD'
$packageHash = '9382AD7BECB7E4D84E300578D8E4F4DF28F43D979D9055D978C42913C47E0E9D'
$url = 'https://github.com/microsoft/terminal/releases/download/v1.24.11911.0/Microsoft.Windows.Console.ConPTY.1.24.260710001.nupkg'
$licenseUrl = 'https://github.com/microsoft/terminal/blob/v1.24.11911.0/LICENSE'
$source = Join-Path $PWD 'source-diagnostic/cc-desk-paste-trace.exe'
if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne $exeHash) { throw 'Unexpected base executable; refusing to change the experiment' }
if ((Get-Content source-diagnostic/BUILD.txt -Raw) -notmatch 'build=0be6078480865bb3bd75bd4810c659876c6f5408') { throw 'Wrong base build' }
$temp = Join-Path $env:RUNNER_TEMP ('conpty-' + [guid]::NewGuid())
New-Item -ItemType Directory $temp | Out-Null
try {
    $zip = Join-Path $temp 'runtime.zip'
    Invoke-WebRequest -Uri $url -OutFile $zip
    if ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -ne $packageHash) { throw 'Microsoft release asset digest mismatch' }
    Expand-Archive -LiteralPath $zip -DestinationPath (Join-Path $temp 'runtime')
    # NuGet places the DLL and host in different trees. Select each by architecture
    # within this one hash-verified package; never mix versions or architectures.
    $dlls = @(Get-ChildItem (Join-Path $temp 'runtime') -Recurse -File -Filter 'conpty.dll' | Where-Object { $_.FullName -match '[\\/](win-)?x64[\\/]' })
    $hosts = @(Get-ChildItem (Join-Path $temp 'runtime') -Recurse -File -Filter 'OpenConsole.exe' | Where-Object { $_.FullName -match '[\\/](win-)?x64[\\/]' })
    if ($dlls.Count -ne 1 -or $hosts.Count -ne 1) { throw "Expected one x64 pair; DLL=$($dlls.Count), host=$($hosts.Count)" }
    $dll = $dlls[0]
    $hostExe = $hosts[0].FullName
    $out = Join-Path $PWD 'conpty-comparison'
    if (Test-Path -LiteralPath $out) { throw 'Output directory already exists' }
    New-Item -ItemType Directory $out | Out-Null
    Copy-Item -LiteralPath $source -Destination (Join-Path $out 'cc-desk-paste-trace.exe')
    Copy-Item -LiteralPath $dll.FullName -Destination $out
    Copy-Item -LiteralPath $hostExe -Destination $out
    Copy-Item docs/paste-conpty-comparison.md (Join-Path $out 'README.md')
    Copy-Item source-diagnostic/BUILD.txt (Join-Path $out 'BASE-BUILD.txt')
    # This nupkg refers to its MIT license but does not contain a LICENSE file.
    # Preserve the complete copyright and license from its pinned upstream tag.
    Copy-Item docs/licenses/Microsoft-ConPTY-LICENSE.txt (Join-Path $out 'LICENSE-Microsoft-ConPTY.txt')

    # ABI/host-lifecycle check using portable-pty 0.8.1 export names and flags.
    # This is not a Claude input acceptance or a Windows 10 field reproduction.
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ConptyCompatibilityProbe {
  [StructLayout(LayoutKind.Sequential)] public struct Coord {
    public short X; public short Y;
    public Coord(short x, short y) { X=x; Y=y; }
  }
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)]
  static extern IntPtr LoadLibraryExW(string path, IntPtr file, uint flags);
  [DllImport("kernel32.dll", CharSet=CharSet.Ansi, ExactSpelling=true)]
  static extern IntPtr GetProcAddress(IntPtr module, string name);
  [DllImport("kernel32.dll", SetLastError=true)]
  static extern bool CreatePipe(out IntPtr read, out IntPtr write, IntPtr attrs, uint size);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
  [DllImport("kernel32.dll")] static extern bool FreeLibrary(IntPtr module);
  [UnmanagedFunctionPointer(CallingConvention.Winapi)]
  delegate int Create(Coord size, IntPtr input, IntPtr output, uint flags, out IntPtr console);
  [UnmanagedFunctionPointer(CallingConvention.Winapi)] delegate int Resize(IntPtr console, Coord size);
  [UnmanagedFunctionPointer(CallingConvention.Winapi)] delegate void Close(IntPtr console);
  static T Function<T>(IntPtr module, string name) where T : Delegate {
    var p=GetProcAddress(module,name);
    if(p==IntPtr.Zero) throw new Exception("Missing legacy export: "+name);
    return Marshal.GetDelegateForFunctionPointer<T>(p);
  }
  public static void Run(string path) {
    IntPtr lib=IntPtr.Zero, ir=IntPtr.Zero, iw=IntPtr.Zero, outputRead=IntPtr.Zero, ow=IntPtr.Zero, pc=IntPtr.Zero;
    Close close=null;
    try {
      lib=LoadLibraryExW(path,IntPtr.Zero,0x1100);
      if(lib==IntPtr.Zero) throw new Exception("LoadLibraryExW error "+Marshal.GetLastWin32Error());
      var create=Function<Create>(lib,"CreatePseudoConsole");
      var resize=Function<Resize>(lib,"ResizePseudoConsole");
      close=Function<Close>(lib,"ClosePseudoConsole");
      if(!CreatePipe(out ir,out iw,IntPtr.Zero,4096) || !CreatePipe(out outputRead,out ow,IntPtr.Zero,4096)) throw new Exception("CreatePipe failed");
      int hr=create(new Coord(80,24),ir,ow,6,out pc);
      if(hr!=0) Marshal.ThrowExceptionForHR(hr);
      hr=resize(pc,new Coord(100,30));
      if(hr!=0) Marshal.ThrowExceptionForHR(hr);
    } finally {
      if(pc!=IntPtr.Zero && close!=null) close(pc);
      foreach(var h in new[]{ir,iw,outputRead,ow}) if(h!=IntPtr.Zero) CloseHandle(h);
      if(lib!=IntPtr.Zero) FreeLibrary(lib);
    }
  }
}
'@
    [ConptyCompatibilityProbe]::Run((Join-Path $out 'conpty.dll'))
    Write-Output 'PASS: legacy DLL exports; CreatePseudoConsole(flags=6); resize; close.'
    $entries = @('cc-desk-paste-trace.exe','conpty.dll','OpenConsole.exe') | ForEach-Object {
        $item = Get-Item -LiteralPath (Join-Path $out $_)
        [ordered]@{ name=$item.Name; bytes=$item.Length; sha256=(Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash; fileVersion=$item.VersionInfo.FileVersion }
    }
    [ordered]@{
        baseBuild='0be6078480865bb3bd75bd4810c659876c6f5408'; packagingBuild=$env:GITHUB_SHA;
        microsoftPackage=$url; microsoftPackageSha256=$packageHash; licenseSource=$licenseUrl;
        validation='Legacy exports and host lifecycle only; NOT a verified fix';
        runnerOs=[Environment]::OSVersion.VersionString; files=$entries
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $out 'MANIFEST.json') -Encoding utf8
    $entries | ForEach-Object { "$($_.sha256)  $($_.name)" } | Set-Content -LiteralPath (Join-Path $out 'SHA256SUMS.txt') -Encoding utf8
    if ((Get-FileHash -LiteralPath (Join-Path $out 'cc-desk-paste-trace.exe')).Hash -ne $exeHash) { throw 'Application changed unexpectedly' }
    # User invokes while app is running. No policy or system configuration changes.
    $check = @'
@echo off
cd /d "%~dp0"
powershell.exe -NoProfile -Command "$ErrorActionPreference='Stop'; try { $root=(Get-Location).Path; $exe=Join-Path $root 'cc-desk-paste-trace.exe'; $dll=Join-Path $root 'conpty.dll'; $p=Get-Process -Name 'cc-desk-paste-trace' -ErrorAction Stop | Where-Object { $_.Path -eq $exe } | Select-Object -First 1; if (!$p) { throw 'Start the app from this folder and open a Claude session first.' }; $m=$p.Modules | Where-Object { $_.ModuleName -ieq 'conpty.dll' } | Select-Object -First 1; if (!$m -or $m.FileName -ine $dll) { throw 'Local ConPTY is not loaded. Do not treat this as the new-backend test.' }; @('conpty_backend=local_verified', ('dll_sha256='+(Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash), ('dll_version='+$m.FileVersionInfo.FileVersion)) | Tee-Object -FilePath 'backend-check.txt' } catch { Write-Host ('CHECK FAILED: '+$_.Exception.Message); exit 1 }"
pause
'@
    [IO.File]::WriteAllText((Join-Path $out 'Check-Backend.cmd'), ($check -replace "`r?`n","`r`n"), [Text.Encoding]::ASCII)
    Get-Content -LiteralPath (Join-Path $out 'MANIFEST.json')
} finally {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
