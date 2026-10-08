"""Local-only Git fixtures; never contact GitHub or change repository refs."""

import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".github/scripts/backup-cleanup-0181.py"
PLAN = ROOT / ".github/scripts/cleanup-0181-branches.json"
WORKFLOW = ROOT / ".github/workflows/0181-cloud-build-cleanup.yml"


class BackupCleanupTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(SCRIPT.is_file(), "The guarded backup/cleanup implementation is missing")
        spec = importlib.util.spec_from_file_location("cleanup0181", SCRIPT)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.tmp = tempfile.TemporaryDirectory(prefix="ccdesk-cleanup-test-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.remote = self.root / "remote.git"
        self.env = {**os.environ, "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
                    "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.invalid"}
        self.git("init", "--bare", str(self.remote))
        tree = self.git("-C", str(self.remote), "mktree", data="").strip()
        self.base = self.git("-C", str(self.remote), "commit-tree", tree, data="keep\n").strip()
        self.controller = self.git("-C", str(self.remote), "commit-tree", tree, "-p", self.base,
                                   data="controller\n").strip()
        self.plan = json.loads(PLAN.read_text())
        self.plan["source_sha"] = self.base
        self.plan["keep_branches"] = dict.fromkeys(self.plan["keep_branches"], self.base)
        for index, branch in enumerate(self.plan["delete_branches"]):
            branch["expected_sha"] = self.git("-C", str(self.remote), "commit-tree", tree,
                                              "-p", self.base, data=f"unique obsolete {index}\n").strip()
        self.plan["tags"] = {**dict.fromkeys(list(self.plan["tags"])[:63], self.base),
                             "refs/tags/retained-lightweight": self.base}
        expected = self.module.expected_refs(self.plan, self.controller)
        for ref, sha in expected.items():
            self.git("-C", str(self.remote), "update-ref", ref, sha)
        self.git("-C", str(self.remote), "symbolic-ref", "HEAD", "refs/heads/main")
        self.git("-C", str(self.remote), "tag", "-a", "retained-annotated", self.base, "-m", "retained")
        self.plan["tags"]["refs/tags/retained-annotated"] = self.git(
            "-C", str(self.remote), "rev-parse", "refs/tags/retained-annotated").strip()
        self.before = self.refs()
        self.output = self.root / "backup"

    def git(self, *args, data=None):
        result = subprocess.run(["git", *args], input=data, text=True, capture_output=True, env=self.env)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def refs(self):
        return dict(line.split()[::-1] for line in self.git(
            "-C", str(self.remote), "for-each-ref", "--format=%(objectname) %(refname)").splitlines())

    def backup(self):
        return self.module.create_backup(str(self.remote), self.output, self.plan, self.controller)

    def delete(self):
        return self.module.delete_from_backup(str(self.remote), self.output, self.plan, self.controller)

    def artifact_name(self, run_id):
        prefix = re.search(r"name: (\S*backup-)\$\{\{ github.run_id \}\}", WORKFLOW.read_text())
        self.assertIsNotNone(prefix, "The workflow must name its uploaded backup with its run ID")
        return prefix.group(1) + str(run_id)

    # 独立还原全部分支和两种标签，并检查未合入的历史对象。
    def test_Backup_Restore_AllRefs_001(self):
        self.backup()
        self.assertEqual(self.refs(), self.before, "Backup must not mutate remote refs")
        manifest = json.loads((self.output / "manifest.json").read_text())
        self.assertEqual(manifest["refs"], self.before)
        self.assertNotIn(str(self.root), json.dumps(manifest), "Manifest must not disclose local paths")
        restored = self.root / "restored.git"
        self.git("clone", "--mirror", str(self.output / "repository.bundle"), str(restored))
        self.git("-C", str(restored), "fsck", "--full", "--strict")
        restored_refs = dict(line.split()[::-1] for line in self.git(
            "-C", str(restored), "for-each-ref", "--format=%(objectname) %(refname)").splitlines())
        self.assertEqual(restored_refs, self.before)
        for branch in self.plan["delete_branches"]:
            self.git("-C", str(restored), "cat-file", "-e", branch["expected_sha"] + "^{commit}")

    # 删除仅影响固定清单，四个保留分支及所有标签保持精确 SHA。
    def test_Delete_KeepRefs_002(self):
        self.backup()
        self.delete()
        deleted = {"refs/heads/" + branch["branch"] for branch in self.plan["delete_branches"]}
        self.assertEqual(self.refs(), {ref: sha for ref, sha in self.before.items() if ref not in deleted})

    # 备份后目标分支移动则整批拒绝，不按旧快照删除。
    def test_Delete_MovedRef_003(self):
        self.backup()
        ref = "refs/heads/" + self.plan["delete_branches"][0]["branch"]
        self.git("-C", str(self.remote), "update-ref", ref, self.controller)
        moved = self.refs()
        with self.assertRaisesRegex(self.module.GuardError, "snapshot|changed|refs"):
            self.delete()
        self.assertEqual(self.refs(), moved)

    # 备份后出现未审计分支则拒绝整批删除。
    def test_Delete_UnexpectedHead_004(self):
        self.backup()
        self.git("-C", str(self.remote), "update-ref", "refs/heads/new-work", self.base)
        before = self.refs()
        with self.assertRaises(self.module.GuardError):
            self.delete()
        self.assertEqual(self.refs(), before)

    # 保留标签移动同样拒绝，不能声称已备份当前状态。
    def test_Delete_MovedTag_005(self):
        self.backup()
        self.git("-C", str(self.remote), "update-ref", "refs/tags/retained-lightweight", self.controller)
        before = self.refs()
        with self.assertRaises(self.module.GuardError):
            self.delete()
        self.assertEqual(self.refs(), before)

    # 服务端拒绝一个分支时，原子推送必须保留其余所有分支。
    def test_Delete_AtomicRejection_006(self):
        self.backup()
        hook = self.remote / "hooks/update"
        rejected = "refs/heads/" + self.plan["delete_branches"][1]["branch"]
        hook.write_text(f'#!/bin/sh\n[ "$1" != "{rejected}" ]\n')
        hook.chmod(0o755)
        with self.assertRaises(self.module.GuardError):
            self.delete()
        self.assertEqual(self.refs(), self.before, "Server refusal must reject the entire transaction")

    # 新鲜检查与推送之间的目标移动由精确 lease 拒绝。
    def test_Delete_LeaseRace_007(self):
        self.backup()
        original = self.module.remote_refs
        ref = "refs/heads/" + self.plan["delete_branches"][0]["branch"]

        def move_after_read(*args):
            snapshot = original(*args)
            self.git("-C", str(self.remote), "update-ref", ref, self.controller)
            return snapshot

        with patch.object(self.module, "remote_refs", side_effect=move_after_read):
            with self.assertRaises(self.module.GuardError):
                self.delete()
        expected = dict(self.before)
        expected[ref] = self.controller
        self.assertEqual(self.refs(), expected, "A raced ref must prevent all deletion")

    # 损坏的 bundle 不得进入删除流程。
    def test_Delete_CorruptBundle_008(self):
        self.backup()
        with (self.output / "repository.bundle").open("ab") as handle:
            handle.write(b"corruption")
        with self.assertRaisesRegex(self.module.GuardError, "digest|checksum"):
            self.delete()
        self.assertEqual(self.refs(), self.before)

    # 缺少下载备份时不得尝试读取或删除远程分支。
    def test_Delete_MissingBackup_012(self):
        with self.assertRaises(self.module.GuardError):
            self.delete()
        self.assertEqual(self.refs(), self.before)

    # 少一个远程分支的截断快照必须拒绝整批删除。
    def test_Delete_TruncatedRefs_013(self):
        self.backup()
        truncated = dict(self.before)
        del truncated["refs/heads/main"]
        with patch.object(self.module, "remote_refs", return_value=truncated):
            with self.assertRaises(self.module.GuardError):
                self.delete()
        self.assertEqual(self.refs(), self.before)

    # 下载字节与上传摘要不一致时，不能只相信 API 元数据。
    def test_Artifact_CorruptDownload_014(self):
        digest = hashlib.sha256(b"original zip bytes").hexdigest()
        metadata = {"id": 123, "name": self.artifact_name(456), "expired": False,
                    "digest": "sha256:" + digest, "workflow_run": {"id": 456, "head_sha": self.controller}}

        def api(endpoint, output=None):
            if output:
                Path(output).write_bytes(b"corrupted downloaded bytes")
            else:
                return metadata

        with patch.object(self.module, "github_api", side_effect=api):
            with self.assertRaisesRegex(self.module.GuardError, "Downloaded artifact digest"):
                self.module.download_backup(123, digest, 456, self.controller, self.root / "downloaded")
        self.assertEqual(self.refs(), self.before)

    # 保护状态为 true、缺失或目标 SHA 变化时都拒绝删除。
    def test_Delete_ProtectionRefusal_015(self):
        self.backup()
        branches = [{"name": item["branch"], "commit": {"sha": item["expected_sha"]}, "protected": False}
                    for item in self.plan["delete_branches"]]
        for protected in (True, None):
            branches[0]["protected"] = protected
            with patch.object(self.module, "github_api", return_value=branches):
                with self.assertRaises(self.module.GuardError):
                    self.module.delete_from_backup(str(self.remote), self.output, self.plan, self.controller,
                        protection_check=lambda: self.module.check_protection(self.plan))
            self.assertEqual(self.refs(), self.before)
        branches[0]["protected"] = False
        branches[0]["commit"]["sha"] = self.controller
        with patch.object(self.module, "github_api", return_value=branches):
            with self.assertRaises(self.module.GuardError):
                self.module.check_protection(self.plan)

    # 仓库内的无跟踪文件和进程凭证不出现在备份清单或 bundle 中。
    def test_Backup_NoAmbientFiles_016(self):
        secret = "fixture-process-secret-never-back-up"
        (self.remote / "private-untracked.txt").write_text(secret)
        with patch.dict(os.environ, {"GH_TOKEN": secret}):
            self.backup()
        self.assertEqual({path.name for path in self.output.iterdir()}, self.module.FILES)
        for path in self.output.iterdir():
            self.assertNotIn(secret.encode(), path.read_bytes())

    # 备份入口也拒绝已经移动的主分支。
    def test_Backup_MovedMain_009(self):
        self.git("-C", str(self.remote), "update-ref", "refs/heads/main", self.controller)
        with self.assertRaises(self.module.GuardError):
            self.backup()
        self.assertFalse((self.output / "manifest.json").exists())

    # 独立下载的产物必须匹配 API 回执、run、控制器及压缩包摘要。
    def test_Artifact_VerifiedDownload_010(self):
        self.backup()
        archive = self.root / "artifact.zip"
        with zipfile.ZipFile(archive, "w") as handle:
            for path in self.output.iterdir():
                handle.write(path, path.name)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        metadata = {"id": 123, "name": self.artifact_name(456), "expired": False,
                    "digest": "sha256:" + digest, "workflow_run": {"id": 456, "head_sha": self.controller}}

        def api(endpoint, output=None):
            if endpoint.endswith("/zip"):
                shutil.copyfile(archive, output)
                return None
            return copy.deepcopy(metadata)

        downloaded = self.root / "downloaded"
        with patch.object(self.module, "github_api", side_effect=api):
            self.module.download_backup(123, digest, 456, self.controller, downloaded)
            self.assertEqual((downloaded / "repository.bundle").read_bytes(),
                             (self.output / "repository.bundle").read_bytes())
            metadata["workflow_run"]["id"] = 999
            with self.assertRaises(self.module.GuardError):
                self.module.download_backup(123, digest, 456, self.controller, self.root / "wrong-run")
            metadata["workflow_run"]["id"] = 456
            with self.assertRaises(self.module.GuardError):
                self.module.download_backup(123, "0" * 64, 456, self.controller, self.root / "wrong-digest")

    # 清单固定为已审核的 20/7/5 分类，不能自动选择新分支。
    def test_Plan_FixedAudit_011(self):
        plan = json.loads(PLAN.read_text())
        counts = {}
        for branch in plan["delete_branches"]:
            counts[branch["disposition"]] = counts.get(branch["disposition"], 0) + 1
        self.assertEqual(counts, {"delete_exact_ancestor": 20, "delete_rejected_release_experiment": 7,
                                  "delete_semantically_superseded_runtime": 5})
        self.assertEqual(len(plan["tags"]), 65)
        self.assertEqual(len(set(branch["branch"] for branch in plan["delete_branches"])), 32)

    # 真实删除成功但后续读取失败时，持久化结果必须保留未知状态。
    def test_Delete_PostReadUnknown_017(self):
        self.backup()
        plan_path = self.root / "plan.json"
        plan_path.write_text(json.dumps(self.plan))
        result_path = self.root / "cleanup-result.json"
        original = self.module.remote_refs
        calls = 0

        def lose_post_read(*args):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise self.module.GuardError("Post-delete read unavailable")
            return original(*args)

        def download(artifact_id, digest, run_id, controller_sha, output):
            shutil.copytree(self.output, output)
            return {"artifact_id": artifact_id, "download_verified": True}

        workflow = WORKFLOW.read_text()
        self.assertRegex(workflow, r"on:\s*\n  push:")
        workflow_ref = self.module.REPOSITORY + "/.github/workflows/0181-cloud-build-cleanup.yml@refs/heads/" + self.plan["controller_branch"]
        env = {"GITHUB_REPOSITORY": self.module.REPOSITORY, "GITHUB_REF": "refs/heads/" + self.plan["controller_branch"],
               "GITHUB_SHA": self.controller, "GITHUB_EVENT_NAME": "push", "GITHUB_WORKFLOW_REF": workflow_ref,
               "GITHUB_RUN_ID": "456", "GITHUB_RUN_ATTEMPT": "1"}
        argv = [str(SCRIPT), "delete", "--artifact-id", "123", "--artifact-digest", "0" * 64,
                "--run-id", "456", "--controller-sha", self.controller, "--result", str(result_path)]
        with patch.dict(os.environ, env), patch.object(self.module.sys, "argv", argv), \
                patch.object(self.module, "PLAN_PATH", plan_path), patch.object(self.module, "REMOTE", str(self.remote)), \
                patch.object(self.module, "check_protection"), patch.object(self.module, "download_backup", side_effect=download), \
                patch.object(self.module, "remote_refs", side_effect=lose_post_read):
            self.assertEqual(self.module.main(), 1)
        result = json.loads(result_path.read_text())
        self.assertEqual(result["status"], "outcome_unknown", "A lost post-read cannot mean refused or verified deleted")
        self.assertTrue(result["deletion_attempted"])
        self.assertEqual(len(self.refs()), len(self.before) - 32, "The fixture must actually delete before losing the receipt")


if __name__ == "__main__":
    unittest.main()
