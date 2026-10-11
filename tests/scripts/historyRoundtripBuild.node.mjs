import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, existsSync, mkdtempSync, mkdirSync, realpathSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, sep } from 'node:path'
import { spawnSync } from 'node:child_process'

const read = (path) => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8')
const present = (path) => existsSync(new URL(`../../${path}`, import.meta.url))

function releaseRejectionStep() {
  const workflow = read('.github/workflows/ci.yml').replace(/\r\n/g, '\n')
  const match = workflow.match(/      - name: Require feature release rejection from the policy guard\n        working-directory: src-tauri\n        run: \|\n((?:          .*\n)+)/)
  assert.ok(match, 'actual release rejection workflow step must exist')
  return match[1].replace(/^ {10}/gm, '')
}

// 执行真实 CI 编排，复用前端仍保留两次原生构建、失败传播和原 binding 字节。
test('Roundtrip_DebugFrontendReuse_007', t => {
  const available = spawnSync('pwsh', ['-NoProfile', '-Command', '$PSVersionTable.PSVersion.ToString()'], { encoding: 'utf8' })
  if (available.error?.code === 'ENOENT') {
    t.skip('PowerShell unavailable: executable debug build orchestration remains unrun')
    return
  }
  assert.equal(available.status, 0, available.stderr)
  const workflow = read('.github/workflows/ci.yml').replace(/\r\n/g, '\n')
  const match = workflow.match(/      - name: Build both inert embedded acceptance scenarios without packages\n        run: \|\n((?:          .*\n)+)/)
  assert.ok(match, 'execute the actual debug workflow body')
  const step = match[1].replace(/^ {10}/gm, '')
  assert.equal((step.match(/npm run tauri -- build --debug --features history-roundtrip-acceptance --no-bundle/g) ?? []).length, 2)
  for (const failure of ['none', 'first', 'frontend', 'second', 'clippy']) {
    const temporary = mkdtempSync(join(tmpdir(), 'ccdesk debug builds '))
    try {
      mkdirSync(join(temporary, 'tests/fixtures/version-history-roundtrip'), { recursive: true })
      mkdirSync(join(temporary, 'src-tauri/target/debug'), { recursive: true })
      mkdirSync(join(temporary, 'dist'))
      const target = join(temporary, 'tests/fixtures/version-history-roundtrip/target.json')
      const original = Buffer.from('{ "schema": 1, "enabled": false, "scenario": "success", "fixture": "preserve exact bytes" }\n')
      writeFileSync(target, original)
      writeFileSync(join(temporary, 'dist/index.html'), 'stale frontend must not be reused initially')
      const script = join(temporary, 'run.ps1')
      writeFileSync(script, `$ErrorActionPreference = 'stop'
$script:events = [Collections.Generic.List[object]]::new()
$script:frontends = 0
function npm {
  $arguments = @($args)
  $binding = Get-Content -LiteralPath 'tests/fixtures/version-history-roundtrip/target.json' -Raw | ConvertFrom-Json
  $index = [Array]::IndexOf($arguments, '--config')
  $override = if ($index -ge 0) { Get-Content -LiteralPath $arguments[$index + 1] -Raw | ConvertFrom-Json } else { $null }
  if ($null -eq $override) {
    $script:frontends++
    [IO.File]::WriteAllText((Join-Path (Get-Location).Path 'dist/index.html'), 'fresh debug frontend')
    if ('${failure}' -eq 'frontend') { Remove-Item -LiteralPath 'dist/index.html' }
  } else {
    if (@($override.PSObject.Properties.Name).Count -ne 1 -or @($override.build.PSObject.Properties.Name).Count -ne 1 -or @($override.build.PSObject.Properties.Name)[0] -cne 'beforeBuildCommand' -or $null -ne $override.build.beforeBuildCommand) { throw 'Only the duplicate build hook may be disabled' }
    if ((Get-Content -LiteralPath 'dist/index.html' -Raw) -cne 'fresh debug frontend') { throw 'Not this invocation frontend' }
  }
  $script:events.Add(@{ command='tauri'; arguments=$arguments; scenario=$binding.scenario; enabled=$binding.enabled; reused=($null -ne $override) })
  [IO.File]::WriteAllText((Join-Path (Get-Location).Path 'src-tauri/target/debug/cc-desk.exe'), $binding.scenario)
  $global:LASTEXITCODE = if (('${failure}' -eq 'first' -and $script:events.Count -eq 1) -or ('${failure}' -eq 'second' -and $script:events.Count -eq 2)) { 101 } else { 0 }
}
function cargo {
  $script:events.Add(@{ command='clippy'; arguments=@($args) })
  $global:LASTEXITCODE = if ('${failure}' -eq 'clippy') { 101 } else { 0 }
}
try {
${step}
} finally {
  @{ events=@($script:events.ToArray()); frontends=$script:frontends; overrides=@(Get-ChildItem -LiteralPath 'src-tauri/target' -Filter 'ci-roundtrip-frontend-*.json' | ForEach-Object Name) } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath 'receipt.json'
}
if ((Test-Path -LiteralPath variable:\\LASTEXITCODE)) { exit $LASTEXITCODE }
`)
      const result = spawnSync('pwsh', ['-NoProfile', '-NonInteractive', '-File', script], { cwd: temporary, encoding: 'utf8', timeout: 30_000 })
      assert.ifError(result.error)
      assert.equal(result.status === 0, failure === 'none', result.stdout + result.stderr)
      const receipt = JSON.parse(readFileSync(join(temporary, 'receipt.json'), 'utf8').replace(/^\uFEFF/, ''))
      assert.equal(receipt.frontends, 1, `duplicate frontend in ${failure}`)
      assert.deepEqual(receipt.overrides, [], 'owned override must be cleaned on success and failure')
      assert.deepEqual(readFileSync(target), original, 'restore the exact original disabled binding')
      const builds = receipt.events.filter(event => event.command === 'tauri')
      assert.equal(builds.length, ['first', 'frontend'].includes(failure) ? 1 : 2)
      for (const [index, build] of builds.entries()) {
        // PowerShell 函数会消费单独的 --，上方另核对传给原生命令的实际源码。
        assert.deepEqual(build.arguments.slice(0, 7), ['run', 'tauri', 'build', '--debug', '--features', 'history-roundtrip-acceptance', '--no-bundle'])
        assert.equal(build.scenario, ['success', 'before-installer-resume'][index])
        assert.equal(build.enabled, false)
        assert.equal(build.reused, index === 1)
      }
      const clippy = receipt.events.filter(event => event.command === 'clippy')
      assert.equal(clippy.length, ['none', 'clippy'].includes(failure) ? 1 : 0)
      if (clippy.length) assert.deepEqual(clippy[0].arguments, ['clippy', '--locked', '--all-targets', '--features', 'history-roundtrip-acceptance', '-D', 'warnings'])
    } finally {
      assert.ok(realpathSync(temporary).startsWith(realpathSync(tmpdir()) + sep), 'cleanup stays inside the owned temporary parent')
      rmSync(temporary, { recursive: true, force: true })
    }
  }
})

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
