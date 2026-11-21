# Standalone, read-only shape diagnostic. Not an installer prerequisite or a network probe.
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
if (!$IsWindows -or ![Environment]::Is64BitProcess -or ![Environment]::UserInteractive -or
    [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) { throw 'INTERACTIVE_WINDOWS_X64_REQUIRED' }

$source=@'
using System;
using System.Runtime.InteropServices;

namespace CcDeskProxyShape {
    public sealed class Flags {
        public bool DefaultQueryOk;
        public string DefaultAccessType = "unknown";
        public bool CurrentUserQueryOk;
        public string AutoDetect = "unknown";
        public string ManualProxy = "unknown";
        public string Pac = "unknown";
    }

    public static class ReadOnly {
        [StructLayout(LayoutKind.Sequential)]
        private struct DefaultProxy {
            public uint AccessType;
            public IntPtr Proxy;
            public IntPtr ProxyBypass;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct CurrentUserProxy {
            public int AutoDetect;
            public IntPtr AutoConfigUrl;
            public IntPtr Proxy;
            public IntPtr ProxyBypass;
        }

        [DllImport("winhttp.dll", ExactSpelling = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool WinHttpGetDefaultProxyConfiguration(ref DefaultProxy info);

        [DllImport("winhttp.dll", ExactSpelling = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool WinHttpGetIEProxyConfigForCurrentUser(ref CurrentUserProxy config);

        [DllImport("kernel32.dll", ExactSpelling = true)]
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        private static extern IntPtr GlobalFree(IntPtr handle);

        private static string Presence(bool present) { return present ? "present" : "absent"; }

        private static void Release(IntPtr first, IntPtr second, IntPtr third) {
            if (first != IntPtr.Zero) GlobalFree(first);
            if (second != IntPtr.Zero && second != first) GlobalFree(second);
            if (third != IntPtr.Zero && third != first && third != second) GlobalFree(third);
        }

        public static Flags Query() {
            var flags = new Flags();
            var defaults = new DefaultProxy();
            try {
                flags.DefaultQueryOk = WinHttpGetDefaultProxyConfiguration(ref defaults);
                if (flags.DefaultQueryOk) {
                    // WINHTTP_ACCESS_TYPE_NO_PROXY (1), WINHTTP_ACCESS_TYPE_NAMED_PROXY (3).
                    flags.DefaultAccessType = defaults.AccessType == 1 ? "no-proxy" :
                        defaults.AccessType == 3 ? "named-proxy" : "other";
                }
            } finally {
                Release(defaults.Proxy, defaults.ProxyBypass, IntPtr.Zero);
            }
            var current = new CurrentUserProxy();
            try {
                flags.CurrentUserQueryOk = WinHttpGetIEProxyConfigForCurrentUser(ref current);
                if (flags.CurrentUserQueryOk) {
                    flags.AutoDetect = Presence(current.AutoDetect != 0);
                    flags.ManualProxy = Presence(current.Proxy != IntPtr.Zero);
                    flags.Pac = Presence(current.AutoConfigUrl != IntPtr.Zero);
                }
            } finally {
                Release(current.AutoConfigUrl, current.Proxy, current.ProxyBypass);
            }
            return flags;
        }
    }
}
'@

function Get-PolicyKeyPresence([Microsoft.Win32.RegistryKey]$Hive) {
    $key=$null
    try {
        $key=$Hive.OpenSubKey('Software\Policies\Microsoft\Windows\CurrentVersion\Internet Settings',$false)
        if ($null -eq $key) { return 'absent' }
        return 'present'
    } catch {
        return 'unknown'
    } finally {
        if ($null -ne $key) { $key.Dispose() }
    }
}

try {
    Add-Type -TypeDefinition $source
    $shape=[CcDeskProxyShape.ReadOnly]::Query()
    [ordered]@{
        winHttpDefaultQueryOk=$shape.DefaultQueryOk
        winHttpDefaultAccessType=$shape.DefaultAccessType
        currentUserQueryOk=$shape.CurrentUserQueryOk
        currentUserAutoDetect=$shape.AutoDetect
        currentUserManualProxy=$shape.ManualProxy
        currentUserPac=$shape.Pac
        currentUserPolicyKey=Get-PolicyKeyPresence ([Microsoft.Win32.Registry]::CurrentUser)
        machinePolicyKey=Get-PolicyKeyPresence ([Microsoft.Win32.Registry]::LocalMachine)
    } | ConvertTo-Json -Compress
} catch {
    throw 'PROXY_SHAPE_QUERY_BLOCKED'
}
