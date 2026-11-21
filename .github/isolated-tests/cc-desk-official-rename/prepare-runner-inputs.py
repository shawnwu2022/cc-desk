"""Offline exact two-member extraction into a newly-created disposable test dir."""
import hashlib
import importlib
from pathlib import Path
import sys
import tarfile

from verify_named_nsis import Rejected
pin = importlib.import_module("check-official-inputs")
INSTALLER, INSTALLER_SHA, PACKAGE_ZIP_SHA = pin.INSTALLER, pin.INSTALLER_SHA, pin.PACKAGE_ZIP_SHA
codex, need, pair, zipped = pin.codex, pin.need, pin.pair, pin.zipped


def extract_exact(package, smoke, vendor, target):
    pair(package, smoke)
    target.mkdir()
    with zipped(package, PACKAGE_ZIP_SHA) as z:
        name = 'test-package/' + INSTALLER
        installer = z.read(name)
        need(len(installer) == 14424921 and hashlib.sha256(installer).hexdigest() == INSTALLER_SHA, 'INSTALLER_PIN')
        with (target / INSTALLER).open('xb') as out:
            out.write(installer)
    codex(vendor, target/'codex.exe')


if __name__ == '__main__':
    try:
        extract_exact(*map(Path, sys.argv[1:]))
    except (OSError, ValueError, Rejected, KeyError, AttributeError, TypeError, tarfile.TarError):
        print('PINNED_INPUT_PREPARATION_BLOCKED')
        sys.exit(1)
    print('PINNED_INPUTS_PREPARED')
