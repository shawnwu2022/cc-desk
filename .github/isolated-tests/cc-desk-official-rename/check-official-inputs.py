"""Read-only check for the admitted #291 artifact pair and pre-fetched official Codex.
No networking, extraction, shell subprocesses, installation or app launches.
"""
from __future__ import annotations
import argparse
import hashlib
import base64
import json
import io
from contextlib import nullcontext
from pathlib import Path, PurePosixPath
import stat
import tarfile
from zipfile import ZipFile
from verify_named_nsis import verify

SOURCE = '04a2f0cedc8e7acc080e8736723c6e8799127f58'
RUN = 44842769369
PACKAGE_ID = 14682820278
SMOKE_ID = 14682071875
PACKAGE_ZIP_SHA = '7fc84ac5af07acaa614ca0ab627ae49796658842abdbfca09795fae584b5b3d0'
SMOKE_ZIP_SHA = '7e698787008dcb77354f5bc2384f6ccf0d30d45bb79011355ca687615b83ba4a'
MANIFEST_SHA = '83aad7a6c7671f1b885aa60ac9eb3f62034cbd7fefd53ae285171d6b476c172d'
INSTALLER = 'CC-Desk-0.28.0-rc.1-test-04a2f0cedc8e-x64-setup.exe'
INSTALLER_SHA = '0fe089112ed137bada3130454c810326f0f5ad6a10c8ff4f9a52161c32ffeaa6'
APP_SHA = 'cd55c6504093a7aa884cc8936bc51b58a133a512d8d496ea0530d64e9999b6f5'
CODEX_SHA = 'f76f94d08b0793bfa129ae62c09341ee32ae96d57e69968ae6b8d7c71a20d8c4'
CODEX_TGZ_B64 = 'fImXWqxwxqHh9+FhJWsRRFiBzgBrSeN1bYTvtGZtUKGTPzHefYAL1agmlf5XScfwY5i5pHsExfdhFekzRBbqIw=='
CODEX_MEMBER = 'package/vendor/x86_64-pc-windows-msvc/bin/codex.exe'


def need(ok, code):
    if not ok:
        raise ValueError(code)


def plain_file(p: Path, limit: int):
    s = p.lstat()
    need(stat.S_ISREG(s.st_mode) and not getattr(s,'st_file_attributes',0)&0x400 and 0<s.st_size<=limit, 'INPUT_FILE')


def zipped(p, digest):
    plain_file(p, 64 * 1024 * 1024)
    data=p.read_bytes()
    need(hashlib.sha256(data).hexdigest() == digest, 'ZIP_SHA')
    z = ZipFile(io.BytesIO(data))
    ii = z.infolist()
    need(len(ii) <= 24 and len({x.filename for x in ii}) == len(ii)
         and sum(x.file_size for x in ii) < 64 * 1024 * 1024, 'ZIP_BOUNDS')
    need(all(not PurePosixPath(x.filename).is_absolute() and '..' not in PurePosixPath(x.filename).parts
             and '\\' not in x.filename and ':' not in x.filename and not stat.S_ISLNK(x.external_attr >> 16)
             for x in ii), 'ZIP_NAMES')
    need(z.testzip() is None, 'ZIP_CRC')
    return z


def pair(package: Path, smoke: Path):
    with zipped(package,PACKAGE_ZIP_SHA) as p, zipped(smoke,SMOKE_ZIP_SHA) as s:
        mbytes=p.read('test-package/test-package-manifest.json')
        need(hashlib.sha256(mbytes).hexdigest()==MANIFEST_SHA,'MANIFEST_SHA')
        m=json.loads(mbytes.decode('utf-8-sig'))
        expected={'schemaVersion':1,'product':'CC Desk','channel':'test-only','baseVersion':'0.28.0-rc.1',
            'sourceCommit':SOURCE,'workflowRunId':str(RUN),'workflowRunAttempt':'1','platform':'windows-x86_64',
            'installerFile':INSTALLER,'sha256':INSTALLER_SHA,'publishable':False,'updaterPublication':False,
            'd20RealCliCertification':'BLOCKED_EXTERNAL_TARGET'}
        need(m==expected,'MANIFEST')
        got=verify(p.read('test-package/'+INSTALLER),INSTALLER_SHA)
        smoked=json.loads(s.read('result.json').decode('utf-8-sig'))
        need(got['member']=='cc-desk.exe' and got['memberBytes']==56135680 and got['memberSha256']==APP_SHA
             and smoked['sha256'].lower()==APP_SHA and smoked['status']=='passed'
             and smoked['normalExit'] is True,'INSTALLED_MEMBER')
    return {'officialRun':RUN,'attempt':1,'packageId':PACKAGE_ID,'smokeId':SMOKE_ID,'source':SOURCE,
        'testOnly':True,'publishable':False,'installerSha256':INSTALLER_SHA,'appSha256':APP_SHA,
        'appBytes':56135680,'officialPairVerified':True}


def codex(p: Path, output: Path | None = None):
    plain_file(p, 450 * 1024 * 1024)
    digest=hashlib.sha512()
    with p.open('rb') as f:
        while b:=f.read(1<<20): digest.update(b)
        need(base64.b64encode(digest.digest()).decode()==CODEX_TGZ_B64,'CODEX_VENDOR_PIN')
        f.seek(0)
        with tarfile.open(fileobj=f,mode='r:gz') as z:
            names=set()
            total=0
            member=None
            for entry in z:
                fragments=PurePosixPath(entry.name).parts
                need(len(names)<47 and entry.name not in names and len(fragments)>1 and fragments[0]=='package'
                     and '..' not in fragments and '\\' not in entry.name and ':' not in entry.name
                     and entry.isfile() and 0<=entry.size<=382647088,'CODEX_VENDOR_LAYOUT')
                names.add(entry.name)
                total+=entry.size
                if entry.name==CODEX_MEMBER: member=entry
            need(len(names)==47 and total==515292799 and member is not None and member.size==382647088,'CODEX_VENDOR_COUNT')
            exe=hashlib.sha256()
            size=0
            with z.extractfile(member) as contents, (output.open('xb') if output else nullcontext()) as dest:
                while b:=contents.read(1<<20):
                    size+=len(b)
                    need(size<=382647088,'CODEX_EXE_SIZE')
                    exe.update(b)
                    if dest: dest.write(b)
            need(size==382647088 and exe.hexdigest()==CODEX_SHA,'CODEX_EXE_PIN')
    return {'codexVersion':'0.169.1','codexSha256':CODEX_SHA,'codexVendorVerified':True}


if __name__=='__main__':
    p=argparse.ArgumentParser()
    p.add_argument('package',type=Path)
    p.add_argument('smoke',type=Path)
    p.add_argument('--codex-tgz',type=Path)
    a=p.parse_args()
    try:
        d=pair(a.package,a.smoke)
        d.update(codex(a.codex_tgz) if a.codex_tgz else {'codexVendorVerified':False})
        # This is only file preflight; no offline-executor admission or UI outcome claim.
        d['runtimeAdmission']='BLOCKED_EXTERNAL_TARGET'
        print(json.dumps(d,sort_keys=True))
    except (OSError,ValueError,AssertionError,tarfile.TarError,KeyError):
        print(json.dumps({'inputProof':'rejected'}))
        raise SystemExit(1)
