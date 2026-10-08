#!/usr/bin/env python3
"""One-time 0.18.1 signed candidates. No tags, releases, updater or private keys."""
import base64
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import plistlib
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile

REPO = 'shawnwu2022/cc-desk'
SHA = '5ed35db9a560e093a91a5ef32a1eb6dd171f27fd'
TREE = 'b628bf01a016e258011de0b365255a16f26ee683'
REF = 'release/0.18.1-source-5ed35db'
WORKFLOW = '.github/workflows/release.yml'
PUBLIC_KEY = 'dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDQ5OTA0ODkwNzQwNjVCMDYKUldRR1d3WjBrRWlRU2IrQVIxbnF2c0h2Z0tEUmd4aUl4UkhzZ2R0Lzdwc1FMVERMY3NqNzIrc3gK'
API = f'https://api.github.com/repos/{REPO}'
STATE = Path('signed-build-state')
READY = Path('verified-signed-candidates')
MAX_ARCHIVE = 1024 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def save(name, value):
    STATE.mkdir(exist_ok=True)
    (STATE / name).write_text(json.dumps(value, indent=2) + '\n')


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(method, path, body=None):
    require(path.startswith('/') or path == '', 'Only fixed repository API paths are supported')
    require(method == 'GET' or (method == 'POST' and path == '/actions/workflows/release.yml/dispatches'),
            'Unsupported API mutation')
    token = os.environ.get('GH_TOKEN', '')
    require(token, 'GH_TOKEN is required')
    req = urllib.request.Request(API + path, method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json',
                 'X-GitHub-Api-Version': '2026-03-10', 'Content-Type': 'application/json'})
    return urllib.request.build_opener(NoRedirect()).open(req, timeout=60)


def api(method, path, body=None):
    with request(method, path, body) as response:
        raw = response.read(2 * 1024 * 1024 + 1)
        require(len(raw) <= 2 * 1024 * 1024, 'API response exceeds limit')
        return response.status, json.loads(raw) if raw else None


def get(path):
    status, data = api('GET', path)
    require(status == 200 and isinstance(data, dict), 'Expected a complete GitHub API response')
    return data


def source_identity():
    require(get('')['full_name'] == REPO, 'Repository identity mismatch')
    ref = get('/git/ref/heads/' + REF)
    require(ref.get('ref') == 'refs/heads/' + REF and ref.get('object', {}).get('type') == 'commit'
            and ref.get('object', {}).get('sha') == SHA, 'Source branch drift')
    commit = get('/git/commits/' + SHA)
    require(commit.get('sha') == SHA and commit.get('tree', {}).get('sha') == TREE, 'Source commit/tree drift')
    hashes = {}
    for filename in ['package.json', 'src-tauri/tauri.conf.json']:
        blob = get('/contents/' + filename + '?ref=' + SHA)
        require(blob.get('encoding') == 'base64', 'Source config encoding mismatch')
        raw = base64.b64decode(''.join(blob['content'].splitlines()), validate=True)
        config = json.loads(raw)
        require(config.get('version') == '0.18.1', 'Source version mismatch')
        if filename.endswith('tauri.conf.json'):
            require(config.get('plugins', {}).get('updater', {}).get('pubkey') == PUBLIC_KEY,
                    'Source updater public key mismatch')
        hashes[filename] = hashlib.sha256(raw).hexdigest()
    return {'repository': REPO, 'commit': SHA, 'tree': TREE, 'ref': REF, 'version': '0.18.1',
            'public_key': PUBLIC_KEY, 'source_config_sha256': hashes}


def emit_run(run_id):
    url = f'https://github.com/{REPO}/actions/runs/{run_id}'
    if os.environ.get('GITHUB_OUTPUT'):
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
            output.write(f'signer_run_id={run_id}\nsigner_run_url={url}\n')
    print(f'Signer run: {url}', flush=True)


def dispatch():
    require(os.environ.get('GITHUB_RUN_ATTEMPT') == '1', 'Controller reruns cannot dispatch again')
    require(os.environ.get('GITHUB_REPOSITORY') == REPO, 'Controller repository mismatch')
    require(not (STATE / 'dispatch-intent.json').exists(), 'Dispatch intent already exists; do not retry')
    identity = source_identity()
    prior = get('/actions/workflows/release.yml/runs?branch=' + urllib.parse.quote(REF, safe='')
                + '&event=workflow_dispatch&per_page=100')
    require(prior.get('total_count') == 0 and prior.get('workflow_runs') == [],
            'Source branch already has signer runs, or run history is incomplete; do not dispatch again')
    save('source-identity.json', identity)
    # Exclusive persistent intent precedes the only POST. Unknown acknowledgements never replay.
    with (STATE / 'dispatch-intent.json').open('x') as intent:
        json.dump({'ref': REF, 'commit': SHA, 'workflow': WORKFLOW}, intent)
    try:
        status, response = api('POST', '/actions/workflows/release.yml/dispatches', {'ref': REF})
        save('dispatch-response.json', response)
        require(status == 200 and isinstance(response, dict), 'Ambiguous dispatch response; do not retry')
        run_id = response.get('workflow_run_id')
        require(type(run_id) is int and run_id > 0, 'Ambiguous dispatch run ID; do not retry')
        require(response.get('run_url') == API + f'/actions/runs/{run_id}' and
                response.get('html_url') == f'https://github.com/{REPO}/actions/runs/{run_id}',
                'Dispatch response URL mismatch; do not retry')
    except Exception as error:
        raise RuntimeError('Dispatch outcome may be ambiguous. Intent preserved; do not dispatch again.') from error
    emit_run(run_id)


def run_id_from_state():
    data = json.loads((STATE / 'dispatch-response.json').read_text())
    run_id = data.get('workflow_run_id')
    require(type(run_id) is int and run_id > 0, 'Missing exact signer run ID')
    return run_id


def validate_run(run, run_id):
    expected = {'id': run_id, 'path': WORKFLOW, 'event': 'workflow_dispatch', 'head_sha': SHA,
                'head_branch': REF, 'run_attempt': 1,
                'html_url': f'https://github.com/{REPO}/actions/runs/{run_id}'}
    require(all(run.get(k) == v for k, v in expected.items()), 'Signer run identity mismatch')
    require(all(run.get(k, {}).get('full_name') == REPO for k in ['repository', 'head_repository']),
            'Signer repository mismatch')


def validate_artifacts(data, run_id):
    artifacts = data.get('artifacts', [])
    names = {f'cc-desk-candidate-{SHA}-{p}' for p in ['windows', 'macos', 'linux']}
    require(data.get('total_count') == 3 and len(artifacts) == 3 and
            {a.get('name') for a in artifacts} == names, 'Expected exactly three candidate artifacts')
    require(len({a.get('id') for a in artifacts}) == 3, 'Duplicate candidate artifact IDs')
    for artifact in artifacts:
        origin = artifact.get('workflow_run', {})
        require(type(artifact.get('id')) is int and artifact['id'] > 0 and artifact.get('expired') is False
                and type(artifact.get('size_in_bytes')) is int and 0 < artifact['size_in_bytes'] <= MAX_ARCHIVE
                and re.fullmatch(r'sha256:[0-9a-f]{64}', artifact.get('digest') or '')
                and origin.get('id') == run_id and origin.get('head_sha') == SHA and origin.get('head_branch') == REF,
                'Candidate artifact provenance, expiry or digest mismatch')
    return artifacts


def wait():
    run_id = run_id_from_state()
    deadline = time.monotonic() + 75 * 60
    while time.monotonic() < deadline:
        try:
            run = get(f'/actions/runs/{run_id}')
            validate_run(run, run_id)
            save('signer-run.json', run)
            if run.get('status') == 'completed':
                require(run.get('conclusion') == 'success', 'Signer build failed: ' + str(run.get('conclusion')))
                artifacts = validate_artifacts(get(f'/actions/runs/{run_id}/artifacts?per_page=100'), run_id)
                save('candidate-manifest.json', {'run_id': run_id, 'artifacts': artifacts})
                emit_run(run_id)
                return
            require(run.get('status') in ['queued', 'in_progress', 'waiting', 'requested', 'pending'],
                    'Unknown signer status')
        except (urllib.error.URLError, TimeoutError) as error:
            if isinstance(error, urllib.error.HTTPError):
                require(error.code in [404, 408, 429] or error.code >= 500, 'Signer API access failed')
            print('Signer API temporarily unavailable; retrying read only', flush=True)
        time.sleep(20)
    raise RuntimeError('Signer wait exceeded 75 minutes; run ID preserved, no redispatch')


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def download_archive(artifact, destination):
    # GitHub returns a short-lived storage URL. Never forward GH_TOKEN to storage.
    try:
        response = request('GET', f"/actions/artifacts/{artifact['id']}/zip")
    except urllib.error.HTTPError as error:
        require(error.code == 302, 'Artifact download did not return the expected redirect')
        location = error.headers.get('Location', '')
        parsed = urllib.parse.urlsplit(location)
        require(parsed.scheme == 'https' and not parsed.username and not parsed.password and parsed.port is None
                and parsed.hostname and (parsed.hostname.endswith('.blob.core.windows.net')
                                         or parsed.hostname.endswith('.actions.githubusercontent.com')),
                'Artifact storage redirect is not allowed')
        response = urllib.request.build_opener(NoRedirect()).open(urllib.request.Request(location), timeout=60)
    with response, destination.open('xb') as target:
        require(response.status == 200, 'Artifact download failed')
        total = 0
        while chunk := response.read(1024 * 1024):
            total += len(chunk)
            require(total <= MAX_ARCHIVE, 'Artifact archive exceeds limit')
            target.write(chunk)
    require('sha256:' + sha256(destination) == artifact['digest'], 'Artifact archive digest mismatch')


def safe_member(name):
    path = PurePosixPath(name)
    return bool(name) and not path.is_absolute() and '\\' not in name and ':' not in name and all(
        part not in ['', '.', '..'] for part in name.rstrip('/').split('/'))


def extract_verified(archive, digest, destination):
    require('sha256:' + sha256(archive) == digest, 'Artifact archive digest mismatch')
    require(not destination.exists(), 'Artifact extraction destination already exists')
    with zipfile.ZipFile(archive) as zipped:
        entries = zipped.infolist()
        require(0 < len(entries) <= 12 and len({x.filename for x in entries}) == len(entries)
                and sum(x.file_size for x in entries) <= MAX_ARCHIVE, 'Unexpected artifact ZIP contents')
        for entry in entries:
            mode = stat.S_IFMT(entry.external_attr >> 16)
            require(safe_member(entry.filename) and mode in [0, stat.S_IFREG, stat.S_IFDIR]
                    and not entry.flag_bits & 1, 'Unsafe artifact ZIP member')
        destination.mkdir(parents=True)
        for entry in entries:
            target = destination / entry.filename
            if entry.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with zipped.open(entry) as source, target.open('xb') as output:
                    shutil.copyfileobj(source, output)


def inspect_platform(folder, platform):
    all_paths = list(folder.rglob('*'))
    require(not folder.is_symlink() and all(not p.is_symlink() and (p.is_file() or p.is_dir()) for p in all_paths),
            'Unexpected candidate symlink or special file')
    files = sorted(p for p in all_paths if p.is_file())
    prefix = r'CC[ .]Desk'
    patterns = {'windows': [prefix + r'_0\.18\.1_x64-setup\.exe', prefix + r'_0\.18\.1_x64-setup\.exe\.sig'],
                'linux': [prefix + r'_0\.18\.1_amd64\.AppImage', prefix + r'_0\.18\.1_amd64\.AppImage\.sig'],
                'macos': [prefix + r'\.app\.tar\.gz', prefix + r'\.app\.tar\.gz\.sig',
                          prefix + r'_0\.18\.1_aarch64\.dmg']}[platform]
    require(len(files) == len(patterns) and all(sum(bool(re.fullmatch(pattern, p.name)) for p in files) == 1
            for pattern in patterns), f'Missing signature pair, wrong version or unexpected {platform} file')
    allowed_dirs = {'macos', 'dmg'} if platform == 'macos' else set()
    require(all(str(p.relative_to(folder)) in allowed_dirs for p in all_paths if p.is_dir()),
            'Unexpected candidate directory')
    for signature in (p for p in files if p.name.endswith('.sig')):
        require(signature.with_suffix('').is_file(), 'Signature has no matching payload')
    return files


def verify_signature(payload, signature, public_key=PUBLIC_KEY):
    require(signature.stat().st_size <= 4096, 'Signature exceeds limit')
    with tempfile.TemporaryDirectory(prefix='cc-desk-verify-') as directory:
        key = Path(directory) / 'public.key'
        sig = Path(directory) / 'signature.minisig'
        for path, raw in [(key, public_key.encode()), (sig, signature.read_bytes())]:
            require(len(raw) <= 4096, 'Minisign wrapper exceeds limit')
            decoded = base64.b64decode(raw.strip(), validate=True)
            require(decoded.startswith(b'untrusted comment: ') and b'\x00' not in decoded, 'Invalid minisign wrapper')
            path.write_bytes(decoded)
        result = subprocess.run(['minisign', '-Vm', str(payload.resolve()), '-p', str(key), '-x', str(sig)],
                                capture_output=True, timeout=60)
        require(result.returncode == 0, 'Candidate signature verification failed: ' + payload.name)


def mac_version(archive):
    with tarfile.open(archive, 'r:gz') as bundle:
        info = bundle.getmember('CC Desk.app/Contents/Info.plist')
        require(info.isfile() and info.size <= 1024 * 1024, 'Invalid macOS Info.plist')
        with bundle.extractfile(info) as source:
            config = plistlib.load(source)
        require(config.get('CFBundleShortVersionString') == '0.18.1' and
                config.get('CFBundleIdentifier') == 'io.github.shawnwu2022.ccdesk', 'macOS package identity mismatch')


def verify():
    run_id = run_id_from_state()
    run = get(f'/actions/runs/{run_id}')
    validate_run(run, run_id)
    require(run.get('status') == 'completed' and run.get('conclusion') == 'success', 'Signer is not successful')
    artifacts = validate_artifacts(get(f'/actions/runs/{run_id}/artifacts?per_page=100'), run_id)
    manifest = json.loads((STATE / 'candidate-manifest.json').read_text())
    require(manifest == {'run_id': run_id, 'artifacts': artifacts}, 'Artifact manifest changed after signer completion')
    identity = source_identity()
    require(identity == json.loads((STATE / 'source-identity.json').read_text()), 'Source identity changed')
    require(not READY.exists(), 'Verified output already exists')
    downloads, archives = Path('candidate-downloads'), Path('candidate-archives')
    downloads.mkdir()
    archives.mkdir()
    inventory = []
    for artifact in artifacts:
        platform = artifact['name'].rsplit('-', 1)[1]
        archive = archives / f"{artifact['id']}.zip"
        download_archive(artifact, archive)
        folder = downloads / artifact['name']
        extract_verified(archive, artifact['digest'], folder)
        files = inspect_platform(folder, platform)
        for file in files:
            if file.name.endswith('.sig'):
                verify_signature(file.with_suffix(''), file)
            if file.name.endswith('.app.tar.gz'):
                mac_version(file)
            inventory.append({'path': str(file.relative_to(downloads)), 'size': file.stat().st_size,
                              'sha256': sha256(file), 'artifact_id': artifact['id']})
    report = {'source': identity, 'signer_run_id': run_id, 'artifacts': artifacts, 'files': inventory,
              'verification': 'Archive SHA-256 and all three original Tauri detached signatures verified. '
                              'macOS DMG is bound by artifact provenance and archive digest; it has no detached signature. '
                              'No binaries executed. Native platform and real CLI acceptance are not established. '
                              'Candidate only; no release or updater publication.'}
    save('verification-report.json', report)
    shutil.copytree(downloads, READY)
    shutil.copy2(STATE / 'verification-report.json', READY / 'verification-report.json')
    print('Verified signed 0.18.1 candidate files prepared', flush=True)


if __name__ == '__main__':
    try:
        require(len(sys.argv) == 2 and sys.argv[1] in ['dispatch', 'wait', 'verify'], 'Usage: signed-build-0181.py dispatch|wait|verify')
        {'dispatch': dispatch, 'wait': wait, 'verify': verify}[sys.argv[1]]()
    except Exception as error:
        # HTTP errors may contain signed storage URLs; never expose raw responses or tokens.
        print(f'::error::{error if isinstance(error, RuntimeError) else type(error).__name__}', file=sys.stderr)
        sys.exit(1)
