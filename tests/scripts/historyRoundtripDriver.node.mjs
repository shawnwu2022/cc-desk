import assert from 'node:assert/strict'
import { readFileSync, existsSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import test from 'node:test'
import { spawnSync } from 'node:child_process'

const root = fileURLToPath(new URL('../../', import.meta.url))
const read = name => existsSync(root + name) ? readFileSync(root + name, 'utf8') : ''
const driver = read('scripts/version-history/roundtrip.ps1')
const ui = read('scripts/version-history/roundtrip-ui.ps1')

// 验收驱动缺少真实目标、控件或记录时必须阻止动作，不能生成成功证据。
test('Driver_DenyMissingAuthority_001', () => {
  for (const code of ['BLOCKED_EXTERNAL_TARGET', 'TARGET_MISMATCH', 'SCENARIO_MISMATCH', 'IMAGE_HASH_MISMATCH', 'BLOCKED_UI_SELECTOR', 'DUPLICATE_PROCESS', 'MISSING_REPORT', 'WAIT_TIMEOUT']) {
    assert.ok(driver.includes(code) || ui.includes(code), `driver must fail closed with ${code}`)
  }
  assert.match(driver, /if\s*\(\$DryRun\)/, 'dry run must precede native execution')
  assert.ok(driver.indexOf('if ($DryRun)') < driver.indexOf('Initialize-RoundtripNative'), 'dry run must not load or execute native driver')
})

// 关闭最终管理器并检查真实退出之后才能独立检查，再正常打开恢复的应用。
test('Driver_ExitRecaptureReopen_002', () => {
  const main = driver.slice(driver.indexOf('# Actual UI scenario'))
  const ordered = ['Wait-RoundtripReport -Stage FinalRestored', 'Close-RoundtripWindow -Process $manager', 'Wait-RoundtripProcessExit -Process $manager', 'Get-RoundtripIndependentCapture', 'Assert-RoundtripRestoredCapture', 'Start-RoundtripRestoredApp']
  let previous = -1
  for (const step of ordered) {
    const index = main.indexOf(step)
    assert.ok(index > previous, `${step} must follow the preceding acceptance barrier`)
    previous = index
  }
  assert.ok(main.includes('restoredAppReopened = $true'), 'PASS must require an actual reopened app')
})

// 所有修改动作来自 UIA 控件，不能通过 IPC、CDP、网页脚本或强制终止。
test('Driver_UseNativeControlsOnly_003', () => {
  assert.match(ui, /System\.Windows\.Automation/, 'actual Windows UI Automation required')
  assert.match(ui, /InvokePattern/, 'UI mutation must invoke accessible native control')
  assert.match(ui, /WindowPattern/, 'normal window close must use UIA WindowPattern')
  for (const forbidden of [/Stop-Process/i, /taskkill/i, /SendMessage\s*\(/, /remote-debugging-port/i, /Invoke-WebRequest/i, /Invoke-RestMethod/i, /ExecuteScript/i, /__TAURI/i, /--version-manager\s+\$/]) {
    assert.doesNotMatch(driver + ui, forbidden, `driver must not use ${forbidden}`)
  }
})

// 两个场景要求不同的原生记录，预恢复故障不被冒充为 NSIS 执行失败。
test('Driver_SeparateScenarioProof_004', () => {
  assert.match(driver, /before-installer-resume/, 'explicit injected failure scenario required')
  for (const stage of ['InstallerSuspended', 'InjectedPreResumeFailure', 'CancelledBeforeResume', 'HistoricalLaunched', 'LaterCaptured']) {
    assert.ok(driver.includes(stage), `${stage} must be required by the applicable scenario`)
  }
  assert.match(driver, /resumeAttempted\s*-ne\s*\$false/, 'failure must reject an installer resume attempt')
})

// 本机重新检查必须保持打开的对象，拒绝链接、命名流和超界，不只比较文件列表。
test('Driver_BoundedNativeCapture_005', () => {
  for (const api of ['CreateFileW', 'GetFileInformationByHandleEx', 'GetFinalPathNameByHandleW', 'GetSecurityInfo', 'RegOpenKeyExW', 'RegEnumValueW', 'RegGetKeySecurity', 'NtQueryKey', 'IsProcessInJob', 'GetTokenInformation']) {
    assert.ok(ui.includes(api), `${api} required for independent native state`)
  }
  assert.match(ui, /FILE_FLAG_OPEN_REPARSE_POINT/, 'never follow reparse points')
  assert.match(ui, /FILE_SHARE_READ/, 'capture must deny concurrent writes and deletes')
  assert.match(ui, /FileStreamInfo/, 'named NTFS streams must be rejected')
  assert.match(ui, /MaxEntries/, 'file capture needs an entry bound')
  assert.match(ui, /MaxBytes/, 'file capture needs a byte bound')
  assert.match(ui, /MaxDepth/, 'file capture needs a depth bound')
})

// 恢复比较需覆盖整个安装包、原始上下文、六个注册槽、Run 以及两个快捷方式。
test('Driver_CompleteTypedComparison_006', () => {
  for (const item of ['logicalDigest', 'object_identity', 'location_identity', 'descriptor', 'attributes', 'Uninstall', 'Publisher', 'DeskDirectory', 'DeskDirectoryBackground', 'LegacyDirectory', 'LegacyDirectoryBackground', 'OwnedRun', 'Desktop', 'StartMenu']) {
    assert.ok((driver + ui).includes(item), `${item} must be independently captured or compared`)
  }
  assert.match(driver, /Assert-RoundtripDescriptor/, 'restoration-only permission equivalence must be explicit')
  assert.match(driver, /Assert-RoundtripRetained/, 'retained later contents must be asserted')
})

test('Driver_RecordWriterSupportedDepth_008', () => {
  const writer = driver.slice(driver.indexOf('function Write-RoundtripRecord'), driver.indexOf('function Assert-RoundtripProcess'))
  assert.match(writer, /ConvertTo-Json -Depth 100 -Compress/)
  assert.doesNotMatch(writer, /ConvertTo-Json -Depth 128/)
  const contracts = read('tests/scripts/historyRoundtripDriver.ps1')
  assert.match(contracts, /Write-RoundtripRecord -Name 'actual-writer' -Value/)
})

test('Driver_SentinelsUseHeldCanonicalRoots_009', () => {
  assert.match(driver, /Assert-RoundtripSentinelKinds \$Manifest.sharedSentinels/)
  assert.match(driver, /\$M0.value.dataRoot/)
  assert.match(driver, /\$M0.value.controlDirectory/)
  assert.match(ui, /RequireSentinelDisjoint\(file.Path, canonical.ToArray\(\)\)/)
  const writer = ui.slice(ui.indexOf('public static void WriteSyntheticSentinel'), ui.indexOf('static ushort[] ToUnits'))
  assert.ok(writer.indexOf('RequireSentinelDisjoint') < writer.indexOf('stream.SetLength(0)'))
})

// Source guard only; the PowerShell suite below exercises actual key cardinalities and mismatches.
test('Driver_ObjectKeyResultsStayArrays_010', () => {
  const equal = driver.slice(driver.indexOf('function Assert-RoundtripEqual'), driver.indexOf('function Assert-RoundtripDescriptor'))
  assert.match(equal, /\$keys = @\(if /, 'collect the conditional output, including empty and single-key objects')
  assert.match(equal, /\$actualKeys = @\(if /, 'actual object keys need the same array shape')
  assert.doesNotMatch(equal, /PSObject\.Properties\.Name/, 'empty objects must not use StrictMode member-access enumeration')
})

// Windows CI executes the production PowerShell validators against mutations, without native effects.
test('Driver_ExecutableDryRunContracts_007', t => {
  const available = spawnSync('pwsh', ['-NoProfile', '-Command', '$PSVersionTable.PSVersion.ToString()'], { encoding: 'utf8' })
  if (available.error?.code === 'ENOENT') {
    t.skip('PowerShell unavailable: source checks only; no native/dry-run execution claim')
    return
  }
  assert.equal(available.status, 0, `PowerShell version probe failed: ${available.stderr}`)
  const result = spawnSync('pwsh', ['-NoProfile', '-File', root + 'tests/scripts/historyRoundtripDriver.ps1'], { encoding: 'utf8', timeout: 120_000 })
  assert.equal(result.status, 0, `PowerShell dry-run/report contracts failed:\n${result.stdout}\n${result.stderr}`)
  assert.match(result.stdout, /POWERSHELL_CONTRACT_PASS checks=\d+ nativeExecution=false roundtripAccepted=false/)
})
