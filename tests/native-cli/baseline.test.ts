import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { describe, expect, it } from 'vitest'

const scriptPath = resolve(process.cwd(), 'scripts/native-cli/record-baseline.mjs')

async function loadModule() {
  expect(existsSync(scriptPath), 'record-baseline.mjs must exist').toBe(true)
  return import(`${pathToFileURL(scriptPath).href}?case=${Date.now()}-${Math.random()}`)
}

function runGit(cwd: string, args: string[]) {
  const result = spawnSync('git', args, { cwd, encoding: 'utf8' })
  expect(result.status, `git ${args.join(' ')} failed: ${result.stderr}`).toBe(0)
}

describe('native CLI baseline report contract', () => {
  it('D01_Baseline_ScriptExists_00', () => {
    expect(existsSync(scriptPath), 'record-baseline.mjs must exist').toBe(true)
  })

  it('D01_Baseline_MissingExitCode_01', async () => {
    const { validateBaseline } = await loadModule()
    expect(() => validateBaseline({
      commit: '77707e3b03187aa2ed96f5ab780f62f14c1e4ffc',
      dirty: false,
      lockHashes: {},
      commands: [{
        argv: ['npm', 'run', 'test:ci'],
        status: 'PASS',
        exitCode: null,
        logPath: 'logs/npm-test.log',
      }],
      platform: 'test',
    })).toThrow('MISSING_EXIT_CODE')
  })

  it('D01_Baseline_PassRequiresZeroExit_02', async () => {
    const { validateBaseline } = await loadModule()
    expect(() => validateBaseline({
      commit: '77707e3b03187aa2ed96f5ab780f62f14c1e4ffc',
      dirty: false,
      lockHashes: { 'package-lock.json': 'a'.repeat(64) },
      commands: [{
        argv: ['npm', 'run', 'typecheck'],
        status: 'PASS',
        exitCode: 1,
        logPath: 'logs/typecheck.log',
      }],
      platform: 'test',
    })).toThrow('PASS_REQUIRES_ZERO_EXIT')
  })

  it('D01_Baseline_BlockedAllowsNullExit_03', async () => {
    const { validateBaseline } = await loadModule()
    expect(validateBaseline({
      commit: '77707e3b03187aa2ed96f5ab780f62f14c1e4ffc',
      dirty: true,
      lockHashes: {},
      commands: [{
        argv: ['codex', '--version'],
        status: 'BLOCKED',
        exitCode: null,
        logPath: 'logs/codex-version.log',
      }],
      platform: 'linux-x64',
    })).toBe(true)
  })

  it('D01_Baseline_RejectsAbsoluteLogPath_04', async () => {
    const { validateBaseline } = await loadModule()
    expect(() => validateBaseline({
      commit: '77707e3b03187aa2ed96f5ab780f62f14c1e4ffc',
      dirty: false,
      lockHashes: {},
      commands: [{
        argv: ['npm', 'run', 'build'],
        status: 'FAIL',
        exitCode: 1,
        logPath: '/tmp/build.log',
      }],
      platform: 'test',
    })).toThrow('LOG_PATH_MUST_BE_RELATIVE')
  })

  it('D01_Baseline_RecordWritesValidatedReport_05', async () => {
    const { recordBaseline } = await loadModule()
    const root = await mkdtemp(join(tmpdir(), 'cc-desk-baseline-'))
    const repoRoot = join(root, 'repo')
    const outDir = join(root, 'evidence')

    try {
      await mkdir(join(repoRoot, 'src-tauri'), { recursive: true })
      await writeFile(join(repoRoot, 'package-lock.json'), '{"lockfileVersion":3}\n', 'utf8')
      await writeFile(join(repoRoot, 'src-tauri', 'Cargo.lock'), '# fixture\n', 'utf8')
      runGit(repoRoot, ['init'])
      runGit(repoRoot, ['config', 'user.email', 'fixture@example.invalid'])
      runGit(repoRoot, ['config', 'user.name', 'CC Desk Fixture'])
      runGit(repoRoot, ['add', '.'])
      runGit(repoRoot, ['commit', '-m', 'fixture'])

      const report = await recordBaseline(repoRoot, outDir, {
        commands: [
          { name: 'ok', argv: [process.execPath, '-e', 'process.stdout.write("ok")'] },
          { name: 'fail', argv: [process.execPath, '-e', 'process.exit(3)'] },
        ],
      })

      expect(report.commit).toMatch(/^[0-9a-f]{40}$/)
      expect(report.dirty).toBe(false)
      expect(Object.keys(report.lockHashes).sort()).toEqual([
        'package-lock.json',
        'src-tauri/Cargo.lock',
      ])
      expect(report.commands.map((entry: { status: string; exitCode: number | null }) => [entry.status, entry.exitCode])).toEqual([
        ['PASS', 0],
        ['FAIL', 3],
      ])
      expect(report.commands.every((entry: { logPath: string }) => !entry.logPath.startsWith('/'))).toBe(true)

      const persisted = JSON.parse(await readFile(join(outDir, 'baseline.json'), 'utf8'))
      expect(persisted).toEqual(report)
    } finally {
      await rm(root, { recursive: true, force: true })
    }
  })
})
