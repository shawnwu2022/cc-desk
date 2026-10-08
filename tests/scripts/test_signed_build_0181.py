"""Offline tests for the one-time signed candidate controller."""
import base64
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / '.github/scripts/signed-build-0181.py'
spec = importlib.util.spec_from_file_location('signed_build', SCRIPT)
m = importlib.util.module_from_spec(spec) if SCRIPT.exists() else None
if m:
    spec.loader.exec_module(m)


class SignedBuildTests(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(m, 'The signed candidate controller has not been implemented')
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.old = Path.cwd()
        os.chdir(self.temp.name)
        self.addCleanup(os.chdir, self.old)
        self.env = patch.dict(os.environ, {'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_REPOSITORY': m.REPO})
        self.env.start()
        self.addCleanup(self.env.stop)
        self.posts = []
        self.run = {'id': 123, 'path': m.WORKFLOW, 'event': 'workflow_dispatch',
                    'head_sha': m.SHA, 'head_branch': m.REF, 'run_attempt': 1,
                    'repository': {'full_name': m.REPO}, 'head_repository': {'full_name': m.REPO},
                    'html_url': f'https://github.com/{m.REPO}/actions/runs/123',
                    'status': 'completed', 'conclusion': 'success'}
        self.artifacts = [{'id': i, 'name': f'cc-desk-candidate-{m.SHA}-{platform}',
                           'expired': False, 'size_in_bytes': 100,
                           'digest': 'sha256:' + 'a' * 64,
                           'workflow_run': {'id': 123, 'head_sha': m.SHA, 'head_branch': m.REF}}
                          for i, platform in enumerate(['windows', 'macos', 'linux'], 1)]

    def fake_api(self, method, path, body=None):
        if method == 'POST':
            self.posts.append((path, body))
            return 200, {'workflow_run_id': 123, 'run_url': f'https://api.github.com/repos/{m.REPO}/actions/runs/123',
                         'html_url': self.run['html_url']}
        if path == '':
            return 200, {'full_name': m.REPO}
        if path.startswith('/git/ref/'):
            return 200, {'ref': 'refs/heads/' + m.REF, 'object': {'type': 'commit', 'sha': m.SHA}}
        if path.startswith('/git/commits/'):
            return 200, {'sha': m.SHA, 'tree': {'sha': m.TREE}}
        if path.startswith('/contents/'):
            config = {'version': '0.18.1', 'plugins': {'updater': {'pubkey': m.PUBLIC_KEY}}}
            return 200, {'encoding': 'base64', 'content': base64.b64encode(json.dumps(config).encode()).decode()}
        if path.endswith('/artifacts?per_page=100'):
            return 200, {'total_count': len(self.artifacts), 'artifacts': self.artifacts}
        if path.startswith('/actions/workflows/release.yml/runs?'):
            return 200, {'total_count': 0, 'workflow_runs': []}
        if path == '/actions/runs/123':
            return 200, self.run
        raise AssertionError(f'Unexpected request {method} {path}')

    # 单次调度后保存身份，重复调用不得再次提交。
    def test_DispatchOnce_Persists_001(self):
        with patch.object(m, 'api', side_effect=self.fake_api):
            m.dispatch()
            with self.assertRaisesRegex(RuntimeError, 'already|exists'):
                m.dispatch()
        self.assertEqual(len(self.posts), 1)
        self.assertEqual(self.posts[0][1], {'ref': m.REF})
        self.assertEqual(json.loads((m.STATE / 'dispatch-response.json').read_text())['workflow_run_id'], 123)

    # 工作流重跑没有本地状态也必须拒绝调度。
    def test_DispatchRerun_Rejects_002(self):
        with patch.dict(os.environ, {'GITHUB_RUN_ATTEMPT': '2'}), patch.object(m, 'api') as api:
            with self.assertRaises(RuntimeError):
                m.dispatch()
        api.assert_not_called()

    # 204 回执无法证明具体运行身份，重试不得再次提交。
    def test_DispatchAmbiguous_NoRetry_003(self):
        def ambiguous(method, path, body=None):
            if method == 'POST':
                self.posts.append(path)
                return 204, None
            return self.fake_api(method, path, body)
        with patch.object(m, 'api', side_effect=ambiguous):
            for _ in range(2):
                with self.assertRaises(RuntimeError):
                    m.dispatch()
        self.assertEqual(len(self.posts), 1)
        self.assertTrue((m.STATE / 'dispatch-intent.json').is_file())

    # 已存在调度记录时，即使控制器是首次执行也不得重复提交。
    def test_PriorRun_NoDispatch_011(self):
        def existing(method, path, body=None):
            if path.startswith('/actions/workflows/release.yml/runs?'):
                return 200, {'total_count': 1, 'workflow_runs': [self.run]}
            return self.fake_api(method, path, body)
        with patch.object(m, 'api', side_effect=existing), self.assertRaises(RuntimeError):
            m.dispatch()
        self.assertEqual(self.posts, [])

    # POST 传输超时后保存意图，下一次调用仍拒绝提交。
    def test_DispatchTimeout_NoRetry_012(self):
        def timeout(method, path, body=None):
            if method == 'POST':
                self.posts.append(path)
                raise TimeoutError('network timeout')
            return self.fake_api(method, path, body)
        with patch.object(m, 'api', side_effect=timeout):
            for _ in range(2):
                with self.assertRaises(RuntimeError):
                    m.dispatch()
        self.assertEqual(len(self.posts), 1)

    # 来源树或分支漂移必须在提交前拒绝。
    def test_SourceDrift_Rejects_004(self):
        for prefix in ['/git/commits/', '/git/ref/']:
            def drift(method, path, body=None):
                status, data = self.fake_api(method, path, body)
                if path.startswith(prefix):
                    data = {'sha': m.SHA, 'tree': {'sha': 'b' * 40}, 'object': {'sha': 'b' * 40}}
                return status, data
            with self.subTest(prefix=prefix), patch.object(m, 'api', side_effect=drift):
                with self.assertRaises(RuntimeError):
                    m.dispatch()
        self.assertEqual(self.posts, [])

    # 运行来源、分支、事件、工作流和次数都绑定到预期值。
    def test_RunBinding_Rejects_005(self):
        for field, value in [('head_sha', 'b' * 40), ('head_branch', 'main'), ('event', 'push'),
                             ('path', '.github/workflows/ci.yml'), ('run_attempt', 2)]:
            bad = {**self.run, field: value}
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                m.validate_run(bad, 123)

    # 构建失败不可产生候选清单或继续验签。
    def test_BuildFailure_Stops_006(self):
        with patch.object(m, 'api', side_effect=self.fake_api):
            m.dispatch()
            self.run['conclusion'] = 'failure'
            with self.assertRaisesRegex(RuntimeError, 'failure'):
                m.wait()
        self.assertFalse((m.STATE / 'candidate-manifest.json').exists())

    # 过期、错运行或重复平台产物必须拒绝。
    def test_ArtifactBinding_Rejects_007(self):
        mutations = [('expired', True), ('digest', None), ('name', 'other')]
        for field, value in mutations:
            bad = copy.deepcopy(self.artifacts)
            bad[0][field] = value
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                m.validate_artifacts({'total_count': 3, 'artifacts': bad}, 123)
        bad = copy.deepcopy(self.artifacts)
        bad[0]['workflow_run']['id'] = 999
        with self.assertRaises(RuntimeError):
            m.validate_artifacts({'total_count': 3, 'artifacts': bad}, 123)

    # ZIP 摘要、路径穿越和符号链接在解压前拒绝。
    def test_ArchiveSafety_Rejects_008(self):
        import hashlib
        for member, mode in [('../escape.exe', 0), ('safe.exe', 0o120777 << 16)]:
            path = Path('archive.zip')
            with zipfile.ZipFile(path, 'w') as archive:
                entry = zipfile.ZipInfo(member)
                entry.external_attr = mode
                archive.writestr(entry, b'test')
            digest = 'sha256:' + hashlib.sha256(path.read_bytes()).hexdigest()
            with self.subTest(member=member), self.assertRaises(RuntimeError):
                m.extract_verified(path, digest, Path('output'))
            self.assertFalse(Path('escape.exe').exists())
        with self.assertRaisesRegex(RuntimeError, 'digest'):
            m.extract_verified(path, 'sha256:' + '0' * 64, Path('output'))

    # 缺失签名、多余文件和目录符号链接均不可当成完整平台包。
    def test_MissingPair_Rejects_009(self):
        folder = Path('windows')
        folder.mkdir()
        (folder / 'CC Desk_0.18.1_x64-setup.exe').write_bytes(b'test')
        with self.assertRaises(RuntimeError):
            m.inspect_platform(folder, 'windows')
        (folder / 'CC Desk_0.18.1_x64-setup.exe.sig').write_bytes(b'sig')
        self.assertEqual(len(m.inspect_platform(folder, 'windows')), 2)
        (folder / 'unexpected.txt').write_bytes(b'extra')
        with self.assertRaises(RuntimeError):
            m.inspect_platform(folder, 'windows')

    # 存储重定向不得携带 GitHub token，未知目标主机在请求前拒绝。
    def test_Redirect_DropsToken_013(self):
        import hashlib
        import urllib.error
        payload = b'archive bytes'
        artifact = {**self.artifacts[0], 'digest': 'sha256:' + hashlib.sha256(payload).hexdigest()}
        response = io.BytesIO(payload)
        response.status = 200
        redirect = urllib.error.HTTPError('https://api.github.com', 302, 'redirect',
                                         {'Location': 'https://storage.blob.core.windows.net/file?sig=redacted'}, None)
        with patch.object(m, 'request', side_effect=redirect), patch.object(m.urllib.request, 'build_opener') as build:
            build.return_value.open.return_value = response
            m.download_archive(artifact, Path('archive.zip'))
            request = build.return_value.open.call_args.args[0]
            self.assertIsNone(request.get_header('Authorization'))
        self.assertEqual(Path('archive.zip').read_bytes(), payload)
        redirect.headers['Location'] = 'https://evil.example/file'
        with patch.object(m, 'request', side_effect=redirect), patch.object(m.urllib.request, 'build_opener') as build:
            with self.assertRaises(RuntimeError):
                m.download_archive(artifact, Path('evil.zip'))
            build.assert_not_called()

    # 完整验证仅在三个平台都通过后复制原始包、原始签名和哈希清单。
    def test_Verify_PreservesFiles_014(self):
        import hashlib
        import plistlib
        import tarfile
        mac = io.BytesIO()
        with tarfile.open(fileobj=mac, mode='w:gz') as archive:
            data = plistlib.dumps({'CFBundleShortVersionString': '0.18.1',
                                  'CFBundleIdentifier': 'io.github.shawnwu2022.ccdesk'})
            item = tarfile.TarInfo('CC Desk.app/Contents/Info.plist')
            item.size = len(data)
            archive.addfile(item, io.BytesIO(data))
        contents = {
            'windows': {'CC Desk_0.18.1_x64-setup.exe': b'windows', 'CC Desk_0.18.1_x64-setup.exe.sig': b'original-win-sig'},
            'linux': {'CC Desk_0.18.1_amd64.AppImage': b'linux', 'CC Desk_0.18.1_amd64.AppImage.sig': b'original-linux-sig'},
            'macos': {'macos/CC Desk.app.tar.gz': mac.getvalue(), 'macos/CC Desk.app.tar.gz.sig': b'original-mac-sig',
                      'dmg/CC Desk_0.18.1_aarch64.dmg': b'dmg'}}
        archives = {}
        for artifact in self.artifacts:
            platform = artifact['name'].rsplit('-', 1)[1]
            stream = io.BytesIO()
            with zipfile.ZipFile(stream, 'w') as archive:
                for name, data in contents[platform].items():
                    archive.writestr(name, data)
            archives[artifact['id']] = stream.getvalue()
            artifact['digest'] = 'sha256:' + hashlib.sha256(stream.getvalue()).hexdigest()
        def download(artifact, path):
            path.write_bytes(archives[artifact['id']])
        with patch.object(m, 'api', side_effect=self.fake_api), patch.object(m, 'download_archive', side_effect=download), \
                patch.object(m, 'verify_signature') as verify:
            m.dispatch()
            m.wait()
            m.verify()
        self.assertEqual(verify.call_count, 3)
        report = json.loads((m.READY / 'verification-report.json').read_text())
        self.assertEqual(len(report['files']), 7)
        for platform, files in contents.items():
            for name, data in files.items():
                self.assertEqual((m.READY / f'cc-desk-candidate-{m.SHA}-{platform}' / name).read_bytes(), data)

    # 轮询的短暂 503 只重试读取，不能重复提交构建。
    def test_PollTransient_ReadOnly_016(self):
        import urllib.error
        reads = []
        def transient(method, path, body=None):
            if path == '/actions/runs/123':
                reads.append(path)
                if len(reads) == 1:
                    raise urllib.error.HTTPError('https://api.github.com', 503, 'unavailable', {}, None)
            return self.fake_api(method, path, body)
        with patch.object(m, 'api', side_effect=transient), patch.object(m.time, 'sleep'):
            m.dispatch()
            m.wait()
        self.assertEqual(len(self.posts), 1)
        self.assertEqual(len(reads), 2)
        self.assertTrue((m.STATE / 'candidate-manifest.json').is_file())

    # 已完成但产物缺签时不能创建 ready 目录。
    def test_WrongVersion_Rejects_017(self):
        folder = Path('linux')
        folder.mkdir()
        for name in ['CC Desk_0.18.0_amd64.AppImage', 'CC Desk_0.18.0_amd64.AppImage.sig']:
            (folder / name).write_bytes(b'wrong version')
        with self.assertRaises(RuntimeError):
            m.inspect_platform(folder, 'linux')
        self.assertFalse(m.READY.exists())

    # 固定公钥必须与已检出的候选源码一致。
    def test_PublicKey_MatchesSource_015(self):
        config = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())
        self.assertEqual(m.PUBLIC_KEY, config['plugins']['updater']['pubkey'])

    # 公共真实签名向量通过，修改载荷后失败；无私钥参与测试。
    @unittest.skipUnless(shutil.which('minisign'), 'minisign must be installed for real signature vectors')
    def test_RealSignature_Tamper_010(self):
        fixture = ROOT / 'tests/fixtures/version-history-minisign'
        public = (fixture / 'tauri-public-key.txt').read_text().strip()
        signature = fixture / 'tauri-signature.sig'
        payload = Path('payload.bin')
        payload.write_bytes((fixture / 'payload.bin').read_bytes())
        m.verify_signature(payload, signature, public)
        payload.write_bytes(b'changed')
        with self.assertRaises(RuntimeError):
            m.verify_signature(payload, signature, public)


if __name__ == '__main__':
    unittest.main()
