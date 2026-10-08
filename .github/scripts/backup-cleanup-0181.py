#!/usr/bin/env python3
"""One-off reviewed 0.18.1 backup, then artifact-gated atomic branch cleanup.

Only main() contacts the fixed GitHub repository. Helpers accept local bare
remotes so the destructive behavior can be exercised without external writes.
"""

import argparse
import base64
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile


REPOSITORY = "shawnwu2022/cc-desk"
REMOTE = "https://github.com/" + REPOSITORY + ".git"
PLAN_PATH = Path(__file__).with_name("cleanup-0181-branches.json")
FILES = {"repository.bundle", "manifest.json", "SHA256SUMS"}


class GuardError(Exception):
    """A fixed, safe-to-log refusal; raw subprocess/API errors stay private."""


def require(condition, message):
    if not condition:
        raise GuardError(message)


def git_environment():
    # No inherited alternate object directories, tracing or persistent Git config.
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
               GIT_TERMINAL_PROMPT="0", GIT_NO_REPLACE_OBJECTS="1")
    if env.get("GH_TOKEN"):
        credential = base64.b64encode(("x-access-token:" + env["GH_TOKEN"]).encode()).decode()
        env.update(GIT_CONFIG_COUNT="1", GIT_CONFIG_KEY_0="http.https://github.com/.extraheader",
                   GIT_CONFIG_VALUE_0="AUTHORIZATION: basic " + credential)
    return env


def git(*args):
    result = subprocess.run(["git", *map(str, args)], capture_output=True,
                            text=True, env=git_environment())
    require(result.returncode == 0, "Git operation failed; no retry or protection override was attempted")
    return result.stdout


def canonical_digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def file_digest(path):
    with Path(path).open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def expected_refs(plan, controller_sha):
    require(re.fullmatch(r"[0-9a-f]{40}", controller_sha), "Invalid controller SHA")
    keep = {"refs/heads/" + name: sha for name, sha in plan["keep_branches"].items()}
    keep["refs/heads/" + plan["controller_branch"]] = controller_sha
    targets = {"refs/heads/" + item["branch"]: item["expected_sha"] for item in plan["delete_branches"]}
    require(not keep.keys() & targets.keys(), "A deletion target overlaps a retained branch")
    require(len(targets) == len(plan["delete_branches"]), "Duplicate deletion targets")
    return {**keep, **targets, **plan["tags"]}


def load_plan():
    plan = json.loads(PLAN_PATH.read_text())
    require(plan["schema_version"] == 1 and plan["repository"] == REPOSITORY, "Wrong reviewed plan")
    require(Counter(item["disposition"] for item in plan["delete_branches"]) == {
        "delete_exact_ancestor": 20, "delete_rejected_release_experiment": 7,
        "delete_semantically_superseded_runtime": 5}, "Reviewed disposition counts changed")
    require(set(plan["keep_branches"]) == {"main", "feat/unified-workspace-ux",
            "release/0.18.1-source-5ed35db"}, "Retained branch set changed")
    require(plan["controller_branch"] == "ops/0181-cloud-build-20261008", "Wrong controller branch")
    require(len(plan["tags"]) == 65, "Reviewed tag set changed")
    for ref, sha in expected_refs(plan, "0" * 40).items():
        git("check-ref-format", ref)
        require(re.fullmatch(r"[0-9a-f]{40}", sha), "Invalid reviewed ref SHA")
    return plan


def parse_refs(text):
    result = {}
    for line in text.splitlines():
        sha, ref = line.split()
        require(ref not in result, "Duplicate ref in snapshot")
        result[ref] = sha
    return result


def local_refs(repository):
    return parse_refs(git("-C", repository, "for-each-ref", "--format=%(objectname) %(refname)"))


def remote_refs(remote):
    return parse_refs(git("ls-remote", "--refs", remote, "refs/heads/*", "refs/tags/*"))


def verify_restore(bundle, expected):
    # Verify in an empty repository, so prerequisite-dependent bundles fail.
    with tempfile.TemporaryDirectory(prefix="ccdesk-restore-") as tmp:
        restored = Path(tmp) / "restored.git"
        git("init", "--bare", restored)
        git("-C", restored, "bundle", "verify", bundle)
        git("-C", restored, "fetch", "--no-tags", bundle,
            "+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*")
        git("-C", restored, "symbolic-ref", "HEAD", "refs/heads/main")
        require(local_refs(restored) == expected, "Restored refs differ from the complete snapshot")
        require(not (restored / "objects/info/alternates").exists(), "Restoration has external object dependencies")
        git("-C", restored, "fsck", "--full", "--strict")


def create_backup(remote, output, plan, controller_sha):
    output = Path(output).resolve()
    require(not output.exists(), "Backup output must be a fresh directory")
    expected = expected_refs(plan, controller_sha)
    with tempfile.TemporaryDirectory(prefix="ccdesk-mirror-") as tmp:
        mirror = Path(tmp) / "mirror.git"
        git("init", "--bare", mirror)
        git("-C", mirror, "fetch", "--prune", "--no-tags", remote,
            "+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*")
        git("-C", mirror, "symbolic-ref", "HEAD", "refs/heads/main")
        snapshot = local_refs(mirror)
        require(snapshot == expected, "Live refs changed or include unexpected heads/tags; fresh review required")
        git("-C", mirror, "fsck", "--full", "--strict")
        bundle = Path(tmp) / "repository.bundle"
        git("-C", mirror, "bundle", "create", bundle, "--all")
        verify_restore(bundle, snapshot)
        require(remote_refs(remote) == snapshot, "Live refs changed while creating the backup")
        manifest = {"schema_version": 1, "repository": plan["repository"],
                    "controller_sha": controller_sha, "plan_sha256": canonical_digest(plan),
                    "bundle_sha256": file_digest(bundle), "refs": snapshot,
                    "restore_verified": True}
        output.mkdir(parents=True)
        shutil.copyfile(bundle, output / "repository.bundle")
        (output / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        (output / "SHA256SUMS").write_text("".join(
            f"{file_digest(output / name)}  {name}\n" for name in ("repository.bundle", "manifest.json")))
    return manifest


def verify_backup(directory, plan, controller_sha):
    directory = Path(directory).resolve()
    require(directory.is_dir(), "Downloaded backup is missing")
    require({path.name for path in directory.iterdir()} == FILES, "Unexpected backup inventory")
    require(all((directory / name).is_file() and not (directory / name).is_symlink()
                for name in FILES), "Backup entries must be regular files")
    checksums = "".join(f"{file_digest(directory / name)}  {name}\n"
                        for name in ("repository.bundle", "manifest.json"))
    require((directory / "SHA256SUMS").read_text() == checksums, "Backup checksum mismatch")
    manifest = json.loads((directory / "manifest.json").read_text())
    require(manifest["schema_version"] == 1 and manifest["repository"] == plan["repository"]
            and manifest["controller_sha"] == controller_sha
            and manifest["plan_sha256"] == canonical_digest(plan), "Backup provenance mismatch")
    require(manifest["bundle_sha256"] == file_digest(directory / "repository.bundle"), "Bundle digest mismatch")
    require(manifest["refs"] == expected_refs(plan, controller_sha), "Backup snapshot differs from reviewed refs")
    verify_restore(directory / "repository.bundle", manifest["refs"])
    return manifest


def delete_from_backup(remote, directory, plan, controller_sha, protection_check=None, before_push=None):
    manifest = verify_backup(directory, plan, controller_sha)
    if protection_check:
        protection_check()
    require(remote_refs(remote) == manifest["refs"], "Live refs changed from the downloaded backup snapshot")
    targets = {"refs/heads/" + item["branch"]: item["expected_sha"] for item in plan["delete_branches"]}
    # One atomic transaction; each individual deletion is compare-and-swap.
    with tempfile.TemporaryDirectory(prefix="ccdesk-delete-") as tmp:
        git("init", "--bare", tmp)
        if before_push:
            before_push()
        git("-C", tmp, "push", "--atomic", "--porcelain",
            *["--force-with-lease=" + ref + ":" + sha for ref, sha in targets.items()],
            remote, "--delete", *targets)
    retained = {ref: sha for ref, sha in manifest["refs"].items() if ref not in targets}
    require(remote_refs(remote) == retained, "Post-delete refs differ from retained snapshot; inspect remote without retry")
    return {"status": "deleted", "deleted_branches": sorted(targets), "retained_refs": retained}


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, file, code, message, headers, new_url):
        return None


def github_api(endpoint, output=None):
    token = os.environ.get("GH_TOKEN")
    require(bool(token), "GH_TOKEN is required")
    request = urllib.request.Request("https://api.github.com/" + endpoint, headers={
        "Authorization": "Bearer " + token, "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28", "User-Agent": "ccdesk-0181-backup"})
    opener = urllib.request.build_opener(NoRedirect())
    try:
        with opener.open(request, timeout=120) as response:
            require(output is None, "Artifact download did not return a storage redirect")
            return json.load(response)
    except urllib.error.HTTPError as error:
        require(output is not None and error.code == 302, "GitHub read failed; no cleanup authorized")
        destination = error.headers.get("Location", "")
        error.close()
    url = urllib.parse.urlsplit(destination)
    hostname = url.hostname or ""
    require(url.scheme == "https" and url.port in (None, 443) and not url.username
            and not url.password and not url.fragment and (
                hostname.endswith(".blob.core.windows.net") or hostname.endswith(".githubusercontent.com")
                or re.fullmatch(r"[a-z0-9.-]+\.s3(?:\.[a-z0-9-]+)?\.amazonaws\.com", hostname)),
            "Unexpected artifact storage redirect")
    # A fresh request with no Authorization; redirects from storage are refused.
    with opener.open(urllib.request.Request(destination), timeout=120) as response:
        with Path(output).open("wb") as handle:
            shutil.copyfileobj(response, handle)


def check_protection(plan):
    # Ordinary branch reads work with contents:read; no admin protection API.
    branches = []
    page = 1
    while True:
        batch = github_api(f"repos/{REPOSITORY}/branches?per_page=100&page={page}")
        require(isinstance(batch, list), "Unknown branch protection response")
        branches.extend(batch)
        if len(batch) < 100:
            break
        page += 1
    by_name = {branch["name"]: branch for branch in branches}
    for item in plan["delete_branches"]:
        branch = by_name.get(item["branch"], {})
        require(branch.get("protected") is False, "Deletion target is protected or protection status is unknown")
        require(branch.get("commit", {}).get("sha") == item["expected_sha"], "Deletion target SHA changed")


def download_backup(artifact_id, digest, run_id, controller_sha, output):
    require(artifact_id > 0 and run_id > 0 and re.fullmatch(r"[0-9a-f]{64}", digest), "Invalid artifact receipt")
    metadata = github_api(f"repos/{REPOSITORY}/actions/artifacts/{artifact_id}")
    run = metadata.get("workflow_run", {})
    require(metadata.get("id") == artifact_id and metadata.get("expired") is False
            and metadata.get("name") == f"cc-desk-0181-all-refs-backup-{run_id}"
            and run.get("id") == run_id and run.get("head_sha") == controller_sha
            and metadata.get("digest") == "sha256:" + digest, "Uploaded artifact provenance/digest mismatch")
    output = Path(output)
    require(not output.exists(), "Downloaded backup directory must be fresh")
    with tempfile.TemporaryDirectory(prefix="ccdesk-artifact-") as tmp:
        archive = Path(tmp) / "artifact.zip"
        github_api(f"repos/{REPOSITORY}/actions/artifacts/{artifact_id}/zip", output=archive)
        require(file_digest(archive) == digest, "Downloaded artifact digest mismatch")
        with zipfile.ZipFile(archive) as handle:
            require(len(handle.namelist()) == len(FILES) and set(handle.namelist()) == FILES,
                    "Unexpected artifact ZIP inventory")
            output.mkdir(parents=True)
            for name in sorted(FILES):
                with handle.open(name) as source, (output / name).open("wb") as target:
                    shutil.copyfileobj(source, target)
    return {"artifact_id": artifact_id, "artifact_digest": digest, "run_id": run_id,
            "artifact_url": f"https://github.com/{REPOSITORY}/actions/runs/{run_id}/artifacts/{artifact_id}",
            "download_verified": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="phase", required=True)
    backup = commands.add_parser("backup")
    backup.add_argument("--output", required=True, type=Path)
    delete = commands.add_parser("delete")
    delete.add_argument("--artifact-id", required=True, type=int)
    delete.add_argument("--artifact-digest", required=True)
    delete.add_argument("--run-id", required=True, type=int)
    delete.add_argument("--result", type=Path, default=Path("cleanup-result.json"))
    for command in (backup, delete):
        command.add_argument("--controller-sha", required=True)
    args = parser.parse_args()
    result = {"status": "refused", "repository": REPOSITORY, "deletion_attempted": False}

    def persist_result():
        temporary = args.result.with_name(args.result.name + ".tmp")
        temporary.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
        temporary.replace(args.result)

    def record_submission():
        # Persist before sending the mutation: a killed process or lost receipt
        # must never be reported as proof that no deletion happened.
        result.update(status="outcome_unknown", deletion_attempted=True)
        persist_result()

    try:
        plan = load_plan()
        workflow_ref = (REPOSITORY + "/.github/workflows/0181-cloud-build-cleanup.yml@refs/heads/"
                        + plan["controller_branch"])
        require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY
                and os.environ.get("GITHUB_REF") == "refs/heads/" + plan["controller_branch"]
                and os.environ.get("GITHUB_SHA") == args.controller_sha
                and os.environ.get("GITHUB_EVENT_NAME") == "push"
                and os.environ.get("GITHUB_WORKFLOW_REF") == workflow_ref
                and os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "Wrong workflow context")
        if args.phase == "backup":
            check_protection(plan)
            create_backup(REMOTE, args.output, plan, args.controller_sha)
            print("Verified self-contained backup and all reviewed refs; upload required before deletion")
        else:
            require(os.environ.get("GITHUB_RUN_ID") == str(args.run_id), "Wrong artifact workflow run")
            with tempfile.TemporaryDirectory(prefix="ccdesk-downloaded-") as tmp:
                directory = Path(tmp) / "backup"
                result["artifact_receipt"] = download_backup(args.artifact_id, args.artifact_digest,
                    args.run_id, args.controller_sha, directory)
                result.update(delete_from_backup(REMOTE, directory, plan, args.controller_sha,
                    protection_check=lambda: check_protection(plan), before_push=record_submission))
            print("Deleted exactly 32 reviewed branches; retained branches and tags verified")
        return 0
    except GuardError as error:
        result["reason"] = str(error)
        print(str(error), file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile):
        result["reason"] = "Backup/cleanup could not be verified; no retry attempted"
        print(result["reason"], file=sys.stderr)
        return 1
    finally:
        if args.phase == "delete":
            persist_result()


if __name__ == "__main__":
    sys.exit(main())
