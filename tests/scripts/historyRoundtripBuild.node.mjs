import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, existsSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

const read = (path) => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8')
const present = (path) => existsSync(new URL(`../../${path}`, import.meta.url))

function releaseRejectionStep() {
  const workflow = read('.github/workflows/ci.yml').replace(/\r\n/g, '\n')
  const match = workflow.match(/      - name: Require feature release rejection from the policy guard\n        working-directory: src-tauri\n        run: \|\n((?:          .*\n)+)/)
  assert.ok(match, 'actual release rejection workflow step must exist')
  return match[1].replace(/^ {10}/gm, '')
}

test('release rejection exits successfully only after both denial assertions', () => {
  const step = releaseRejectionStep()
  assert.match(step, /if \(\$result -eq 0\) \{ throw 'Acceptance feature entered a release build' \}/)
  assert.match(step, /if \(\(\$output -join "`n"\) -notmatch 'roundtrip acceptance is forbidden in release builds'\) \{\n  throw 'Release failed for an unrelated reason; exclusion remains unverified'\n\}/)
  assert.match(step, /\nexit 0\n$/, 'acknowledge only the verified expected native-command failure')
  assert.equal((step.match(/\bexit 0\b/g) ?? []).length, 1)
})

test('actual release rejection step preserves GitHub pwsh wrapper outcomes', t => {
  const available = spawnSync('pwsh', ['-NoProfile', '-Command', '$PSVersionTable.PSVersion.ToString()'], { encoding: 'utf8' })
  if (available.error?.code === 'ENOENT') {
    t.skip('PowerShell unavailable: executable GitHub wrapper contract remains unrun')
    return
  }
  assert.equal(available.status, 0, `PowerShell version probe failed: ${available.stderr}`)
  const step = releaseRejectionStep()
  const cargo = 'cargo build --locked --release --features history-roundtrip-acceptance'
  assert.equal(step.split(cargo).length, 2, 'replace only the actual native build invocation')
  const temporary = mkdtempSync(join(tmpdir(), 'ccdesk-release-policy-'))
  const quote = value => `'${value.replaceAll("'", "''")}'`
  try {
    const fixture = join(temporary, 'native-build.cjs')
    writeFileSync(fixture, "process.stdout.write(process.argv[3] + '\\n'); process.exit(Number(process.argv[2]));\n")
    const run = (body, code, output) => {
      const script = join(temporary, 'step.ps1')
      const command = `${quote(process.execPath)} ${quote(fixture)} ${code} ${quote(output)}`
      // Match the runner's prefix, suffix and dot-source invocation, not pwsh -File semantics.
      // https://github.com/actions/runner/blob/main/src/Runner.Worker/Handlers/ScriptHandlerHelpers.cs
      const wrapped = "$ErrorActionPreference = 'stop'\n" + body.replace(cargo, command) +
        '\nif ((Test-Path -LiteralPath variable:\\LASTEXITCODE)) { exit $LASTEXITCODE }\n'
      writeFileSync(script, wrapped)
      const result = spawnSync('pwsh', ['-NoProfile', '-NonInteractive', '-Command', `. ${quote(script)}`], { encoding: 'utf8', timeout: 30_000 })
      assert.ifError(result.error)
      assert.notEqual(result.status, null, 'wrapper must terminate with an exit status')
      return { status: result.status, output: result.stdout + result.stderr }
    }
    const marker = 'roundtrip acceptance is forbidden in release builds'
    const denied = run(step, 101, marker)
    assert.equal(denied.status, 0, denied.output)
    const succeeded = run(step, 0, marker)
    assert.notEqual(succeeded.status, 0, 'an unexpected successful build must fail the step')
    assert.match(succeeded.output, /Acceptance feature entered a release build/)
    const unrelated = run(step, 101, 'unrelated build fixture failure')
    assert.notEqual(unrelated.status, 0, 'an unrelated build failure must fail the step')
    assert.match(unrelated.output, /Release failed for an unrelated reason/)
    const missingAcknowledgement = run(step.replace(/\nexit 0\n$/, '\n'), 101, marker)
    // pwsh -Command 会将被调用脚本的非 0/1 退出码映射为 1。
    assert.equal(missingAcknowledgement.status, 1, 'the runner suffix must expose an unacknowledged native failure')
  } finally {
    rmSync(temporary, { recursive: true, force: true })
  }
})

test('acceptance feature is opt-in, debug-only, Windows x64 and excluded from unit-test authority', () => {
  const cargo = read('src-tauri/Cargo.toml')
  assert.match(cargo, /^history-roundtrip-acceptance = \[\]$/m)
  assert.match(cargo, /^default = \["sqlite"\]$/m)
  const module = read('src-tauri/src/version_history/mod.rs')
  assert.match(module, /feature = "history-roundtrip-acceptance"/)
  assert.match(module, /not\(debug_assertions\)/)
  assert.match(module, /compile_error!/)
  assert.match(read('src-tauri/build.rs'), /CARGO_FEATURE_HISTORY_ROUNDTRIP_ACCEPTANCE/)
  assert.match(read('src-tauri/build.rs'), /PROFILE/)
})

test('compiled binding is deny-only; no runtime switch can select it', () => {
  assert.ok(present('tests/fixtures/version-history-roundtrip/target.json'), 'deny-only compiled binding must exist')
  const target = JSON.parse(read('tests/fixtures/version-history-roundtrip/target.json'))
  assert.equal(target.enabled, false)
  assert.equal(target.schema, 1)
  const source = read('src-tauri/src/version_history/acceptance.rs')
  assert.match(source, /include_str!\(/)
  assert.doesNotMatch(source, /std::env::(?:var|var_os|args)/)
  assert.match(source, /RegisteredInstallation::capture\(\)/)
  assert.match(source, /FOLDERID_Profile/)
  assert.match(source, /require_job_free_source\(\)/)
})

test('ordinary capability requires reviewed payloads while acceptance retains its separate binding', () => {
  const source = read('src-tauri/src/version_history/payload_policy.rs')
  assert.doesNotMatch(source, /const SUPPORTED_ROUNDTRIP_ENABLED: bool = (?:false|true);/)
  assert.match(source, /PayloadAdmission::for_selection\(selection\)\.is_err\(\)[\s\S]*else if !roundtrip_enabled\(selection\)/)
  assert.match(source, /let admission = Self::admit\(package\)\?;\s*if !roundtrip_enabled\(package.selection\(\)\)/)
  assert.match(source, /#\[cfg\(all\(feature = "history-roundtrip-acceptance", not\(test\)\)\)\]/)
  assert.match(source, /super::acceptance::require_source_target\(selection\)\.is_ok\(\)/)
  assert.match(source, /#\[cfg\(not\(all\(feature = "history-roundtrip-acceptance", not\(test\)\)\)\)\]/)
  assert.match(source, /HostPlatform::WindowsX64/)
  assert.match(source, /HostPlatform::current\(\)/)
  const admission = read('src-tauri/src/version_history/windows/source_begin.rs')
  const firstJobCheck = admission.indexOf('super::manager_process::require_job_free_source()')
  const payloadCheck = admission.indexOf('PayloadAdmission::admit_begin(package)?')
  const sessionCheck = admission.indexOf('process_admissions().freeze(transaction)?')
  assert.ok(firstJobCheck >= 0 && payloadCheck > firstJobCheck && sessionCheck > payloadCheck,
    'production confirmation still checks foreign Jobs, exact payload and live session ownership')
  assert.match(read('src-tauri/src/version_history/download.rs'), /package\.revalidate\(&check\)\?[\s\S]*let permit = admit\(&package, &switch_id\)/)
})

test('passive evidence cannot propagate errors and injection precedes every installer resume intent', () => {
  const source = read('src-tauri/src/version_history/windows/coordinator.rs')
  const install = source.slice(source.indexOf('fn install('), source.indexOf('fn launch_and_wait_for_return('))
  const persisted = install.indexOf('receipt = Some(process.persist_identity')
  const applied = install.indexOf('record_applied(', persisted)
  const injection = install.indexOf('before_installer_resume()?')
  const resume = install.indexOf('EffectKind::InstallerResume')
  assert.ok(persisted >= 0 && applied > persisted && injection > applied && resume > injection)
  assert.match(read('src-tauri/src/version_history/acceptance.rs'), /catch_unwind/)
  assert.match(read('src-tauri/src/version_history/acceptance.rs'), /pub\(crate\) fn observe<T: Serialize>\([\s\S]*?collect: impl FnOnce\(\) -> Result<T, SafeError>,\s*\) \{/)
  assert.match(read('src-tauri/src/version_history/windows/source_begin.rs'), /evidence_observation/)
  assert.match(source, /check_evidence_scope\(Some\(&self\.exclusions\)\)/)
  assert.match(source, /return_attempt[\s\S]*?map\(\|attempt\| attempt\.acceptance_exclusions\(\)\)/)
  const sink = read('src-tauri/src/version_history/acceptance.rs')
  assert.match(sink, /SINK_FAILED\.store\(true, Ordering::Release\)/)
  assert.match(sink, /\.acceptance_require_disjoint\(&evidence\)/)
  assert.ok(source.indexOf('acceptance::check_evidence_scope(Some(&self.exclusions))') < source.indexOf('AcceptanceStage::M0'))
  const returning = source.slice(source.indexOf('fn return_previous('))
  assert.ok(returning.indexOf('acceptance::check_evidence_scope(') < returning.indexOf('name("later-context")'))
})
