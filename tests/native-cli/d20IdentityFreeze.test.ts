import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { afterEach, describe, expect, it } from 'vitest'

const runnerPath = resolve(process.cwd(), 'scripts/native-cli/real-cli-runner.mjs')
const cleanup: string[] = []

async function loadRunner() {
  expect(existsSync(runnerPath)).toBe(true)
  return import(`${pathToFileURL(runnerPath).href}?case=${Date.now()}-${Math.random()}`)
}

function setup(driverSource: string) {
  const testRoot = mkdtempSync(join(tmpdir(), 'cc-desk-d20-identity-'))
  cleanup.push(testRoot)
  const binaryPath = join(testRoot, 'bin', 'codex')
  const driverDirectory = join(testRoot, 'drivers')
  mkdirSync(dirname(binaryPath), { recursive: true })
  mkdirSync(driverDirectory, { recursive: true })
  writeFileSync(binaryPath, 'fixed binary bytes', 'utf8')
  const ccDesk = join(driverDirectory, 'cc-desk.mjs')
  const systemTerminal = join(driverDirectory, 'system-terminal.mjs')
  writeFileSync(ccDesk, driverSource, 'utf8')
  writeFileSync(systemTerminal, driverSource, 'utf8')
  return {
    cli: 'codex',
    authorizedTestAccount: true,
    testRoot,
    binaryPath,
    drivers: { ccDesk, systemTerminal },
    nonce: 'identity-freeze-nonce',
    originalText: 'identity-freeze-nonce\nfixture',
    transformId: 'codex-user-prompt-submit-v1-exact',
    hostEnv: { PATH: process.env.PATH ?? '' },
    testAccountEnv: {},
  }
}

afterEach(() => {
  while (cleanup.length > 0) rmSync(cleanup.pop()!, { recursive: true, force: true })
})

describe('D28 D20 executable identity freeze', () => {
  it('D28_D20_DriverCannotMutateSelectedCliDuringMatrix_01', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const config = setup([
      "import { appendFileSync } from 'node:fs'",
      "const rows = []",
      "for (let index = 0; index < process.argv.length; index += 1) {",
      "  if (process.argv[index].startsWith('--')) rows.push([process.argv[index].slice(2), process.argv[index + 1]])",
      "}",
      "const args = Object.fromEntries(rows)",
      "appendFileSync(args.binary, 'mutated')",
    ].join('\n'))

    expect(executeD20Matrix(prepareD20Matrix(config))).toMatchObject({
      status: 'FAIL',
      reason: 'REAL_CLI_BINARY_CHANGED',
      failedRunId: 'codex-cc-desk-off-identity-freeze-nonce',
    })
  })

  it('D28_D20_DriverCannotSelfModifyDuringMatrix_02', async () => {
    const { executeD20Matrix, prepareD20Matrix } = await loadRunner()
    const config = setup([
      "import { appendFileSync } from 'node:fs'",
      "import { fileURLToPath } from 'node:url'",
      "appendFileSync(fileURLToPath(import.meta.url), '\\n// mutated')",
    ].join('\n'))

    expect(executeD20Matrix(prepareD20Matrix(config))).toMatchObject({
      status: 'FAIL',
      reason: 'REAL_CLI_DRIVER_CHANGED',
      failedRunId: 'codex-cc-desk-off-identity-freeze-nonce',
    })
  })
})
