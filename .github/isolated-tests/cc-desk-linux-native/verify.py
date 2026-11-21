"""One bounded real Codex metadata rename using a synthetic CC Desk fixture."""
import base64
import contextlib
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import selectors
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import uuid

SOURCE = '04a2f0cedc8e7acc080e8736723c6e8799127f58'
SRI = 'KmlHQ/lB9jcn8Yy5z/tWCpSPsqBvI3pU+0PUPccr+pKiiU2NJSCvZcAD7s3dmIFq5ZvGYtGcSn2KdFFdaPQICg=='
EXE_SHA = '105102da93472e8641b839554c7713849817943a09f76db3f4ea333fd67ce8b5'
EXE_MEMBER = 'package/vendor/x86_64-unknown-linux-musl/bin/codex'
NEW = 'CC Desk synthetic metadata title'
METHODS = {'initialize', 'initialized', 'thread/read', 'thread/list', 'thread/name/set'}
CONFIG = r'''# Applied only inside the project-owned temporary CODEX_HOME.
check_for_update_on_startup = false
cli_auth_credentials_store = "file"
model_provider = "ccdesk_fixture_local_only"
chatgpt_base_url = "http://127.0.0.1:1/"
openai_base_url = "http://127.0.0.1:1/v1"
web_search = "disabled"

[analytics]
enabled = false

[feedback]
enabled = false

[otel]
exporter = "none"
trace_exporter = "none"
metrics_exporter = "none"
log_user_prompt = false

[features]
apps = false
plugins = false
remote_control = false
remote_plugin = false
api_key_model_discovery = false
system_proxy_fallback = false

[model_providers.ccdesk_fixture_local_only]
name = "CC Desk isolated read-only history fixture"
base_url = "http://127.0.0.1:1/v1"
wire_api = "responses"
requires_openai_auth = false
'''
CONFIG_SHA = '77716d232bafa5ba15aaec3838433b05c540e96b1fa53d81934eab5ab7ce9f94'


def need(condition):
    if not condition:
        raise ValueError('fixed gate not met')


def admitted():
    need(sys.platform.startswith('linux') and os.uname().machine == 'x86_64')
    need(os.environ.get('GITHUB_ACTIONS') == 'true' and
         os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted')
    for p in ('config.toml', 'requirements.toml', 'managed_config.toml', 'hooks.json'):
        candidate = Path('/etc/codex') / p
        # lstat distinguishes missing from access errors and rejects dangling links.
        try:
            candidate.lstat()
        except FileNotFoundError:
            continue
        raise ValueError('system config present')
    need(not any('proxy' in name.lower() for name in os.environ))
    for table in ('/proc/net/tcp', '/proc/net/tcp6'):
        for row in Path(table).read_text().splitlines()[1:]:
            _, local, _, state, *_ = row.split()
            need(not (state == '0A' and local.rpartition(':')[2] == '0001'))


def exact_binary(archive, target):
    fd = os.open(archive, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as f:
        info = os.fstat(f.fileno())
        need(stat.S_ISREG(info.st_mode) and info.st_size == 167001033)
        need(base64.b64encode(hashlib.file_digest(f, 'sha512').digest()).decode() == SRI)
        f.seek(0)
        with tarfile.open(fileobj=f, mode='r:gz') as tar:
            entries = {}
            n = 0
            for e in tar:
                p = PurePosixPath(e.name)
                need(len(entries)<45 and e.isfile() and e.name not in entries and
                     p.parts[0]=='package' and len(p.parts)>1 and '..' not in p.parts and
                     '\\' not in e.name and 0<=e.size<=294427960)
                n += e.size
                need(n<=422796466)
                entries[e.name] = e
            need(len(entries)==45 and n==422796466)
            need([p for p in entries if p.endswith('/bin/codex')]==[EXE_MEMBER])
            e=entries[EXE_MEMBER]
            need(e.size==294427960)
            with tar.extractfile(e) as inp, target.open('xb') as out:
                shutil.copyfileobj(inp,out,1 << 20)
    with target.open('rb') as pinned:
        need(pinned.read(5)==b'\x7fELF\x02')
        pinned.seek(0)
        need(hashlib.file_digest(pinned,'sha256').hexdigest()==EXE_SHA)
    need(target.stat().st_size==294427960)
    target.chmod(0o500)


class AppServer:
    def __init__(self, process):
        self.p, self.i, self.buffer = process, 0, b''
        self.reader=selectors.DefaultSelector()
        self.reader.register(process.stdout,selectors.EVENT_READ)

    def request(self, method, params=None, notification=False):
        need(method in METHODS)
        self.i += 1
        msg={'method':method}
        if params is not None:
            msg['params']=params
        if not notification:
            msg['id']=self.i
        self.p.stdin.write(json.dumps(msg,separators=(',',':')).encode()+b'\n')
        self.p.stdin.flush()
        if notification:
            return
        deadline=time.monotonic()+12
        for _ in range(24):
            while b'\n' not in self.buffer:
                left=deadline-time.monotonic()
                need(left>0 and bool(self.reader.select(left)))
                block=os.read(self.p.stdout.fileno(),65536)
                need(block and len(self.buffer)+len(block)<=262144)
                self.buffer+=block
            frame,self.buffer=self.buffer.split(b'\n',1)
            j=json.loads(frame)
            need(type(j) is dict)
            if 'method' in j:
                need('id' not in j)
                continue
            need(type(j.get('id')) is int and j['id']==self.i and 'error' not in j and type(j.get('result')) is dict)
            return j['result']
        need(False)


@contextlib.contextmanager
def launch(exe, project, env):
    p=subprocess.Popen([str(exe),'app-server','--strict-config'],cwd=project,env=env,
                       stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL, close_fds=True, start_new_session=True)
    rpc=None
    try:
        rpc=AppServer(p)
        yield rpc
        p.stdin.close()
        need(p.wait(timeout=12)==0)
    finally:
        if p.poll() is None:
            p.kill()
            p.wait(timeout=5)
        if rpc:
            rpc.reader.close()
        p.stdin.close()
        p.stdout.close()


def same_identity(thread, identity, project):
    need(thread['id']==identity and thread['cwd']==str(project))


def setup(root):
    home=root/'home'
    codex=root/'codex-home'
    project=root/'project'
    env={'PATH':'/usr/bin:/bin','HOME':str(home),'CODEX_HOME':str(codex),
         'XDG_CONFIG_HOME':str(home/'config'), 'XDG_CACHE_HOME':str(home/'cache'),
         'XDG_DATA_HOME':str(home/'data'),'XDG_STATE_HOME':str(home/'state'),
         'TMPDIR':str(root/'tmp'),'LANG':'C.UTF-8'}
    for destination in (home,codex,project,root/'tmp'):
        destination.mkdir(mode=0o700)
    file=codex/'config.toml'
    cfg=CONFIG.encode('utf-8')
    need(hashlib.sha256(cfg).hexdigest()==CONFIG_SHA)
    file.write_bytes(cfg)
    stamp=datetime.now(timezone.utc)
    now=stamp.isoformat(timespec='milliseconds').replace('+00:00','Z')
    identity=str(uuid.uuid4())
    folder=codex/'sessions'/stamp.strftime('%Y/%m/%d')
    folder.mkdir(parents=True)
    rollout=folder/f"rollout-{now[:19].replace(':','-')}-{identity}.jsonl"
    # Adapted from exact-source scripts/native-cli/metadata-lifecycle.mjs:
    # same two record kinds, fields and synthetic-only event.
    records=[{'timestamp':now,'type':'session_meta','payload':
              {'session_id':identity,'id':identity,'timestamp':now,'cwd':str(project),
               'originator':'cc_desk_acceptance','cli_version':'0.169.1','source':'cli'}},
             {'timestamp':now,'type':'event_msg','payload':
              {'type':'user_message','message':'CC Desk synthetic fixture. No model call.',
               'images':[],'local_images':[],'text_elements':[]}}]
    with rollout.open('x',encoding='utf-8') as out:
        out.write(''.join(json.dumps(row)+'\n' for row in records))
    return env, codex, project, identity


def check_read(env, codex):
    need(not os.path.lexists(codex/'auth.json'))
    need(hashlib.sha256((codex/'config.toml').read_bytes()).hexdigest()==CONFIG_SHA)
    need(not any(re.search(r'proxy|token|credential|api_key|openai|chatgpt|auth',k,re.I) for k in env))


def native(exe, root, receipt):
    env,codex,project,identity=setup(root)
    check_read(env,codex)
    v=subprocess.run([str(exe),'--version'],env=env,cwd=project,capture_output=True,timeout=8,check=True)
    need(v.stdout.strip()==b'codex-cli 0.169.1')
    receipt['versionPinned']=True
    for attempt in (1,2):
        check_read(env,codex)
        with launch(exe,project,env) as rpc:
            init=rpc.request('initialize',{'clientInfo':{'name':'cc_desk','version':'0.0.0'},
                    'capabilities':{'explicitGatewayOauth':True,'experimentalApi':False}})
            need(Path(init['codexHome']).resolve()==codex)
            rpc.request('initialized',notification=True)
            params={'threadId':identity,'includeTurns':False}
            thread=rpc.request('thread/read',params)['thread']
            same_identity(thread,identity,project)
            if attempt==1:
                need(thread.get('name')!=NEW)
                receipt['originalIdentityRead']=True
                receipt['nameSetInvoked']=True  # Mark before wire write; never resend.
                rpc.request('thread/name/set',{'threadId':identity,'name':NEW})
                thread=rpc.request('thread/read',params)['thread']
            same_identity(thread,identity,project)
            need(thread.get('name')==NEW)
            listing=rpc.request('thread/list',{'limit':100,'archived':False})
            need(listing.get('nextCursor') is None)
            rows=[row for row in listing['data'] if row.get('id')==identity]
            need(len(rows)==1 and rows[0].get('name')==NEW)
            same_identity(rows[0],identity,project)
            receipt['sameProcessReadback' if attempt==1 else 'reopenedSameIdentityTitle']=True
        receipt['firstProcessExited' if attempt==1 else 'secondProcessExited']=True
    check_read(env,codex)


def main():
    receipt=dict.fromkeys(('cleanHostedAdmission','artifactPinned','versionPinned','originalIdentityRead',
              'nameSetInvoked','sameProcessReadback','firstProcessExited','reopenedSameIdentityTitle',
              'secondProcessExited','passed'),False)
    try:
        admitted()
        receipt['cleanHostedAdmission']=True
        working=Path(os.environ['RUNNER_TEMP']).resolve(strict=True)
        with tempfile.TemporaryDirectory(prefix='codex-native-rename-',dir=working) as place:
            root=Path(place)
            exe=root/'codex'
            exact_binary(Path(sys.argv[1]),exe)
            receipt['artifactPinned']=True
            native(exe,root,receipt)
        receipt['passed']=True
    except Exception:
        pass  # No traceback, actual paths, raw replies, args, UUID, or title in logs.
    print(json.dumps(receipt,sort_keys=True,separators=(',',':')))
    return 0 if receipt['passed'] else 1


if __name__=='__main__':
    raise SystemExit(main())
