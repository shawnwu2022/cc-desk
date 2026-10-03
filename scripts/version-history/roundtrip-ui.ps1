# Windows-only native observation and UI Automation support. Dot-source from roundtrip.ps1.
# No observation returned here grants coordinator or document authority.
Set-StrictMode -Version Latest

function Initialize-RoundtripNative {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
    if ('RoundtripNative' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Text;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Security.Principal;
using Microsoft.Win32.SafeHandles;

public static class RoundtripNative {
    const uint FILE_SHARE_READ = 1;
    const uint FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000;
    const uint FILE_FLAG_BACKUP_SEMANTICS = 0x02000000;
    const int FileStreamInfo = 7;
    public const int MaxEntries = 100000, MaxDepth = 128;
    public const long MaxBytes = 17179869184L, MaxFileBytes = 8589934592L;
    static readonly IntPtr HKCU = new IntPtr(unchecked((int)0x80000001));
    static readonly IntPtr HKU = new IntPtr(unchecked((int)0x80000003));
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern SafeFileHandle CreateFileW(string path, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle h, int kind, byte[] buffer, uint size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint GetFileType(SafeFileHandle h);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern uint GetFinalPathNameByHandleW(SafeFileHandle h, StringBuilder text, uint size, uint flags);
    [DllImport("advapi32.dll")] static extern uint GetSecurityInfo(SafeFileHandle h, int kind, uint information, out IntPtr owner, out IntPtr group, out IntPtr dacl, out IntPtr sacl, out IntPtr descriptor);
    [DllImport("advapi32.dll")] static extern uint GetSecurityDescriptorLength(IntPtr descriptor);
    [DllImport("advapi32.dll")] static extern bool IsValidSecurityDescriptor(IntPtr descriptor);
    [DllImport("advapi32.dll")] static extern bool IsValidSid(IntPtr sid);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr value);
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool contained);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(IntPtr process, out long created, out long exited, out long kernel, out long user);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool QueryFullProcessImageNameW(IntPtr process, uint flags, StringBuilder image, ref uint size);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError=true)] static extern bool GetTokenInformation(IntPtr token, int kind, IntPtr information, uint size, out uint needed);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr OpenJobObjectW(uint access, bool inherit, string name);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job, int kind, byte[] bytes, uint length, out uint returned);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] static extern int RegOpenKeyExW(IntPtr root, string name, uint options, uint access, out IntPtr key);
    [DllImport("advapi32.dll")] static extern int RegCloseKey(IntPtr key);
    [DllImport("advapi32.dll")] static extern int RegGetKeySecurity(IntPtr key, uint information, byte[] descriptor, ref uint size);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] static extern int RegQueryValueExW(IntPtr key, string name, IntPtr reserved, out uint kind, byte[] data, ref uint size);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] static extern int RegEnumValueW(IntPtr key, uint index, StringBuilder name, ref uint length, IntPtr reserved, IntPtr kind, IntPtr data, IntPtr size);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] static extern int RegEnumKeyExW(IntPtr key, uint index, StringBuilder name, ref uint length, IntPtr reserved, IntPtr cls, IntPtr clsLength, IntPtr time);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode)] static extern int RegQueryInfoKeyW(IntPtr key, StringBuilder cls, ref uint clsLength, IntPtr reserved, IntPtr subkeys, IntPtr maxSubkey, IntPtr maxClass, IntPtr values, IntPtr maxValueName, IntPtr maxValue, IntPtr securityLength, out long time);
    [DllImport("ntdll.dll")] static extern int NtQueryKey(IntPtr key, int kind, byte[] data, uint size, out uint needed);
    [DllImport("shell32.dll")] static extern int SHGetKnownFolderPath(ref Guid folder, uint flags, IntPtr token, out IntPtr path);
    [DllImport("ole32.dll")] static extern void CoTaskMemFree(IntPtr path);
    static Exception Block(string detail) { return new InvalidOperationException("NATIVE_CAPTURE_BLOCKED: " + detail); }
    sealed class Utf8Order : IComparer<string> {
        public int Compare(string left,string right) {
            var utf8=new UTF8Encoding(false,true); byte[] a=utf8.GetBytes(left),b=utf8.GetBytes(right);
            for(int i=0;i<Math.Min(a.Length,b.Length);i++) if(a[i]!=b[i]) return a[i]<b[i] ? -1:1;
            return a.Length.CompareTo(b.Length);
        }
    }
    static readonly IComparer<string> NativeOrder=new Utf8Order();
    static void Win(bool ok) { if (!ok) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    static void Reg(int status) { if (status != 0) throw new Win32Exception(status); }
    public static Dictionary<string,object> Map(params object[] pairs) {
        var result = new Dictionary<string,object>();
        for (int i=0; i<pairs.Length; i+=2) result.Add((string)pairs[i], pairs[i+1]);
        return result;
    }
    public static string Hash(byte[] bytes) { using (var sha = SHA256.Create()) return BitConverter.ToString(sha.ComputeHash(bytes)).Replace("-", "").ToLowerInvariant(); }
    public static string Json(object value) {
        if (value == null) return "null";
        if (value is string) {
            var b = new StringBuilder("\"");
            foreach (char c in (string)value) {
                switch(c) {
                    case '"': b.Append("\\\""); break; case '\\': b.Append("\\\\"); break;
                    case '\b': b.Append("\\b"); break; case '\f': b.Append("\\f"); break;
                    case '\n': b.Append("\\n"); break; case '\r': b.Append("\\r"); break; case '\t': b.Append("\\t"); break;
                    default: if (c < 32) b.Append("\\u" + ((int)c).ToString("x4")); else b.Append(c); break;
                }
            }
            return b.Append('"').ToString();
        }
        if (value is bool) return (bool)value ? "true" : "false";
        if (value is IDictionary) {
            var parts = new List<string>();
            foreach (DictionaryEntry pair in (IDictionary)value) parts.Add(Json(pair.Key) + ":" + Json(pair.Value));
            return "{" + String.Join(",", parts.ToArray()) + "}";
        }
        if (value is IEnumerable) {
            var parts = new List<string>(); foreach (object item in (IEnumerable)value) parts.Add(Json(item));
            return "[" + String.Join(",", parts.ToArray()) + "]";
        }
        return Convert.ToString(value, System.Globalization.CultureInfo.InvariantCulture);
    }
    public static string Identity(object value) { return Hash(new UTF8Encoding(false, true).GetBytes(Json(value))); }
    static byte[] Information(SafeFileHandle h, int kind, int length) { var bytes = new byte[length]; Win(GetFileInformationByHandleEx(h, kind, bytes, (uint)length)); return bytes; }
    static string FinalPath(SafeFileHandle h) {
        var text = new StringBuilder(32768); uint count = GetFinalPathNameByHandleW(h, text, 32768, 1);
        if (count == 0) throw new Win32Exception(Marshal.GetLastWin32Error());
        if (count >= 32768 || !text.ToString().StartsWith(@"\\?\Volume{")) throw Block("non-local canonical volume path");
        return text.ToString();
    }
    static byte[] Security(SafeFileHandle h) {
        IntPtr owner, group, dacl, sacl, sd; uint status = GetSecurityInfo(h, 1, 7, out owner, out group, out dacl, out sacl, out sd);
        if (status != 0) throw new Win32Exception((int)status);
        try {
            uint count = GetSecurityDescriptorLength(sd);
            if (!IsValidSecurityDescriptor(sd) || !IsValidSid(owner) || !IsValidSid(group) || dacl == IntPtr.Zero || count < 20 || count > 65536) throw Block("unsupported file security");
            var bytes = new byte[count]; Marshal.Copy(sd, bytes, 0, (int)count);
            if ((BitConverter.ToUInt16(bytes,2) & 0x8000) == 0) throw Block("not self-relative security");
            return bytes;
        } finally { LocalFree(sd); }
    }
    sealed class Held : IDisposable {
        public SafeFileHandle Handle; public string Path; public Dictionary<string,object> FileIdentity; public byte[] Id, Standard, Tag, Descriptor;
        public bool Directory; public long Size; public uint Attributes;
        public Held(string path, bool ancestor, bool writable = false) {
            Handle = CreateFileW(path, writable ? 0x00120083U : 0x00120081U, writable ? 0U : ancestor ? FILE_SHARE_READ | 2 : FILE_SHARE_READ, IntPtr.Zero, 3, FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS, IntPtr.Zero);
            if (Handle.IsInvalid) { int error = Marshal.GetLastWin32Error(); Handle.Dispose(); throw new Win32Exception(error); }
            try {
                if (GetFileType(Handle) != 1) throw Block("not a disk object");
                Id = Information(Handle,18,24); Standard = Information(Handle,1,24); Tag = Information(Handle,9,8);
                Directory = Standard[21] != 0; Size = BitConverter.ToInt64(Standard,8); Attributes = BitConverter.ToUInt32(Tag,0);
                if ((Attributes & ~0x20b7U) != 0 || BitConverter.ToUInt32(Tag,4) != 0 || Standard[20] != 0 || Size < 0 || (!Directory && BitConverter.ToUInt32(Standard,16) != 1) || Directory != ((Attributes & 16) != 0)) throw Block("reparse/type/link/attribute/delete-pending refusal");
                Path = FinalPath(Handle); Descriptor = Security(Handle);
                var identifier = new byte[16]; Array.Copy(Id,8,identifier,0,16);
                FileIdentity = Map("volume", BitConverter.ToUInt64(Id,0), "id", identifier);
                CheckStreams();
            } catch { Handle.Dispose(); throw; }
        }
        void CheckStreams() {
            var bytes = new byte[65536];
            if (!GetFileInformationByHandleEx(Handle,FileStreamInfo,bytes,65536)) {
                if (Directory && Marshal.GetLastWin32Error() == 38) return;
                throw new Win32Exception(Marshal.GetLastWin32Error());
            }
            uint next = BitConverter.ToUInt32(bytes,0), length = BitConverter.ToUInt32(bytes,4);
            if (Directory && length == 0 && next == 0) return;
            if (Directory || next != 0 || length == 0 || length % 2 != 0 || length > 65512 || Encoding.Unicode.GetString(bytes,24,(int)length) != "::$DATA" || BitConverter.ToInt64(bytes,8) != Size) throw Block("named or unsupported NTFS stream");
        }
        public void Verify() {
            if (Hash(Id) != Hash(Information(Handle,18,24)) || Hash(Standard) != Hash(Information(Handle,1,24)) || Hash(Tag) != Hash(Information(Handle,9,8)) || Path != FinalPath(Handle) || Hash(Descriptor) != Hash(Security(Handle))) throw Block("held object changed");
            CheckStreams();
        }
        public void Dispose() { Handle.Dispose(); }
        public byte[] Read(long maximum) {
            if (Directory || Size > maximum) throw Block("file read bound");
            Verify(); var bytes = new byte[(int)Size];
            using (var stream = new FileStream(new SafeFileHandle(Handle.DangerousGetHandle(),false),FileAccess.Read)) {
                int offset=0; while (offset < bytes.Length) { int count=stream.Read(bytes,offset,bytes.Length-offset); if (count == 0) throw Block("file shrank"); offset+=count; }
                if (stream.ReadByte() != -1) throw Block("file grew");
            }
            Verify(); return bytes;
        }
        public string Digest() {
            Verify(); string digest;
            using (var stream = new FileStream(new SafeFileHandle(Handle.DangerousGetHandle(),false),FileAccess.Read)) using (var hash = SHA256.Create()) {
                if (stream.Length != Size) throw Block("file size changed");
                digest = BitConverter.ToString(hash.ComputeHash(stream)).Replace("-", "").ToLowerInvariant();
                if (stream.Position != Size) throw Block("file digest length changed");
            }
            Verify(); return digest;
        }
    }
    static List<Held> Ancestors(string path) {
        string full = System.IO.Path.GetFullPath(path);
        string root;
        if(full.StartsWith(@"\\?\Volume{")) {
            int end=full.IndexOf("}\\",StringComparison.Ordinal); Guid volume;
            if(end < 0 || !Guid.TryParse(full.Substring(11,end-11),out volume)) throw Block("invalid canonical volume path");
            root=full.Substring(0,end+2);
        } else { if(full.StartsWith(@"\\")) throw Block("driver target requires a local volume path"); root=System.IO.Path.GetPathRoot(full); }
        if(full.Length > 32760 || String.IsNullOrEmpty(root)) throw Block("native path bound");
        var held = new List<Held>();
        try {
            held.Add(new Held(root,true)); string parent = System.IO.Path.GetDirectoryName(full);
            if (parent != null && parent.Length > root.Length) {
                string current=root; foreach(string component in parent.Substring(root.Length).Split('\\')) { current=System.IO.Path.Combine(current,component); var item=new Held(current,true); if(!item.Directory) { item.Dispose(); throw Block("ancestor is not a directory"); } held.Add(item); }
            }
            return held;
        } catch { foreach(var item in held) item.Dispose(); throw; }
    }
    static void Walk(Held item, string relative, List<Held> held, List<object> entries, ref long total, int depth) {
        if (depth > MaxDepth || entries.Count >= MaxEntries) throw Block("tree entry/depth bound");
        item.Verify(); if(!item.Directory) { if(item.Size > MaxFileBytes || total > MaxBytes-item.Size) throw Block("tree byte bound"); total+=item.Size; }
        var metadata=Map("path",relative,"kind",item.Directory ? "Directory":"File","size",item.Directory ? 0:item.Size,"object_identity",Identity(item.FileIdentity),"link_count",1,"permissions",Map("Windows",Map("descriptor",item.Descriptor,"attributes",item.Attributes)));
        entries.Add(Map("metadata",metadata,"sha256",item.Directory ? null:item.Digest()));
        if(item.Directory) {
            var children=System.IO.Directory.GetFileSystemEntries(item.Path); Array.Sort(children,NativeOrder);
            foreach(string child in children) {
                string leaf=System.IO.Path.GetFileName(child);
                if(String.IsNullOrEmpty(leaf) || leaf == "." || leaf == ".." || leaf.IndexOf(':') >= 0) throw Block("unsupported component");
                var next=new Held(child,false); held.Add(next);
                Walk(next,relative.Length == 0 ? leaf:relative+"/"+leaf,held,entries,ref total,depth+1);
            }
            var after=System.IO.Directory.GetFileSystemEntries(item.Path); Array.Sort(after,NativeOrder);
            if(Json(children) != Json(after)) throw Block("directory enumeration changed");
        }
        item.Verify();
    }
    public static Dictionary<string,object> CaptureTree(string path) {
        var held=Ancestors(path);
        try {
            Held root;
            try { root=new Held(path,false); }
            catch(Win32Exception e) {
                if(e.NativeErrorCode != 2) throw;
                var parent=held[held.Count-1]; string leaf=System.IO.Path.GetFileName(path);
                // Recheck exact absence, not an access error or a missing ancestor.
                try { using(var unexpected=new Held(path,false)) { throw Block("absent root appeared"); } }
                catch(Win32Exception second) { if(second.NativeErrorCode != 2) throw; }
                foreach(var item in held) item.Verify();
                return Map("canonicalPath",System.IO.Path.Combine(parent.Path,leaf),"fileIdentity",null,"manifest",Map("schema",1,"location_identity",Identity(new object[]{"absent",parent.FileIdentity,parent.Path,leaf}),"entries",new object[0]));
            }
            held.Add(root); if(!root.Directory) throw Block("tree root is not directory");
            var entries=new List<object>(); long total=0; Walk(root,"",held,entries,ref total,0);
            entries.Sort(delegate(object a,object b) { return NativeOrder.Compare((string)((Dictionary<string,object>)((Dictionary<string,object>)a)["metadata"])["path"],(string)((Dictionary<string,object>)((Dictionary<string,object>)b)["metadata"])["path"]); });
            foreach(var item in held) item.Verify();
            return Map("canonicalPath",root.Path,"fileIdentity",root.FileIdentity,"manifest",Map("schema",1,"location_identity",Identity(new object[]{"present",root.FileIdentity,root.Path}),"entries",entries));
        } finally { foreach(var item in held) item.Dispose(); }
    }
    public static string KnownFolder(string slot) {
        var guid=new Guid(slot == "Desktop" ? "B4BFCC3A-DB2C-424C-B029-7FE99A87C641" : slot == "StartMenu" ? "A77F5D77-2E2B-44C3-A6A2-ABA601054A51" : throw Block("unknown shortcut slot"));
        IntPtr path; int status=SHGetKnownFolderPath(ref guid,0,IntPtr.Zero,out path); if(status != 0) Marshal.ThrowExceptionForHR(status);
        try { return Marshal.PtrToStringUni(path); } finally { CoTaskMemFree(path); }
    }
    public static Dictionary<string,object> CaptureShortcut(string slot) {
        string directory=KnownFolder(slot); var held=Ancestors(System.IO.Path.Combine(directory,"CC Desk.lnk"));
        try {
            var parent=held[held.Count-1]; object state;
            try {
                using(var file=new Held(System.IO.Path.Combine(directory,"CC Desk.lnk"),false)) {
                    if(file.Attributes != 32 && file.Attributes != 128) throw Block("unsupported shortcut attributes");
                    byte[] bytes=file.Read(1048576);
                    state=Map("Present",Map("identity",file.FileIdentity,"attributes",file.Attributes,"bytes",bytes,"sha256",Hash(bytes),"descriptor",file.Descriptor));
                }
            } catch(Win32Exception e) { if(e.NativeErrorCode != 2) throw; state="Absent"; }
            foreach(var item in held) item.Verify();
            return Map("slot",slot,"parent",parent.FileIdentity,"parent_path",ToUnits(parent.Path),"leaf","CC Desk.lnk","state",state);
        } finally { foreach(var item in held) item.Dispose(); }
    }
    public static byte[] ReadBoundedFile(string path, int maximum) {
        var held=Ancestors(path);
        try { using(var file=new Held(path,false)) { byte[] bytes=file.Read(maximum); foreach(var item in held) item.Verify(); return bytes; } }
        finally { foreach(var item in held) item.Dispose(); }
    }
    public static void RequireSentinelDisjoint(string canonicalFile,string[] canonicalRoots) {
        if(canonicalRoots == null || canonicalRoots.Length != 6 || String.IsNullOrEmpty(canonicalFile)) throw Block("synthetic sentinel root contract");
        foreach(string root in canonicalRoots) {
            if(String.IsNullOrEmpty(root)) throw Block("synthetic sentinel root contract");
            string prefix=root.TrimEnd('\\');
            if(String.Equals(canonicalFile,prefix,StringComparison.OrdinalIgnoreCase) || canonicalFile.StartsWith(prefix+"\\",StringComparison.OrdinalIgnoreCase)) throw Block("synthetic sentinel overlaps a protected root");
        }
    }
    public static void WriteSyntheticSentinel(string path,string expectedSha256,byte[] replacement,string[] protectedRoots) {
        if(replacement.Length > 65536) throw Block("synthetic sentinel byte bound");
        var held=Ancestors(path);
        try {
            if(protectedRoots == null || protectedRoots.Length != 6) throw Block("synthetic sentinel root contract");
            var canonical=new List<string>();
            foreach(string protectedPath in protectedRoots) {
                var ancestors=Ancestors(protectedPath); held.AddRange(ancestors);
                try {
                    var root=new Held(protectedPath,true); held.Add(root);
                    if(!root.Directory) throw Block("protected root is not a directory");
                    canonical.Add(root.Path);
                } catch(Win32Exception e) {
                    // Exact absent roots still protect their original name.
                    // Inaccessible or missing ancestors never mean absence.
                    if(e.NativeErrorCode != 2) throw;
                    canonical.Add(System.IO.Path.Combine(ancestors[ancestors.Count-1].Path,System.IO.Path.GetFileName(protectedPath)));
                }
            }
            using(var file=new Held(path,false,true)) {
                RequireSentinelDisjoint(file.Path, canonical.ToArray());
                if(file.Directory || file.Size > 65536 || file.Digest() != expectedSha256) throw Block("synthetic sentinel identity/content changed");
                using(var stream=new FileStream(new SafeFileHandle(file.Handle.DangerousGetHandle(),false),FileAccess.ReadWrite)) {
                    stream.Position=0; stream.SetLength(0); stream.Write(replacement,0,replacement.Length); stream.Flush(true);
                }
                if(Hash(file.Id) != Hash(Information(file.Handle,18,24)) || file.Path != FinalPath(file.Handle) || Hash(file.Descriptor) != Hash(Security(file.Handle))) throw Block("synthetic sentinel object/security changed");
            }
            foreach(var item in held) item.Verify();
        } finally { foreach(var item in held) item.Dispose(); }
    }
    static ushort[] ToUnits(string text) { var result=new ushort[text.Length]; for(int i=0;i<text.Length;i++) result[i]=text[i]; return result; }
    static byte[] RegSecurity(IntPtr key) {
        uint count=0; int status=RegGetKeySecurity(key,7,null,ref count);
        if(status != 122 || count < 20 || count > 65536) throw Block("registry security bound");
        var bytes=new byte[count]; Reg(RegGetKeySecurity(key,7,bytes,ref count)); if(count != bytes.Length) throw Block("registry security changed size"); return bytes;
    }
    static object RegValue(IntPtr key, string name) {
        uint count=0, kind; int status=RegQueryValueExW(key,name,IntPtr.Zero,out kind,null,ref count);
        if(status == 2) return null; Reg(status); if(count > 1048576) throw Block("registry value bound");
        var bytes=new byte[count]; uint expectedKind=kind; Reg(RegQueryValueExW(key,name,IntPtr.Zero,out kind,bytes,ref count));
        if(count != bytes.Length || kind != expectedKind) throw Block("registry value changed size/type");
        return Map("kind",kind,"bytes",bytes);
    }
    static ushort[] Namespace(IntPtr key) {
        uint count; var bytes=new byte[65536]; int status=NtQueryKey(key,3,bytes,65536,out count);
        if(status < 0 || count < 4 || count > 65536) throw Block("native registry namespace unavailable");
        uint length=BitConverter.ToUInt32(bytes,0); if(length % 2 != 0 || length+4 != count) throw Block("native registry name bound");
        return ToUnits(Encoding.Unicode.GetString(bytes,4,(int)length));
    }
    sealed class KeyGuard {
        public IntPtr Key; string name, link; long stamp;
        public KeyGuard(IntPtr key) { Key=key; name=Json(Namespace(key)); link=Json(RegValue(key,"SymbolicLinkValue")); stamp=Stamp(key); }
        public void Verify() { if(name != Json(Namespace(Key)) || link != Json(RegValue(Key,"SymbolicLinkValue")) || stamp != Stamp(Key)) throw Block("registry key changed"); }
    }
    static long Stamp(IntPtr key) { uint length=0; long stamp; Reg(RegQueryInfoKeyW(key,null,ref length,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,out stamp)); return stamp; }
    static IntPtr OpenPath(string path,string sid,List<KeyGuard> guards) {
        IntPtr parent=HKCU; var parts=path.Split('\\');
        for(int i=0;i<parts.Length;i++) {
            IntPtr key; int status=RegOpenKeyExW(parent,parts[i],8,0x20119|0x100,out key); if(status == 2) return IntPtr.Zero; Reg(status);
            var guard=new KeyGuard(key); guards.Add(guard); var link=RegValue(key,"SymbolicLinkValue") as Dictionary<string,object>;
            if(link != null && Convert.ToUInt32(link["kind"]) == 6) {
                string target=Encoding.Unicode.GetString((byte[])link["bytes"]).TrimEnd('\0');
                if(i != 1 || parts[0] != "Software" || parts[1] != "Classes" || !String.Equals(target,@"\REGISTRY\USER\"+sid+"_Classes",StringComparison.OrdinalIgnoreCase)) throw Block("unsupported registry alias");
                Reg(RegOpenKeyExW(HKU,sid+"_Classes",8,0x20119|0x100,out key)); guards.Add(new KeyGuard(key));
                var nested=RegValue(key,"SymbolicLinkValue") as Dictionary<string,object>; if(nested != null && Convert.ToUInt32(nested["kind"]) == 6) throw Block("linked Classes target");
            }
            parent=key;
        }
        return parent;
    }
    sealed class RegBudget { public int Keys,Values,Bytes; public void Add(int size) { if(size < 0 || Bytes > 4194304-size) throw Block("registry byte bound"); Bytes+=size; } }
    static List<string> Subkeys(IntPtr key) {
        var result=new List<string>();
        for(uint i=0;i<=4096;i++) { uint length=256; var name=new StringBuilder((int)length); int status=RegEnumKeyExW(key,i,name,ref length,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero); if(status == 259) { result.Sort(NativeOrder); return result; } Reg(status); if(i==4096 || length==0 || length>255) throw Block("registry subkey bound"); result.Add(name.ToString()); }
        throw Block("registry enumeration bound");
    }
    static void WalkKey(IntPtr key,List<string> relative,List<object> nodes,List<KeyGuard> guards,RegBudget budget) {
        if(++budget.Keys > 4096 || relative.Count > 32) throw Block("registry key/depth bound");
        var guard=new KeyGuard(key); var link=RegValue(key,"SymbolicLinkValue") as Dictionary<string,object>; if(link != null && Convert.ToUInt32(link["kind"]) == 6) throw Block("linked product key");
        var cls=new StringBuilder(1024); uint clsLength=1024; long stamp; Reg(RegQueryInfoKeyW(key,cls,ref clsLength,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,out stamp));
        var values=new SortedDictionary<string,object>(NativeOrder);
        for(uint i=0;i<=16384;i++) { uint length=32761; var name=new StringBuilder((int)length); int status=RegEnumValueW(key,i,name,ref length,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero); if(status==259) break; Reg(status); if(++budget.Values > 16384) throw Block("registry value count bound"); var value=(Dictionary<string,object>)RegValue(key,name.ToString()); if(value==null) throw Block("registry value disappeared"); budget.Add(Encoding.UTF8.GetByteCount(name.ToString())+((byte[])value["bytes"]).Length); values.Add(name.ToString(),value); }
        var ns=Namespace(key); var security=RegSecurity(key); budget.Add(ns.Length*2+security.Length+Encoding.UTF8.GetByteCount(cls.ToString()));
        nodes.Add(Map("relative",relative.ToArray(),"namespace",ns,"class",cls.ToString(),"security",security,"values",values));
        var children=Subkeys(key); foreach(string child in children) { IntPtr next; Reg(RegOpenKeyExW(key,child,8,0x20119|0x100,out next)); guards.Add(new KeyGuard(next)); var path=new List<string>(relative); path.Add(child); WalkKey(next,path,nodes,guards,budget); }
        if(Json(children) != Json(Subkeys(key))) throw Block("registry enumeration changed"); guard.Verify();
    }
    public static Dictionary<string,object> CaptureRegistration(string sid,object installation) {
        var slots=new string[]{"Uninstall","Publisher","DeskDirectory","DeskDirectoryBackground","LegacyDirectory","LegacyDirectoryBackground"};
        var paths=new string[]{@"Software\Microsoft\Windows\CurrentVersion\Uninstall\CC Desk",@"Software\shawnwu2022\CC Desk",@"Software\Classes\Directory\shell\cc-desk",@"Software\Classes\Directory\Background\shell\cc-desk",@"Software\Classes\Directory\shell\cc-box",@"Software\Classes\Directory\Background\shell\cc-box"};
        var guards=new List<KeyGuard>(); var budget=new RegBudget(); var trees=new List<object>();
        try {
            for(int i=0;i<paths.Length;i++) {
                IntPtr parent=OpenPath(paths[i].Substring(0,paths[i].LastIndexOf('\\')),sid,guards);
                object parentObservation=Map("namespace",parent==IntPtr.Zero ? null:Namespace(parent),"security",parent==IntPtr.Zero ? null:RegSecurity(parent));
                budget.Add(Encoding.UTF8.GetByteCount(Json(parentObservation)));
                IntPtr key=OpenPath(paths[i],sid,guards); var nodes=new List<object>(); if(key!=IntPtr.Zero) WalkKey(key,new List<string>(),nodes,guards,budget);
                trees.Add(Map("slot",slots[i],"parent",parentObservation,"nodes",nodes));
            }
            // OwnedRun captures exactly one value, never the other shared Run values.
            IntPtr run=OpenPath(@"Software\Microsoft\Windows\CurrentVersion\Run",sid,guards);
            object runObservation=Map("namespace",run==IntPtr.Zero ? null:Namespace(run),"parent_security",run==IntPtr.Zero ? null:RegSecurity(run),"value",run==IntPtr.Zero ? null:RegValue(run,"CC Desk"));
            budget.Add(Encoding.UTF8.GetByteCount(Json(runObservation))); foreach(var guard in guards) guard.Verify();
            return Map("schema",2,"user_sid",sid,"installation",installation,"trees",trees,"run",runObservation);
        } finally { foreach(var guard in guards) RegCloseKey(guard.Key); }
    }
    public static Dictionary<string,object> ProcessFacts(uint pid) {
        IntPtr process=OpenProcess(0x1000,false,pid); if(process==IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error()); IntPtr token=IntPtr.Zero;
        try {
            bool job; Win(IsProcessInJob(process,IntPtr.Zero,out job)); Win(OpenProcessToken(process,8,out token));
            uint length; GetTokenInformation(token,1,IntPtr.Zero,0,out length); if(length==0 || length>65536) throw Block("token user bound");
            IntPtr data=Marshal.AllocHGlobal((int)length); string sid;
            try { Win(GetTokenInformation(token,1,data,length,out length)); sid=new SecurityIdentifier(Marshal.ReadIntPtr(data)).Value; } finally { Marshal.FreeHGlobal(data); }
            data=Marshal.AllocHGlobal(4); bool elevated;
            try { Win(GetTokenInformation(token,20,data,4,out length)); elevated=Marshal.ReadInt32(data)!=0; } finally { Marshal.FreeHGlobal(data); }
            var image=new StringBuilder(32768); uint count=32768; Win(QueryFullProcessImageNameW(process,0,image,ref count));
            long created,exited,kernel,user; Win(GetProcessTimes(process,out created,out exited,out kernel,out user));
            return Map("pid",pid,"createdFileTime",created,"imagePath",image.ToString(),"userSid",sid,"elevated",elevated,"inAnyJob",job);
        } finally { if(token!=IntPtr.Zero) CloseHandle(token); CloseHandle(process); }
    }
    public static uint JobActiveProcesses(string name) {
        if(!name.StartsWith("Local\\CCDeskRecovery-",StringComparison.Ordinal) || name.Length > 256) throw Block("unsupported observed owned job name");
        IntPtr job=OpenJobObjectW(4,false,name); if(job==IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
        try { var bytes=new byte[48]; uint count; Win(QueryInformationJobObject(job,1,bytes,48,out count)); if(count!=48) throw Block("job observation size"); return BitConverter.ToUInt32(bytes,40); }
        finally { CloseHandle(job); }
    }
}
'@
}

function Wait-RoundtripCondition {
    param([scriptblock]$Check, [int]$TimeoutSeconds, [string]$Description)
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $value = & $Check
        if ($value) { return $value }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "WAIT_TIMEOUT: $Description; preserve the live transaction and all evidence"
}

function Get-RoundtripWindow {
    param([System.Diagnostics.Process]$Process)
    $Process.Refresh()
    if ($Process.HasExited) { throw 'BLOCKED_UI_SELECTOR: observed process has exited' }
    $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $Process.Id)
    $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
    $visible = @($windows | Where-Object { -not $_.Current.IsOffscreen -and $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Window })
    if ($visible.Count -gt 1) { throw 'BLOCKED_UI_SELECTOR: duplicate top-level windows' }
    if ($visible.Count -eq 1) { return $visible[0] }
    return $null
}

function Find-RoundtripControl {
    param($Window, $Selector, [switch]$AllowDisabled)
    if (-not $Selector -or -not $Selector.name -or -not $Selector.controlType) { throw 'BLOCKED_UI_SELECTOR: missing reviewed native selector' }
    $kind = [System.Windows.Automation.ControlType]::LookupById([int]$Selector.controlType)
    $conditions = [System.Collections.Generic.List[System.Windows.Automation.Condition]]::new()
    $conditions.Add([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, [string]$Selector.name))
    $conditions.Add([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, $kind))
    if ($Selector.automationId) { $conditions.Add([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty, [string]$Selector.automationId)) }
    $condition = [System.Windows.Automation.AndCondition]::new($conditions.ToArray())
    $matches = @($Window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition) | Where-Object { -not $_.Current.IsOffscreen -and ($AllowDisabled -or $_.Current.IsEnabled) })
    if ($matches.Count -gt 1) { throw "BLOCKED_UI_SELECTOR: ambiguous $($Selector.name)" }
    if ($matches.Count -eq 1) { return $matches[0] }
    return $null
}

function Wait-RoundtripControl {
    param([System.Diagnostics.Process]$Process, $Selector, [int]$TimeoutSeconds)
    Wait-RoundtripCondition -TimeoutSeconds $TimeoutSeconds -Description "native control $($Selector.name)" -Check {
        $window = Get-RoundtripWindow -Process $Process
        if ($window) { Find-RoundtripControl -Window $window -Selector $Selector }
    }
}

function Invoke-RoundtripControl {
    param([System.Diagnostics.Process]$Process, $Selector, [int]$TimeoutSeconds)
    $control = Wait-RoundtripControl -Process $Process -Selector $Selector -TimeoutSeconds $TimeoutSeconds
    $control.SetFocus()
    $pattern = $null
    if (-not $control.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { throw "BLOCKED_UI_SELECTOR: $($Selector.name) has no InvokePattern" }
    ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
}

function Set-RoundtripPreference {
    param([System.Diagnostics.Process]$Process, $Selector, [int]$TimeoutSeconds)
    # The reviewed probe chooses the actual historical control and desired native action.
    $control = Wait-RoundtripControl -Process $Process -Selector $Selector -TimeoutSeconds $TimeoutSeconds
    $control.SetFocus(); $pattern = $null
    switch ($Selector.pattern) {
        'Invoke' { Invoke-RoundtripControl -Process $Process -Selector $Selector -TimeoutSeconds $TimeoutSeconds }
        'SelectionItem' {
            if (-not $control.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { throw 'BLOCKED_UI_SELECTOR: no SelectionItemPattern' }
            ([System.Windows.Automation.SelectionItemPattern]$pattern).Select()
        }
        'Value' {
            if (-not $control.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) { throw 'BLOCKED_UI_SELECTOR: no ValuePattern' }
            if ([string]::IsNullOrEmpty($Selector.value)) { throw 'BLOCKED_UI_SELECTOR: missing reviewed preference value' }
            ([System.Windows.Automation.ValuePattern]$pattern).SetValue([string]$Selector.value)
        }
        default { throw 'BLOCKED_UI_SELECTOR: preference native pattern has not been reviewed' }
    }
}

function Close-RoundtripWindow {
    param([System.Diagnostics.Process]$Process)
    $window = Get-RoundtripWindow -Process $Process
    if (-not $window) { throw 'BLOCKED_UI_SELECTOR: no normal window close surface' }
    $pattern = $null
    if (-not $window.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$pattern)) { throw 'BLOCKED_UI_SELECTOR: no WindowPattern; close manually and preserve evidence' }
    ([System.Windows.Automation.WindowPattern]$pattern).Close()
}

function Wait-RoundtripProcessExit {
    param([System.Diagnostics.Process]$Process, [int]$TimeoutSeconds)
    Wait-RoundtripCondition -TimeoutSeconds $TimeoutSeconds -Description "observed process $($Process.Id) exit" -Check { $Process.Refresh(); $Process.HasExited } | Out-Null
}

function Save-RoundtripScreenshot {
    param([System.Diagnostics.Process]$Process, [string]$Path)
    $window = Get-RoundtripWindow -Process $Process
    if (-not $window) { throw 'BLOCKED_UI_SELECTOR: screenshot needs the observed native window' }
    $rectangle = $window.Current.BoundingRectangle
    if ($rectangle.Width -le 0 -or $rectangle.Height -le 0 -or $rectangle.Width -gt 8192 -or $rectangle.Height -gt 8192) { throw 'BLOCKED_UI_SELECTOR: screenshot bounds' }
    $bitmap = [System.Drawing.Bitmap]::new([int]$rectangle.Width, [int]$rectangle.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $window.SetFocus()
        $graphics.CopyFromScreen([int]$rectangle.X, [int]$rectangle.Y, 0, 0, $bitmap.Size)
        if ([IO.File]::Exists($Path)) { throw 'EVIDENCE_EXISTS: screenshots are create-new records' }
        $stream = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        try { $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png); $stream.Flush($true) } finally { $stream.Dispose() }
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
