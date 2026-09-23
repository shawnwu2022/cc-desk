# Read-only diagnostic for repository-built PE executables. This runs in its
# own PowerShell process after a failed test and never changes test outcomes.
param([Parameter(Mandatory = $true)][string]$Directory)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
public static class DeskLoaderProbe {
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern IntPtr LoadLibraryExW(string name, IntPtr file, uint flags);
  [DllImport("kernel32.dll", CharSet=CharSet.Ansi, ExactSpelling=true, SetLastError=true)]
  public static extern IntPtr GetProcAddress(IntPtr module, string name);
  [DllImport("kernel32.dll", EntryPoint="GetProcAddress", ExactSpelling=true, SetLastError=true)]
  public static extern IntPtr GetOrdinal(IntPtr module, IntPtr ordinal);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  public static extern uint GetModuleFileNameW(IntPtr module, StringBuilder name, int size);
  [DllImport("kernel32.dll")]
  public static extern bool FreeLibrary(IntPtr module);
  static string Text(byte[] data, int start) {
    int end = Array.IndexOf(data, (byte)0, start);
    if (end < start || end - start > 4096) throw new InvalidDataException("PE string");
    return Encoding.ASCII.GetString(data, start, end - start);
  }
  public static Dictionary<string,List<string>> Imports(string path) {
    byte[] data = File.ReadAllBytes(path);
    int pe = BitConverter.ToInt32(data, 60);
    if (BitConverter.ToUInt32(data, pe) != 0x4550) throw new InvalidDataException("PE signature");
    int count = BitConverter.ToUInt16(data, pe+6);
    int optional = pe+24;
    int sections = optional+BitConverter.ToUInt16(data,pe+20);
    bool x64 = BitConverter.ToUInt16(data,optional) == 0x20b;
    Func<uint,int> offset = rva => {
      for (int n=0; n<count; n++) {
        int section=sections+n*40;
        uint va=BitConverter.ToUInt32(data,section+12);
        uint size=Math.Max(BitConverter.ToUInt32(data,section+8),BitConverter.ToUInt32(data,section+16));
        if(rva>=va && (ulong)rva<(ulong)va+size)
          return checked((int)(rva-va+BitConverter.ToUInt32(data,section+20)));
      }
      throw new InvalidDataException("PE RVA");
    };
    var result=new Dictionary<string,List<string>>(StringComparer.OrdinalIgnoreCase);
    uint imports=BitConverter.ToUInt32(data,optional+(x64?112:96)+8);
    if(imports==0) return result;
    int descriptor=offset(imports);
    for(int n=0;n<512;n++,descriptor+=20) {
      uint names=BitConverter.ToUInt32(data,descriptor+12);
      if(names==0) return result;
      string dll=Text(data,offset(names));
      var symbols=new List<string>();
      uint table=BitConverter.ToUInt32(data,descriptor);
      if(table==0) table=BitConverter.ToUInt32(data,descriptor+16);
      int thunk=offset(table);
      for(int t=0;t<16384;t++,thunk+=x64?8:4) {
        ulong item=x64?BitConverter.ToUInt64(data,thunk):BitConverter.ToUInt32(data,thunk);
        if(item==0) break;
        if((item & (x64?0x8000000000000000UL:0x80000000UL))!=0)
          symbols.Add("#"+(item & 0xffff));
        else symbols.Add(Text(data,offset(checked((uint)item))+2));
      }
      result.Add(dll,symbols);
    }
    throw new InvalidDataException("PE descriptor limit");
  }
}
'@
$executables = @(Get-ChildItem -LiteralPath $Directory -Filter 'cc_desk-*.exe' -File)
if ($executables.Count -eq 0) { throw 'No repository test executable found' }
foreach ($exe in $executables) {
    Write-Output "LOADER_PROBE executable=$($exe.Name)"
    $imports = [DeskLoaderProbe]::Imports($exe.FullName)
    foreach ($entry in $imports.GetEnumerator()) {
        $sibling = Join-Path $exe.DirectoryName $entry.Key
        $name = $entry.Key
        [uint32]$flags = 0x800 # System32, including Windows API-set resolution.
        if (Test-Path -LiteralPath $sibling -PathType Leaf) {
            $name = $sibling
            $flags = 0x1100 # Explicit app-local path and its normal dependencies.
        }
        $module = [DeskLoaderProbe]::LoadLibraryExW($name, [IntPtr]::Zero, $flags)
        if ($module -eq [IntPtr]::Zero) {
            $nativeError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            Write-Output "LOADER_PROBE load_failed dll=$($entry.Key) win32=$nativeError"
            continue
        }
        try {
            $resolved = New-Object Text.StringBuilder 32768
            [void][DeskLoaderProbe]::GetModuleFileNameW($module, $resolved, $resolved.Capacity)
            Write-Output "LOADER_PROBE dll=$($entry.Key) resolved=$resolved imports=$($entry.Value.Count)"
            foreach ($symbol in $entry.Value) {
                if ($symbol.StartsWith('#')) {
                    $address = [DeskLoaderProbe]::GetOrdinal($module, [IntPtr]([int]$symbol.Substring(1)))
                } else {
                    $address = [DeskLoaderProbe]::GetProcAddress($module, $symbol)
                }
                if ($address -eq [IntPtr]::Zero) {
                    Write-Output "LOADER_PROBE missing_entry dll=$($entry.Key) symbol=$symbol"
                }
            }
        } finally {
            [void][DeskLoaderProbe]::FreeLibrary($module)
        }
    }
}
