"""Reconstruct one fixed-schema receipt, never copy logs, paths, UI tree or inputs."""
import json
import os
from pathlib import Path
import sys

INV_STAGES = ('admission', 'installer-member', 'installer-invocation', 'installed-image', 'isolated-ordinary-ui', 'passed')
INV_FLAGS = ('preexistingWebView', 'proxyEnvironmentAbsent', 'directWinHttp', 'directWinInet', 'cleanTestProfile',
             'installerVerified', 'installedImageVerified', 'installerUnchanged')
GUI_FLAGS = ('preflight','activeRowObserved','activeRowUuidMatched','oldEditorMatched','inputReadbackMatched',
             'refreshedTitleMatched','postRenameUuidMatched','nativeReadbackMatched','nonArchivedNativeRecord',
             'appServerExecutableProven','productObserverArmed','productCodexChildImageObserved','productCodeHomeInputBound')
GUI_STAGES = ('fixture-read', 'initial-settings-observation', 'create-official-codex-profile', 'settings-open', 'settings-launch-configurations', 'codex-config-create-control', 'codex-config-name', 'codex-config-options', 'codex-config-program-mode', 'codex-config-program-readback', 'codex-config-save', 'codex-config-saved', 'project-select', 'refresh-bound-fixture-history', 'verify-profile-project-cwd', 'native-old-name', 'locate-exact-sessions-tree', 'scoped-expand', 'unique-visible-old-row', 'verify-pre-save-uuid', 'invoke-scoped-rename-entry', 'verify-unique-old-editor', 'read-back-edit', 'save-once', 'refresh-exact-row', 'verify-post-save-uuid', 'native-readback', 'rename-proof-complete')


def read(path):
    if not path.is_file() or path.is_symlink() or path.stat().st_size > 8192:
        return {}
    try:
        obj = json.loads(path.read_text(encoding='utf-8-sig'))
        return obj if type(obj) is dict else {}
    except (OSError, UnicodeError, ValueError):
        return {}


def pick(obj, key, allowed):
    v = obj.get(key)
    return v if type(v) is str and v in allowed else None


def count_once(obj, key):
    return 1 if type(obj.get(key)) is int and obj[key] == 1 else 0


def receipt(directory):
    inv=read(directory/'invocation.json')
    raw=read(directory/'result.json')
    details=raw.get('codexHistory')
    ui=details if type(details) is dict else {}
    out={'schema': 1,'source':'04a2f0cedc8e7acc080e8736723c6e8799127f58',
         'officialRun':44842769369,'officialAttempt':1,'officialArtifact':14682820278,
         'testOnly':True,'publishable':False,
         'invocationStage':pick(inv,'stage',INV_STAGES),'guiStage':pick(ui,'stage',GUI_STAGES),
         'installerInvocations':count_once(inv,'installerInvocations'),
         'saveInvocationCount':count_once(ui,'saveInvocationCount'),
         'normalExit':raw.get('normalExit') is True,
         'uiSessionOpenedByHarness':False,'runningCliSessionProven':False,'productChildEnvironmentInspected':False}
    out['admission']={k:inv.get(k) is True for k in INV_FLAGS}
    out['rename']={k:ui.get(k) is True for k in GUI_FLAGS}
    out['passed']=(out['invocationStage']=='passed' and out['guiStage']=='rename-proof-complete' and
                   out['normalExit'] and raw.get('status')=='passed' and out['saveInvocationCount']==1 and
                   out['installerInvocations']==1 and all(out['admission'].values()) and all(out['rename'].values()))
    return out


if __name__ == '__main__':
    source, dest = map(Path, sys.argv[1:])
    if os.path.lexists(dest):
        sys.exit(1)
    outcome=receipt(source)
    with dest.open('x',encoding='utf-8') as f:
        json.dump(outcome,f,sort_keys=True)
    sys.exit(0 if outcome['passed'] else 1)
