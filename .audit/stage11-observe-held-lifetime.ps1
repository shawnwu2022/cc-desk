# One-shot read-only observation of the already-created stage11 parent.
# No process termination, memory dump, handle-source close, or default-UDF access.
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
public static class Stage11HeldLifetime {
  [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint a,bool inherit,uint pid);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool QueryFullProcessImageNameW(IntPtr p,uint f,StringBuilder b,ref uint n);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr p,out long c,out long e,out long k,out long u);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool DuplicateHandle(IntPtr source,IntPtr original,IntPtr target,out IntPtr duplicate,uint access,bool inherit,uint options);
  [DllImport("kernel32.dll",SetLastError=true)] static extern uint GetProcessId(IntPtr p);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr p,out uint code);
  [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr h,uint ms);
  [DllImport("kernel32.dll")] static extern uint PssCaptureSnapshot(IntPtr p,uint flags,uint context,out IntPtr snapshot);
  [DllImport("kernel32.dll")] static extern uint PssFreeSnapshot(IntPtr p,IntPtr snapshot);
  [DllImport("kernel32.dll")] static extern uint PssWalkMarkerCreate(IntPtr allocator,out IntPtr marker);
  [DllImport("kernel32.dll")] static extern uint PssWalkMarkerFree(IntPtr marker);
  [DllImport("kernel32.dll")] static extern uint PssWalkSnapshot(IntPtr snapshot,int info,IntPtr marker,IntPtr buffer,uint length);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job,int info,byte[] buffer,uint length,out uint returned);
  // Windows SDK 10.0.26100.0 processsnapshot.h; x64 layout, no PEB/memory capture.
  [StructLayout(LayoutKind.Explicit,Size=136)] struct Entry {
    [FieldOffset(0)] public IntPtr Handle;
    [FieldOffset(8)] public uint Flags;
    [FieldOffset(12)] public uint ObjectType;
    [FieldOffset(56)] public ushort TypeNameLength;
    [FieldOffset(64)] public IntPtr TypeName;
    [FieldOffset(72)] public ushort ObjectNameLength;
    [FieldOffset(80)] public IntPtr ObjectName;
    [FieldOffset(88)] public uint ProcessExit;
    [FieldOffset(116)] public uint ProcessId;
    [FieldOffset(120)] public uint ParentProcessId;
  }
  static void Check(bool ok,string stage) { if(!ok) throw new InvalidOperationException(stage+":"+Marshal.GetLastWin32Error()); }
  static string Text(IntPtr p,int bytes,int max) {
    if(bytes<0||bytes>max||(bytes&1)!=0||(bytes>0&&p==IntPtr.Zero)) throw new InvalidOperationException("PSS_TEXT_INVALID");
    return bytes==0?"":Marshal.PtrToStringUni(p,bytes/2);
  }
  static string Image(IntPtr p) { uint n=32768; var b=new StringBuilder((int)n);Check(QueryFullProcessImageNameW(p,0,b,ref n),"IMAGE");return b.ToString(); }
  static byte[] JobInfo(IntPtr j,int kind,int size) { var b=new byte[size];uint n;Check(QueryInformationJobObject(j,kind,b,(uint)size,out n),"JOB_QUERY_"+kind);if(n>size)throw new InvalidOperationException("JOB_QUERY_TRUNCATED");return b; }
  public static object Observe() {
    if(IntPtr.Size!=8)throw new InvalidOperationException("X64_REQUIRED");
    const uint pid=52908;
    const string image=@"E:\Codex\2026-11-06\task-2\cc-desk\src-tauri\target\debug\deps\cc_desk-e220344963676309.exe";
    const string task=@"\v027-codex-delete-live-Q8q5x9\temporary\v027-codex-delete-live-9EZwNW";
    long expected=DateTimeOffset.Parse("2026-11-06T16:41:05.3893493Z").UtcDateTime.ToFileTimeUtc();
    // Existing caller token only: QUERY_INFORMATION, DUP_HANDLE, SYNCHRONIZE.
    // No VM_READ, debug privilege, ACL edit, or escalation. Denial is terminal.
    var parent=OpenProcess(0x100440,false,pid);Check(parent!=IntPtr.Zero,"OPEN_EXACT_PARENT");
    IntPtr snapshot=IntPtr.Zero,marker=IntPtr.Zero,buffer=IntPtr.Zero;
    try {
      long created,exit,kernel,user;Check(GetProcessTimes(parent,out created,out exit,out kernel,out user),"PARENT_TIMES");
      if(created!=expected||!String.Equals(Image(parent),image,StringComparison.OrdinalIgnoreCase)||WaitForSingleObject(parent,0)!=258)throw new InvalidOperationException("PARENT_IDENTITY_CHANGED");
      uint code=PssCaptureSnapshot(parent,0x3c,0,out snapshot);if(code!=0)throw new InvalidOperationException("PSS_CAPTURE:"+code);
      code=PssWalkMarkerCreate(IntPtr.Zero,out marker);if(code!=0)throw new InvalidOperationException("PSS_MARKER:"+code);
      buffer=Marshal.AllocHGlobal(136);var jobs=new List<Entry>();var processes=new List<Entry>();int taskHandles=0,total=0;
      while(true) {
        code=PssWalkSnapshot(snapshot,2,marker,buffer,136);if(code==259)break;if(code!=0)throw new InvalidOperationException("PSS_WALK:"+code);
        if(++total>16384)throw new InvalidOperationException("PSS_HANDLE_LIMIT");
        var item=Marshal.PtrToStructure<Entry>(buffer);
        string type=(item.Flags&1)!=0?Text(item.TypeName,item.TypeNameLength,256):"";
        if(type=="Job")jobs.Add(item);
        if(type=="Process")processes.Add(item);
        if(type=="File"&&(item.Flags&2)!=0&&Text(item.ObjectName,item.ObjectNameLength,32768).IndexOf(task,StringComparison.OrdinalIgnoreCase)>=0)taskHandles++;
      }
      if(taskHandles==0||jobs.Count!=1)throw new InvalidOperationException("EXACT_TASK_HANDLE_BINDING_UNPROVEN:"+taskHandles+":"+jobs.Count);
      var output=new Dictionary<string,object>{{"utc",DateTime.UtcNow.ToString("O")},{"parentPid",pid},{"parentCreatedUtc",DateTime.FromFileTimeUtc(created).ToString("O")},{"parentImage",image},{"captureFlags","0x3c: handle metadata only"},{"taskPathFileHandles",taskHandles},{"jobHandleCount",jobs.Count}};
      IntPtr job;Check(DuplicateHandle(parent,jobs[0].Handle,GetCurrentProcess(),out job,4,false,0),"DUP_QUERY_ONLY_JOB");
      try {
        var accounting=JobInfo(job,1,48);var limits=JobInfo(job,9,144);var list=JobInfo(job,3,4104);
        uint assigned=BitConverter.ToUInt32(list,0),listed=BitConverter.ToUInt32(list,4);if(assigned!=listed||listed>512)throw new InvalidOperationException("JOB_PROCESS_LIST_TRUNCATED");
        var ids=new List<ulong>();for(int i=0;i<listed;i++)ids.Add(BitConverter.ToUInt64(list,8+i*8));
        output["jobOriginalHandle"]=jobs[0].Handle.ToInt64().ToString("x");output["jobTotalProcesses"]=BitConverter.ToUInt32(accounting,36);output["jobActiveProcesses"]=BitConverter.ToUInt32(accounting,40);output["jobTerminatedProcesses"]=BitConverter.ToUInt32(accounting,44);output["jobLimitFlags"]=BitConverter.ToUInt32(limits,16);output["jobAssignedProcesses"]=assigned;output["jobProcessIds"]=ids;
      } finally {CloseHandle(job);}
      var children=new List<object>();
      foreach(var item in processes) {
        if(item.ObjectType!=1||(item.Flags&8)==0||item.ParentProcessId!=pid)continue;
        IntPtr process;Check(DuplicateHandle(parent,item.Handle,GetCurrentProcess(),out process,0x101000,false,0),"DUP_QUERY_ONLY_CHILD");
        try {
          if(GetProcessId(process)!=item.ProcessId)throw new InvalidOperationException("CHILD_HANDLE_CHANGED");
          uint childExit;Check(GetExitCodeProcess(process,out childExit),"CHILD_EXIT");
          long c,e,k,u;Check(GetProcessTimes(process,out c,out e,out k,out u),"CHILD_TIMES");
          children.Add(new {pid=item.ProcessId,parentPid=item.ParentProcessId,originalHandle=item.Handle.ToInt64().ToString("x"),pssExit=item.ProcessExit,exitCode=childExit,wait=WaitForSingleObject(process,0),createdUtc=DateTime.FromFileTimeUtc(c).ToString("O"),exitedUtc=e==0?null:DateTime.FromFileTimeUtc(e).ToString("O"),image=Image(process)});
        } finally {CloseHandle(process);}
      }
      output["heldDirectChildren"]=children;output["ownershipLimit"]="Own parent/held Job/held direct children only; no claim about unknown or shared browser instances; original handles unchanged.";return output;
    } finally {
      if(buffer!=IntPtr.Zero)Marshal.FreeHGlobal(buffer);
      if(marker!=IntPtr.Zero)PssWalkMarkerFree(marker);
      if(snapshot!=IntPtr.Zero)PssFreeSnapshot(GetCurrentProcess(),snapshot);
      CloseHandle(parent);
    }
  }
}
'@
[Stage11HeldLifetime]::Observe() | ConvertTo-Json -Depth 8
