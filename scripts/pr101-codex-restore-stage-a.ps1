# Stage A ONLY. Fresh synthetic data; never accepts an existing HOME/session.
# Draft for review. Does not invoke/change any historical Stage-B Go gate.
param([Parameter(Mandatory=$true)][string]$Codex, [switch]$SameDatasetReadiness, [string]$PreviewWorker, [string]$PreviewWorkerSha256)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (!$IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or
    $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or !$env:RUNNER_TEMP -or
    $env:COMPUTERNAME -in @('WSK-MSI','SXY')) { throw 'STAGE_A_HOST_REJECTED' }
$phase='preflight-temp-boundary'
$imageLease=$null
$previewLease=$null
try {
    function Assert-Ordinary([string]$Path) {
        $item=Get-Item -LiteralPath $Path -Force
        while ($null -ne $item) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'STAGE_A_REPARSE' }
            if ($item -is [IO.FileInfo]) { $item=$item.Directory } else { $item=$item.Parent }
        }
    }
    $temp=[IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')
    Assert-Ordinary $temp
    $phase='preflight-image-path'
    $image=[IO.Path]::GetFullPath($Codex)
    if (!$image.StartsWith($temp+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'STAGE_A_IMAGE_LOCATION' }
    Assert-Ordinary $image
    # Same official 0.169.1 image pinned by verifier commit 3c014ac4.
    # The historical Stage-B 0.169.0 path/hash is NOT compatible with this fixture.
    $phase='preflight-image-sha'
    $imageLease=[IO.File]::Open($image,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    $sha=[Security.Cryptography.SHA256]::Create()
    try { $imageHash=[Convert]::ToHexString($sha.ComputeHash($imageLease)).ToLowerInvariant() } finally { $sha.Dispose() }
    if ($imageHash -cne 'f76f94d08b0793bfa129ae62c09341ee32ae96d57e69968ae6b8d7c71a20d8c4') { throw 'STAGE_A_IMAGE_HASH' }

    # Compile only on disposable hosted Windows. CreateProcess is suspended until
    # assigned to a private Job; all descendants must be gone before copying data.
    $phase='preflight-type-compile'
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Text;
using System.Threading.Tasks;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public sealed class StageAOwned : IDisposable {
 [StructLayout(LayoutKind.Sequential)] struct SA { public int length; public IntPtr security; public int inherit; }
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct SI {
  public int cb; public string reserved, desktop, title; public uint x,y,xsize,ysize,xchars,ychars,fill,flags;
  public short show,reserved2; public IntPtr reservedPtr,input,output,error;
 }
 [StructLayout(LayoutKind.Sequential)] struct SIEX { public SI start; public IntPtr attributes; }
 [StructLayout(LayoutKind.Sequential)] struct PI { public IntPtr process,thread; public uint pid,tid; }
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr p,int count,int flags,ref IntPtr size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr p,uint flags,IntPtr attr,IntPtr value,IntPtr size,IntPtr previous,IntPtr returned);
 [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr p);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CreatePipe(out IntPtr r,out IntPtr w,ref SA a,uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetHandleInformation(IntPtr h,uint mask,uint flags);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr a,string n);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetInformationJobObject(IntPtr j,int c,IntPtr p,uint n);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr j,int c,IntPtr p,uint n,IntPtr ret);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr j,IntPtr p);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateJobObject(IntPtr j,uint c);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateProcess(IntPtr p,uint c);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint ResumeThread(IntPtr h);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr h,uint ms);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr p,out uint c);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcessW(string app,StringBuilder cmd,IntPtr pa,IntPtr ta,bool inherit,uint flags,IntPtr env,string cwd,ref SIEX si,out PI pi);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandleEx(IntPtr h,int c,IntPtr p,uint n);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,ExactSpelling=true,SetLastError=true)] static extern SafeFileHandle CreateFileW(string p,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
 public static string ReadFixtureFileId(string path,bool directory) {
  const uint readAttributes=0x80, shareReadWriteDelete=7, openExisting=3, backupSemantics=0x02000000, openReparsePoint=0x00200000;
  using(var handle=CreateFileW(path,readAttributes,shareReadWriteDelete,IntPtr.Zero,openExisting,backupSemantics|openReparsePoint,IntPtr.Zero)) {
   if(handle.IsInvalid) throw new Exception("STAGE_A_FILE_ID_OPEN");
   var memory=Marshal.AllocHGlobal(24);
   try {
    var h=handle.DangerousGetHandle();
    if(!GetFileInformationByHandleEx(h,9,memory,8)) throw new Exception("STAGE_A_FILE_ID_ATTRIBUTES");
    var attrs=Marshal.ReadInt32(memory);
    if((attrs&0x400)!=0 || ((attrs&0x10)!=0)!=directory) throw new Exception("STAGE_A_FILE_ID_WRONG_TYPE");
    if(!directory && (!GetFileInformationByHandleEx(h,1,memory,24) || Marshal.ReadInt32(memory,16)!=1)) throw new Exception("STAGE_A_FILE_ID_LINKS");
    if(!GetFileInformationByHandleEx(h,18,memory,24)) throw new Exception("STAGE_A_FILE_ID_QUERY");
    var id=new byte[24]; Marshal.Copy(memory,id,0,id.Length);
    bool hasFileId=false; for(int index=8;index<id.Length;index++) if(id[index]!=0) hasFileId=true;
    if(!hasFileId) throw new Exception("STAGE_A_FILE_ID_EMPTY");
    return Convert.ToBase64String(id);
   } finally { Marshal.FreeHGlobal(memory); }
  }
 }
 [DllImport("shell32.dll",CharSet=CharSet.Unicode)] static extern int SHGetKnownFolderPath(ref Guid id,uint flags,IntPtr token,out IntPtr path);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] static extern int RegOpenKeyExW(IntPtr key,string name,uint options,uint access,out IntPtr result);
 [DllImport("advapi32.dll")] static extern int RegCloseKey(IntPtr key);
 public static void RequireNoProgramDataConfig() {
  Guid id=new Guid("62AB5D82-FDC1-4DC3-A9DD-070D1D495D97"); IntPtr p;
  if(SHGetKnownFolderPath(ref id,0,IntPtr.Zero,out p)!=0) throw new Exception("STAGE_A_PROGRAMDATA_UNKNOWN");
  string known; try { known=Marshal.PtrToStringUni(p); } finally { Marshal.FreeCoTaskMem(p); }
  if(String.IsNullOrEmpty(known) || !Path.IsPathFullyQualified(known)) throw new Exception("STAGE_A_PROGRAMDATA_PATH");
  for(var directory=new DirectoryInfo(known);directory!=null;directory=directory.Parent) {
   directory.Refresh(); if(!directory.Exists || (directory.Attributes&FileAttributes.ReparsePoint)!=0 || (directory.Attributes&FileAttributes.Directory)==0) throw new Exception("STAGE_A_PROGRAMDATA_ANCESTOR");
  }
  string parent=Path.Combine(known,"OpenAI");
  try { var attributes=File.GetAttributes(parent); if((attributes&FileAttributes.Directory)==0 || (attributes&FileAttributes.ReparsePoint)!=0) throw new Exception("STAGE_A_PROGRAMDATA_UNSAFE"); }
  catch(FileNotFoundException) {} catch(DirectoryNotFoundException) {}
  bool absent=false;
  try { File.GetAttributes(Path.Combine(parent,"Codex")); }
  catch(FileNotFoundException) { absent=true; } catch(DirectoryNotFoundException) { absent=true; }
  if(!absent) throw new Exception("STAGE_A_PROGRAMDATA_PRESENT");
 }
 public static void RequireNoRegistryPolicy() {
  IntPtr key; int status=RegOpenKeyExW(new IntPtr(unchecked((int)0x80000002)),@"SOFTWARE\Policies\OpenAI\Codex",8,0x20119,out key);
  if(status==0){RegCloseKey(key);throw new Exception("STAGE_A_POLICY_PRESENT");}
  if(status!=2) throw new Exception("STAGE_A_POLICY_UNKNOWN");
 }
 public static void RequireNoMachineConfig() { RequireNoProgramDataConfig(); RequireNoRegistryPolicy(); }
 IntPtr job,process; StreamReader output; StreamWriter input; FileStream error; Task drain;
 bool settled=false; public long ErrorBytes { get; private set; }
 public static void RequireSingleLink(FileStream stream) {
  IntPtr p=Marshal.AllocHGlobal(24); try {
   if(!GetFileInformationByHandleEx(stream.SafeFileHandle.DangerousGetHandle(),1,p,24) || Marshal.ReadInt32(p,16)!=1) throw new Exception("STAGE_A_LINKS");
  } finally { Marshal.FreeHGlobal(p); }
 }
 static FileStream Stream(IntPtr h,FileAccess a) { return new FileStream(new SafeFileHandle(h,true),a,4096,false); }
 public StageAOwned(string image,string cwd,string environment) : this(image,cwd,environment,false) {}
 public StageAOwned(string image,string cwd,string environment,bool previewOnly) {
  IntPtr ir=IntPtr.Zero,iw=IntPtr.Zero,orr=IntPtr.Zero,ow=IntPtr.Zero,er=IntPtr.Zero,ew=IntPtr.Zero,env=IntPtr.Zero;
  PI pi=new PI(); bool assigned=false; IntPtr attributes=IntPtr.Zero,handles=IntPtr.Zero; bool attributesReady=false;
  try {
   job=CreateJobObject(IntPtr.Zero,null); if(job==IntPtr.Zero) throw new Exception("STAGE_A_JOB");
   // JOBOBJECT_EXTENDED_LIMIT_INFORMATION, x64; LimitFlags at offset 16.
   if(IntPtr.Size!=8) throw new Exception("STAGE_A_ARCH");
   IntPtr limits=Marshal.AllocHGlobal(144); try { for(int k=0;k<144;k++) Marshal.WriteByte(limits,k,0); Marshal.WriteInt32(limits,16,0x2000);
    if(!SetInformationJobObject(job,9,limits,144)) throw new Exception("STAGE_A_JOB_LIMIT");
   } finally { Marshal.FreeHGlobal(limits); }
   SA sa=new SA{length=Marshal.SizeOf<SA>(),inherit=1};
   if(!CreatePipe(out ir,out iw,ref sa,0)||!CreatePipe(out orr,out ow,ref sa,0)||!CreatePipe(out er,out ew,ref sa,0)) throw new Exception("STAGE_A_PIPE");
   if(!SetHandleInformation(iw,1,0)||!SetHandleInformation(orr,1,0)||!SetHandleInformation(er,1,0)) throw new Exception("STAGE_A_PIPE_FLAGS");
   env=Marshal.StringToHGlobalUni(environment);
   IntPtr attrSize=IntPtr.Zero; InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref attrSize);
   if(attrSize==IntPtr.Zero) throw new Exception("STAGE_A_HANDLE_LIST_SIZE");
   attributes=Marshal.AllocHGlobal(attrSize);
   if(!InitializeProcThreadAttributeList(attributes,1,0,ref attrSize)) throw new Exception("STAGE_A_HANDLE_LIST"); attributesReady=true;
   handles=Marshal.AllocHGlobal(3*IntPtr.Size); Marshal.WriteIntPtr(handles,0,ir); Marshal.WriteIntPtr(handles,IntPtr.Size,ow); Marshal.WriteIntPtr(handles,2*IntPtr.Size,ew);
   if(!UpdateProcThreadAttribute(attributes,0,new IntPtr(0x20002),handles,new IntPtr(3*IntPtr.Size),IntPtr.Zero,IntPtr.Zero)) throw new Exception("STAGE_A_HANDLE_WHITELIST");
   SIEX si=new SIEX{start=new SI{cb=Marshal.SizeOf<SIEX>(),flags=0x100,input=ir,output=ow,error=ew},attributes=attributes};
   var args=previewOnly?" --exact tests::native_cli_projection_live::delete_preview_only::PreviewOnlyOwned_098 --ignored --nocapture --test-threads=1":" app-server --strict-config";
   if(!CreateProcessW(image,new StringBuilder("\""+image+"\""+args),IntPtr.Zero,IntPtr.Zero,true,0x08080404,env,cwd,ref si,out pi)) throw new Exception("STAGE_A_SPAWN");
   process=pi.process;
   if(!AssignProcessToJobObject(job,process)) throw new Exception("STAGE_A_JOB_ASSIGN"); assigned=true;
   CloseHandle(ir); ir=IntPtr.Zero; CloseHandle(ow); ow=IntPtr.Zero; CloseHandle(ew); ew=IntPtr.Zero;
   input=new StreamWriter(Stream(iw,FileAccess.Write),new UTF8Encoding(false)){AutoFlush=true}; iw=IntPtr.Zero;
   output=new StreamReader(Stream(orr,FileAccess.Read),new UTF8Encoding(false,true)); orr=IntPtr.Zero;
   error=Stream(er,FileAccess.Read); er=IntPtr.Zero;
   drain=Task.Run(()=>{byte[] buf=new byte[4096]; int n; while((n=error.Read(buf,0,buf.Length))>0) ErrorBytes+=n;});
   if(ResumeThread(pi.thread)==0xffffffff) throw new Exception("STAGE_A_RESUME");
  } catch {
   if(process!=IntPtr.Zero) { if(assigned) TerminateJobObject(job,1); else TerminateProcess(process,1); WaitForSingleObject(process,5000); }
   Dispose(); throw;
  } finally {
   if(attributesReady) DeleteProcThreadAttributeList(attributes); if(attributes!=IntPtr.Zero) Marshal.FreeHGlobal(attributes); if(handles!=IntPtr.Zero) Marshal.FreeHGlobal(handles);
   if(pi.thread!=IntPtr.Zero) CloseHandle(pi.thread); if(env!=IntPtr.Zero) Marshal.FreeHGlobal(env);
   foreach(IntPtr h in new[]{ir,iw,orr,ow,er,ew}) if(h!=IntPtr.Zero) CloseHandle(h);
  }
 }
 public void Send(string text) { if(text.Length>65536) throw new Exception("STAGE_A_INPUT_BOUND"); input.WriteLine(text); }
 public string ReadLine(DateTime deadline) { return ReadLine(deadline,5000); }
 public string ReadLine(DateTime deadline,int stepBudget) {
  var b=new StringBuilder(); var c=new char[1];
  while(b.Length<=65536) { int ms=(int)Math.Min(stepBudget,(deadline-DateTime.UtcNow).TotalMilliseconds); if(ms<=0) throw new Exception("STAGE_A_DEADLINE");
   var pending=output.ReadAsync(c,0,1); if(!pending.Wait(ms)) throw new Exception("STAGE_A_READ_TIMEOUT"); if(pending.Result==0) throw new Exception("STAGE_A_EOF");
   if(c[0]=='\n') return b.ToString(); b.Append(c[0]);
  } throw new Exception("STAGE_A_OUTPUT_BOUND");
 }
 int Active() { IntPtr p=Marshal.AllocHGlobal(48); try {if(!QueryInformationJobObject(job,1,p,48,IntPtr.Zero)) throw new Exception("STAGE_A_JOB_QUERY"); return Marshal.ReadInt32(p,40);} finally{Marshal.FreeHGlobal(p);} }
 public void Finish() {
  input.Dispose(); input=null;
  if(WaitForSingleObject(process,5000)!=0 || !GetExitCodeProcess(process,out uint code) || code!=0) throw new Exception("STAGE_A_EXIT");
  DateTime until=DateTime.UtcNow.AddSeconds(5); while(Active()!=0 && DateTime.UtcNow<until) System.Threading.Thread.Sleep(20);
  if(Active()!=0 || !drain.Wait(1000)) throw new Exception("STAGE_A_OWNER_PENDING"); settled=true;
 }
 public void Dispose() {
  if(job!=IntPtr.Zero && !settled) { TerminateJobObject(job,1); if(process!=IntPtr.Zero) WaitForSingleObject(process,5000); }
  try { input?.Dispose(); } catch {}
  try { output?.Dispose(); } catch {}
  try { error?.Dispose(); } catch {}
  if(process!=IntPtr.Zero){CloseHandle(process);process=IntPtr.Zero;} if(job!=IntPtr.Zero){CloseHandle(job);job=IntPtr.Zero;}
 }
}
'@
    $phase='preflight-machine-programdata'
    [StageAOwned]::RequireNoProgramDataConfig()
    $phase='preflight-machine-registry'
    [StageAOwned]::RequireNoRegistryPolicy()
    $phase='preflight-fresh-temp'
    $root=Join-Path $temp ('pr101-restore-stage-a-'+[Guid]::NewGuid().ToString('N'))
    if (Test-Path -LiteralPath $root) { throw 'STAGE_A_EXISTING_ROOT' }
    $live=Join-Path $root 'live'; $fixtureHome=Join-Path $live 'home'; $codexHome=Join-Path $fixtureHome 'codex'
    $sqlite=Join-Path $live 'sqlite'; $project=Join-Path $live 'project'
    $snapshot=Join-Path $root 'snapshot'; $retained=Join-Path $root 'retained-original'
    foreach($path in @($project,$sqlite,$codexHome,(Join-Path $live 'temporary'),(Join-Path $live 'logs'),(Join-Path $live 'roaming'),(Join-Path $live 'local'))) {
        $null=[IO.Directory]::CreateDirectory($path)
    }
    $id=[Guid]::NewGuid().ToString();$archivedId=[Guid]::NewGuid().ToString();$now=[DateTime]::UtcNow
    $activeDir=Join-Path $codexHome ('sessions/'+$now.ToString('yyyy/MM/dd'));$archiveDir=Join-Path $codexHome 'archived_sessions'
    $null=[IO.Directory]::CreateDirectory($activeDir);$null=[IO.Directory]::CreateDirectory($archiveDir)
    $rollouts=@()
    foreach($target in @(@($id,$activeDir,'StageAActiveCanary'),@($archivedId,$archiveDir,'StageAArchivedCanary'))) {
        $file=Join-Path $target[1] ('rollout-'+$now.ToString('yyyy-MM-ddTHH-mm-ss')+'-'+$target[0]+'.jsonl')
        $stamp=$now.ToString("yyyy-MM-ddTHH:mm:ss.fff'Z'")
        $lines=@(@{timestamp=$stamp;type='session_meta';payload=@{id=$target[0];session_id=$target[0];timestamp=$stamp;cwd=$project;originator='cc_desk_acceptance';cli_version='0.169.1';source='cli'}},
          @{timestamp=$stamp;type='event_msg';payload=@{type='user_message';message=$target[2];images=@();local_images=@();text_elements=@()}})
        [IO.File]::WriteAllText($file,(($lines|ForEach-Object{ConvertTo-Json -InputObject $_ -Depth 8 -Compress}) -join "`n")+"`n",[Text.UTF8Encoding]::new($false))
        $rollouts+= $file
    }
    # JSON quoted Windows strings are valid TOML basic strings.
    $config=@"
sqlite_home = $(ConvertTo-Json -InputObject $sqlite -Compress)
log_dir = $(ConvertTo-Json -InputObject (Join-Path $live 'logs') -Compress)
cli_auth_credentials_store = "file"
mcp_oauth_credentials_store = "file"
check_for_update_on_startup = false
model_provider = "fixture"
model = "fixture-no-model-call"
[features]
plugins = false
remote_plugin = false
plugin_discovery_prefetch = false
skill_mcp_dependency_install = false
hooks = false
[analytics]
enabled = false
[feedback]
enabled = false
[otel]
exporter = "none"
trace_exporter = "none"
metrics_exporter = "none"
[model_providers.fixture]
name = "fixture"
base_url = "http://127.0.0.1:9/v1"
wire_api = "responses"
requires_openai_auth = false
[mcp_servers]
"@
    [IO.File]::WriteAllText((Join-Path $codexHome 'config.toml'),$config,[Text.UTF8Encoding]::new($false))
    $environment=@{}
    foreach($key in @('SystemRoot','WINDIR','SystemDrive','ComSpec')) {
        $value=[Environment]::GetEnvironmentVariable($key);if(!$value){throw 'STAGE_A_SYSTEM_ENV'};$environment[$key]=$value
    }
    foreach($pair in @(@('HOME',$fixtureHome),@('USERPROFILE',$fixtureHome),@('APPDATA',(Join-Path $live 'roaming')),@('LOCALAPPDATA',(Join-Path $live 'local')),
      @('TEMP',(Join-Path $live 'temporary')),@('TMP',(Join-Path $live 'temporary')),@('CODEX_HOME',$codexHome),@('CODEX_SQLITE_HOME',$sqlite),
      @('HOMEDRIVE',$fixtureHome.Substring(0,2)),@('HOMEPATH',$fixtureHome.Substring(2)),@('GIT_CEILING_DIRECTORIES',$live),
      @('GIT_CONFIG_NOSYSTEM','1'),@('GIT_CONFIG_GLOBAL',(Join-Path $live 'empty-gitconfig')))){$environment[$pair[0]]=$pair[1]}
    [IO.File]::WriteAllText((Join-Path $live 'empty-gitconfig'),'')
    $environmentBlock=(($environment.Keys|Sort-Object|ForEach-Object{$_+'='+$environment[$_]}) -join "`0")+"`0`0"
    function Read-Official {
        [StageAOwned]::RequireNoMachineConfig()
        Assert-Ordinary $live
        $owner=[StageAOwned]::new($image,$project,$environmentBlock)
        $deadline=[DateTime]::UtcNow.AddSeconds(30)
        function Send([object]$Frame) {
            if($Frame.method -cnotin @('initialize','initialized','thread/read','thread/list')){throw 'STAGE_A_METHOD_REJECTED'}
            $owner.Send((ConvertTo-Json -InputObject $Frame -Compress -Depth 8))
        }
        function Receive([int]$Expected) {
            for($n=0;$n -lt 48;$n++) {
                $frame=$owner.ReadLine($deadline)|ConvertFrom-Json -AsHashtable
                if($frame.ContainsKey('method') -and !$frame.ContainsKey('id')){continue}
                if(!$frame.ContainsKey('id') -or $frame.id -ne $Expected -or $frame.ContainsKey('error') -or !$frame.ContainsKey('result')){throw 'STAGE_A_REPLY'}
                return $frame.result
            };throw 'STAGE_A_FRAME_BOUND'
        }
        try {
            Send @{id=1;method='initialize';params=@{clientInfo=@{name='cc_desk';version='0.28.0'};capabilities=@{explicitGatewayOauth=$true;experimentalApi=$false}}}
            $initial=Receive 1
            if([IO.Path]::GetFullPath($initial.codexHome).TrimEnd('\') -cne $codexHome){throw 'STAGE_A_HOME_BINDING'}
            Send @{method='initialized'}
            $rpc=2
            foreach($expected in @($id,$archivedId)){
                Send @{id=$rpc;method='thread/read';params=@{threadId=$expected;includeTurns=$false}}
                $reply=Receive $rpc;$rpc++
                if($reply.thread.id -cne $expected -or $reply.thread.cwd -cne $project){throw 'STAGE_A_READ_IDENTITY'}
                $canary=if($expected -ceq $id){'StageAActiveCanary'}else{'StageAArchivedCanary'}
                # Fail closed if this pinned provider does not expose the expected preview.
                if(!$reply.thread.ContainsKey('preview') -or $reply.thread.preview -cne $canary){throw 'STAGE_A_CONTENT_NOT_VERIFIED'}
            }
            foreach($archived in @($false,$true)){
                Send @{id=$rpc;method='thread/list';params=@{archived=$archived;limit=100}}
                $reply=Receive $rpc;$rpc++
                $expected=if($archived){$archivedId}else{$id};$other=if($archived){$id}else{$archivedId}
                if(@($reply.data|Where-Object{$_.id -ceq $expected -and $_.cwd -ceq $project}).Count -ne 1 -or
                   @($reply.data|Where-Object{$_.id -ceq $other}).Count -ne 0){throw 'STAGE_A_LIST_STATE'}
            }
            $owner.Finish()
        } finally { $owner.Dispose() }
    }
    function Inventory([string]$Base) {
        Assert-Ordinary $Base
        $rows=[Collections.Generic.List[object]]::new();$queue=[Collections.Generic.Stack[string]]::new();$queue.Push($Base)
        [long]$total=0
        while($queue.Count){
            foreach($item in Get-ChildItem -LiteralPath $queue.Pop() -Force){
                if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'STAGE_A_REPARSE'}
                $relative=[IO.Path]::GetRelativePath($Base,$item.FullName)
                if($relative.StartsWith('..') -or [IO.Path]::IsPathRooted($relative)){throw 'STAGE_A_ESCAPE'}
                if($item.PSIsContainer){$rows.Add([pscustomobject]@{Path=$relative;Directory=$true;Length=0;Hash=''});$queue.Push($item.FullName)}else{
                    $total+=$item.Length;if($total -gt 128MB -or $rows.Count -gt 4096){throw 'STAGE_A_SNAPSHOT_BOUND'}
                    $stream=[IO.File]::Open($item.FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
                    try{[StageAOwned]::RequireSingleLink($stream);$hash=[Security.Cryptography.SHA256]::Create();try{$digest=[Convert]::ToHexString($hash.ComputeHash($stream))}finally{$hash.Dispose()}}finally{$stream.Dispose()}
                    $rows.Add([pscustomobject]@{Path=$relative;Directory=$false;Length=$item.Length;Hash=$digest})
                }
            }
        }
        return @($rows|Sort-Object Path)
    }
    function Assert-Same($A,$B) {
        if((ConvertTo-Json -InputObject @($A) -Depth 4 -Compress) -cne (ConvertTo-Json -InputObject @($B) -Depth 4 -Compress)){throw 'STAGE_A_COPY_MISMATCH'}
    }
    function Copy-Exact([string]$Source,[string]$Destination,$Manifest){
        if(Test-Path -LiteralPath $Destination){throw 'STAGE_A_EXISTING_COPY'}
        Assert-Ordinary (Split-Path $Destination -Parent)
        $null=[IO.Directory]::CreateDirectory($Destination)
        foreach($entry in $Manifest){$dest=Join-Path $Destination $entry.Path
            if($entry.Directory){$null=[IO.Directory]::CreateDirectory($dest)}else{
                $null=[IO.Directory]::CreateDirectory((Split-Path $dest -Parent))
                $src=Join-Path $Source $entry.Path;Assert-Ordinary $src
                [IO.File]::Copy($src,$dest,$false)
            }
        }
        Assert-Same $Manifest (Inventory $Source);Assert-Same $Manifest (Inventory $Destination)
    }
    if ($PreviewWorker -or $PreviewWorkerSha256) {
        $phase='preview-worker-pin'
        if (!$SameDatasetReadiness -or !$PreviewWorker -or $PreviewWorkerSha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'PREVIEW_WORKER_BINDING' }
        $previewExe=[IO.Path]::GetFullPath($PreviewWorker)
        if ($previewExe -cne (Join-Path $temp 'pr101-preview-worker/cc-desk-preview-test.exe')) {throw 'PREVIEW_WORKER_LOCATION'}
        Assert-Ordinary $previewExe
        $previewLease=[IO.File]::Open($previewExe,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
        [StageAOwned]::RequireSingleLink($previewLease)
        $hasher=[Security.Cryptography.SHA256]::Create()
        try { $actualWorkerHash=[Convert]::ToHexString($hasher.ComputeHash($previewLease)).ToLowerInvariant() } finally { $hasher.Dispose() }
        if ($actualWorkerHash -cne $PreviewWorkerSha256) {throw 'PREVIEW_WORKER_HASH'}
    }
    $phase='baseline-read';Read-Official
    $phase='snapshot';[StageAOwned]::RequireNoMachineConfig();$baseline=@(Inventory $live)
    foreach($rollout in $rollouts){
        $relative=[IO.Path]::GetRelativePath($live,$rollout)
        if(@($baseline|Where-Object{!$_.Directory -and $_.Path -ceq $relative -and $_.Length -gt 0}).Count -ne 1){throw 'STAGE_A_EXPECTED_ROLLOUT_MISSING'}
    }
    $databases=@($baseline|Where-Object{!$_.Directory -and $_.Path.StartsWith('sqlite\',[StringComparison]::OrdinalIgnoreCase) -and $_.Path -match '\.(sqlite|db)$'})
    if(!$databases.Count){throw 'STAGE_A_SQLITE_NOT_OBSERVED'}
    foreach($db in $databases){
        $s=[IO.File]::OpenRead((Join-Path $live $db.Path));try{$header=[byte[]]::new(16);if($s.Read($header,0,16) -ne 16 -or [Text.Encoding]::ASCII.GetString($header) -cne "SQLite format 3`0"){throw 'STAGE_A_SQLITE_HEADER'}}finally{$s.Dispose()}
    }
    Copy-Exact $live $snapshot $baseline
    # Original synthetic paths are restored, so SQLite absolute rollout/cwd pointers
    # cannot accidentally reference a second fixture. Original is retained, not deleted.
    $phase='restore';[StageAOwned]::RequireNoMachineConfig();Assert-Same $baseline (Inventory $live)
    if(Test-Path -LiteralPath $retained){throw 'STAGE_A_RETAINED_EXISTS'}
    [IO.Directory]::Move($live,$retained)
    Copy-Exact $snapshot $live $baseline
    function Observe-RestoredFixtureFileIds {
        # Private checkpoint values only; no path, id or reusable authority leaves this run.
        # Handles are opened for attributes and closed at each checkpoint; no continuous lease is claimed.
        if (!$SameDatasetReadiness -or $rollouts.Count -ne 2 -or $rollouts[0] -ceq $rollouts[1]) {
            throw 'STAGE_A_READINESS_SCOPE'
        }
        $phaseIdSet=@(
            [StageAOwned]::ReadFixtureFileId($live,$true)
            foreach($leaf in $rollouts) {
                if (!$leaf.StartsWith($live+'\',[StringComparison]::OrdinalIgnoreCase)) {throw 'STAGE_A_READINESS_ESCAPE'}
                Assert-Ordinary $leaf
                [StageAOwned]::ReadFixtureFileId($leaf,$false)
            }
        )
        if ($phaseIdSet.Count -ne 3 -or @($phaseIdSet|Sort-Object -Unique -CaseSensitive).Count -ne 3) {
            throw 'STAGE_A_READINESS_FILE_ID_AMBIGUOUS'
        }
        $phaseIdSet
    }
    if ($SameDatasetReadiness) {
        $phase='readiness-before-restored-read'
        Assert-Ordinary $live
        $beforeFileIds=@(Observe-RestoredFixtureFileIds)
    }
    $phase='restored-read';Read-Official
    if ($SameDatasetReadiness) {
        $phase='readiness-after-restored-read'
        Assert-Ordinary $live
        $afterFileIds=@(Observe-RestoredFixtureFileIds)
        for ($i=0;$i -lt 3;$i++) {
            if ($beforeFileIds[$i] -cne $afterFileIds[$i]) { throw 'STAGE_A_READINESS_FILE_ID_CHANGED' }
        }
    }
    Assert-Same $baseline (Inventory $snapshot);Assert-Same $baseline (Inventory $retained)
    # Read-only protocol can update its own SQLite state/logs; only rollout bytes must
    # remain byte-identical after the reader exits. The restored pre-read tree was exact.
    $after=@(Inventory $live)
    foreach($entry in @($baseline|Where-Object{!$_.Directory -and $_.Path.EndsWith('.jsonl')})){
        $same=@($after|Where-Object{$_.Path -ceq $entry.Path})
        if($same.Count -ne 1 -or $same[0].Hash -cne $entry.Hash -or $same[0].Length -ne $entry.Length){throw 'STAGE_A_ROLLOUT_CHANGED'}
    }
    if ($previewLease) {
        $phase='product-preview-only'
        [StageAOwned]::RequireNoMachineConfig()
        $beforePreview=@(Inventory $live)
        $beforePreviewIds=@(Observe-RestoredFixtureFileIds)
        $childEnvironment=@{}+$environment
        # Wry/OS support state is outside the restored dataset. Provider projections
        # still bind the exact Stage-A CODEX_HOME and CODEX_SQLITE_HOME.
        $processState=Join-Path $root 'preview-os'
        foreach($pair in @(@('HOME','home'),@('USERPROFILE','home'),@('APPDATA','roaming'),@('LOCALAPPDATA','local'),@('TEMP','temp'),@('TMP','temp'))){
            $childEnvironment[$pair[0]]=Join-Path $processState $pair[1]
            $null=[IO.Directory]::CreateDirectory($childEnvironment[$pair[0]])
        }
        $childEnvironment['HOMEDRIVE']=$childEnvironment['HOME'].Substring(0,2)
        $childEnvironment['HOMEPATH']=$childEnvironment['HOME'].Substring(2)
        $childEnvironment['GITHUB_ACTIONS']='true';$childEnvironment['RUNNER_ENVIRONMENT']='github-hosted'
        $childEnvironment['RUNNER_TEMP']=$temp;$childEnvironment['CC_DESK_PREVIEW_STAGE']='codex-preview-only'
        $childBlock=(($childEnvironment.Keys|Sort-Object|ForEach-Object{$_+'='+$childEnvironment[$_]}) -join "`0")+"`0`0"
        $probe=[StageAOwned]::new($previewExe,$project,$childBlock,$true)
        try {
            $probe.Send(([ordered]@{live=$live;codex=$image;active=$id;archived=$archivedId}|ConvertTo-Json -Compress))
            $deadline=[DateTime]::UtcNow.AddSeconds(45)
            $seen=$false
            for($i=0;$i -lt 48;$i++){
                $row=$probe.ReadLine($deadline,20000)
                if ($row -cmatch 'STAGE_B_WRY_PREVIEW_ONLY_PASS$') { $seen=$true; break }
                if ($row -cmatch 'STAGE_B_WRY_PREVIEW_BLOCKED (scope|before-list|active-preview|archived-preview|after-list|unexpected)$') {
                    $phase='product-'+$Matches[1]
                    throw 'PREVIEW_WORKER_REJECTED'
                }
            }
            if (!$seen) {throw 'PREVIEW_MARKER_MISSING'}
            $probe.Finish()
        } finally { $probe.Dispose() }
        $phase='product-preview-integrity'
        Assert-Same $beforePreview (Inventory $live)
        Assert-Same $baseline (Inventory $snapshot);Assert-Same $baseline (Inventory $retained)
        $afterPreviewIds=@(Observe-RestoredFixtureFileIds)
        for($i=0;$i -lt 3;$i++){if($beforePreviewIds[$i] -cne $afterPreviewIds[$i]){throw 'PREVIEW_FILE_ID_CHANGED'}}
        [ordered]@{schema=1;stage='synthetic-product-preview-only';providerVersion='0.169.1';sameJobRestoredFixture=$true;
            activeControlBlockedWithoutTicket=$true;archivedTargetPreviewReady=$true;bothRowsUnchanged=$true;
            restoredBytesAndCheckpointIdsUnchanged=$true;ownerSettled=$true;continuousFileLeaseVerified=$false;
            confirmHandlerRegistered=$false;confirmInvoked=$false;deletionExecuted=$false;stageBGoGranted=$false;
            standardPackageUiVerified=$false}|ConvertTo-Json -Compress
        return
    }

    # Extra evidence is available only if the caller explicitly selects this verifier mode.
    # The original Stage-A default success schema is unchanged.
    if ($SameDatasetReadiness) {
        [ordered]@{schema=1;stage='synthetic-dataset-readiness-dry-run';providerVersion='0.169.1';
            sameProcessCustody=$true;sameDatasetActiveArchivedRead=$true;recoveryManifestEquivalentBeforeRead=$true;
            restoredRootAndRolloutFileIdsStable=$true;preservedCopiesMatch=$true;ownerSettled=$true;
            continuousFileLeaseVerified=$false;sqliteSemanticRowsVerified=$false;readOnlyMayReindex=$true;
            deletionExecuted=$false;stageBGoGranted=$false;productDeletePreflightVerified=$false;standardPackageUiVerified=$false} | ConvertTo-Json -Compress
        return
    }
    # No raw paths, UUIDs, contents, stdout/stderr or credentials in the receipt.
    [ordered]@{schema=1;stage='synthetic-restore-readonly';providerVersion='0.169.1';baselineRead=$true;restoredRead=$true;
        sqliteSemanticRowsVerified=$false;readOnlyMayReindex=$true;byteManifestEquivalentBeforeRead=$true;originalRetained=$true;snapshotRetained=$true;sqliteObserved=$true;sqliteFileCount=$databases.Count;
        fileCount=@($baseline|Where-Object{!$_.Directory}).Count;bytes=($baseline|Measure-Object Length -Sum).Sum;
        rolloutBytesRetained=$true;providerCanaryPreviewVerified=$true;activeAndArchivedIdentityVerified=$true;ownerSettled=$true;
        deletionExecuted=$false;stageBGoGranted=$false;standardPackageUiVerified=$false}|ConvertTo-Json -Compress
} catch {
    # Do not echo exception text: filesystem/process errors can contain raw paths/data.
    [ordered]@{schema=1;stage='synthetic-restore-readonly';state='blocked';phase=$phase;deletionExecuted=$false;stageBGoGranted=$false}|ConvertTo-Json -Compress
    throw 'STAGE_A_FAILED_CLOSED'
} finally {if($previewLease){$previewLease.Dispose()};if($imageLease){$imageLease.Dispose()}}
