"""Small checks for a scratch harness, before starting an installed GUI.

Treat paths/booleans as local test inputs. Only safe file digests/sizes cross the
diagnostic output boundary; verification never opens an executable for execution.
"""
import argparse
import hashlib
import hmac
import json
import os
from pathlib import Path
import stat
from verify_named_nsis import DIGEST, MAX_INSTALLER, MAX_STREAM, MEMBER, Rejected, require

MAX_APP = MAX_STREAM
REPARSE = 0x400


def regular_path(p):
    """No link or reparse along the selected test path."""
    absolute = Path(p).absolute()
    for ancestor in reversed((*absolute.parents,)):
        s = os.lstat(ancestor)
        require(stat.S_ISDIR(s.st_mode) and not getattr(s, "st_file_attributes", 0) & REPARSE, "UNSAFE_PATH")
    s = os.lstat(absolute)
    require(stat.S_ISREG(s.st_mode) and not getattr(s, "st_file_attributes", 0) & REPARSE, "UNSAFE_FILE")
    return absolute, s


def measure(path, maximum):
    file, snapshot = regular_path(path)
    require(0 < snapshot.st_size <= maximum, "FILE_SIZE")
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(file, flags)
    with os.fdopen(fd, "rb") as stream:
        opened = os.fstat(stream.fileno())
        require(stat.S_ISREG(opened.st_mode) and os.path.samestat(snapshot, opened) and
                opened.st_size == snapshot.st_size, "FILE_CHANGED")
        total = 0
        digest = hashlib.sha256()
        while chunk := stream.read(256 * 1024):
            total += len(chunk)
            require(total <= maximum, "FILE_SIZE")
            digest.update(chunk)
        after = os.fstat(stream.fileno())
        require(total == snapshot.st_size == after.st_size and
                opened.st_mtime_ns == after.st_mtime_ns, "FILE_CHANGED")
    latest = os.lstat(file)
    require(os.path.samestat(opened, latest) and latest.st_size == total and
            not getattr(latest, "st_file_attributes", 0) & REPARSE, "FILE_CHANGED")
    return digest.hexdigest(), total


def exact_path(a, b):
    return os.path.normcase(os.path.abspath(a)) == os.path.normcase(os.path.abspath(b))


def check_installer(invoked, pinned, expected_sha):
    require(DIGEST.fullmatch(expected_sha), "INSTALLER_DIGEST_FORMAT")
    require(exact_path(invoked, pinned), "INSTALLER_PATH")
    digest, size = measure(invoked, MAX_INSTALLER)
    require(hmac.compare_digest(digest, expected_sha), "INSTALLER_DRIFT")
    return {"installerSha256": digest, "installerBytes": size}


def check_installed(selected, installed_dir, member_name, expected_sha, expected_bytes):
    require(member_name == MEMBER, "MEMBER_NAME")
    require(DIGEST.fullmatch(expected_sha), "MEMBER_DIGEST_FORMAT")
    require(type(expected_bytes) is int and 0 < expected_bytes < MAX_APP, "MEMBER_SIZE_FORMAT")
    require(Path(selected).name == MEMBER and exact_path(selected, Path(installed_dir) / MEMBER), "INSTALLED_PATH")
    digest, size = measure(selected, MAX_APP)
    require(size == expected_bytes, "INSTALLED_SIZE")
    require(hmac.compare_digest(digest, expected_sha), "INSTALLED_DIGEST")
    return {"installedSha256": digest, "installedBytes": size}


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="mode", required=True)
    ci = sub.add_parser("installer")
    ci.add_argument("invoked")
    ci.add_argument("pinned")
    ci.add_argument("digest")
    ca = sub.add_parser("installed")
    ca.add_argument("selected")
    ca.add_argument("directory")
    ca.add_argument("member")
    ca.add_argument("digest")
    ca.add_argument("size", type=int)
    a = p.parse_args()
    try:
        if a.mode == "installer":
            r = check_installer(a.invoked, a.pinned, a.digest)
        else:
            r = check_installed(a.selected, a.directory, a.member, a.digest, a.size)
        print(json.dumps(r, sort_keys=True))
    except Rejected as exc:
        print(json.dumps({"rejected": str(exc)}))
        raise SystemExit(1)
    except OSError:
        print(json.dumps({"rejected": "INPUT_IO"}))
        raise SystemExit(1)
