import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, existsSync } from 'node:fs'

const read = (path) => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8')
const present = (path) => existsSync(new URL(`../../${path}`, import.meta.url))

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

test('both ordinary policy gates remain false and acceptance follows measured selection', () => {
  const source = read('src-tauri/src/version_history/payload_policy.rs')
  assert.match(source, /const SUPPORTED_ROUNDTRIP_ENABLED: bool = false;/)
  assert.match(source, /PayloadAdmission::for_selection\(selection\)\.is_err\(\)[\s\S]*else if !roundtrip_enabled\(selection\)/)
  assert.match(source, /let admission = Self::admit\(package\)\?;\s*if !roundtrip_enabled\(package.selection\(\)\)/)
  assert.match(source, /#\[cfg\(all\(feature = "history-roundtrip-acceptance", not\(test\)\)\)\]/)
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
