import { existsSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const root = process.cwd()
const workflowPath = resolve(root, '.github/workflows/conpty-integration.yml')
const installProbePath = resolve(root, 'scripts/test-conpty-install.ps1')

describe('D21 OS bridge installer certification gate', () => {
  it('D21_Installer_StackedPullRequestsTriggerRuntimeCertification_01', () => {
    expect(existsSync(workflowPath)).toBe(true)
    const workflow = readFileSync(workflowPath, 'utf8')
    const pullRequestBlock = workflow.match(/\n  pull_request:\n([\s\S]*?)\n  workflow_dispatch:/)?.[1] ?? ''

    expect(pullRequestBlock).not.toMatch(/branches:\s*\[main\]/)
    expect(pullRequestBlock).toContain('src-tauri/src/conpty_runtime.rs')
    expect(pullRequestBlock).toContain('scripts/*conpty*')
    expect(workflow).toContain('Install, reinstall, relocate and test fail-closed startup')
  })

  it('D21_Installer_ProbeRetainsInstallRelocateAndCorruptionCases_02', () => {
    expect(existsSync(installProbePath)).toBe(true)
    const script = readFileSync(installProbePath, 'utf8')

    expect(script).toContain('install-$iteration')
    expect(script).toContain("'relocated'")
    expect(script).toContain("'missing-host'")
    expect(script).toContain("'corrupt-dll'")
    expect(script).toContain("'missing-dll-cwd-decoy'")
    expect(script).toContain("$data.backend -ne 'bundled'")
    expect(script).toContain('!$data.ptyLifecycle')
  })
})
