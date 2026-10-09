// Source wiring checks supplement Rust behavior tests; they do not execute Windows acceptance.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

const read = path => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8')
const sources = {
  worker: read('src-tauri/src/tests/native_cli_launch_worker.rs'),
  live: read('src-tauri/src/tests/native_cli_launch_live.rs'),
  modules: read('src-tauri/src/tests/mod.rs'),
  ci: read('.github/workflows/ci.yml'),
}

for (const ending of ['\n', '\r\n']) {
  const label = ending === '\n' ? 'LF' : 'CRLF'
  // Exercise both checkout encodings, then normalize before applying source contracts.
  const { worker, live, modules, ci } = Object.fromEntries(Object.entries(sources)
    .map(([name, source]) => [name, source.replace(/\r?\n/g, ending).replace(/\r\n/g, '\n')]))

  test(`launch timeout keeps its deadline and fails with bounded diagnostics (${label})`, () => {
    assert.match(worker, /let deadline = Instant::now\(\) \+ Duration::from_secs\(90\);/)
    const timeout = worker.split('if Instant::now() > deadline {')[1].split('\n            }')[0]
    assert.match(timeout, /child\.kill\(\)[\s\S]*child\.wait\(\)[\s\S]*diagnostics::snapshot\([\s\S]*panic!\(/,
      'the killed worker must still fail, reporting only the bounded fixed-code snapshot')
    assert.doesNotMatch(timeout, /worker\.log|read_to_string|from_utf8_lossy|return Ok|break/)
    assert.match(worker, /assert!\(\s*status\.success\(\)/)
    assert.match(worker, /assert_eq!\(report\["failure"\], Value::Null/)
  })

  test(`launch stages are persisted before initialization, exit, and reader join (${label})`, () => {
    assert.match(worker, /diagnostics\.mark\(Code::InitializeStarted\);\s*bundled_runtime::initialize\(\)\.unwrap\(\);\s*diagnostics\.mark\(Code::InitializeComplete\);/)
    for (const stage of ['AppBuildStarted', 'AppSetupStarted', 'MainInitialized', 'MainPageLoadStarted', 'MainPageLoaded', 'PeerPageLoaded', 'AppReady', 'AppBuilt', 'RunReturnStarted', 'RunReturned', 'ReportWritten']) {
      assert.match(worker, new RegExp(`\\.mark\\(Code::${stage}\\)`), stage)
    }
    assert.match(live, /self\.diagnostics\.failure\(code\);[\s\S]*?app\.exit\(1\);/)
    assert.match(live, /observations\.push\(name\.into\(\)\);\s*self\.diagnostics\.observation\(name\);/)
    assert.match(live, /diagnostics\.mark\(Code::ReaderJoinStarted\);\s*thread\.join\(\)\.map_err\([\s\S]*?\)\?;\s*diagnostics\.mark\(Code::ReaderJoinComplete\);/)
    assert.match(live, /diagnostics\.mark\(Code::RootWaitStarted\);\s*resource\.process\.pty\.wait\(\)\?;\s*diagnostics\.mark\(Code::RootWaitComplete\);/)
    assert.match(live, /service\.registry\(\)\.retire\(&key\)\?;\s*diagnostics\.mark\(Code::RegistryRetired\);\s*drop\(resource\);[^\n]*\n\s*diagnostics\.mark\(Code::ChildCleanupComplete\);/)
    assert.match(live, /fn cleanup\([\s\S]*?diagnostics\.mark\(Code::CleanupStarted\);/)
    assert.match(live, /diagnostics\.mark\(Code::CleanupComplete\);\s*Ok\(\(\)\)/)
    assert.match(worker, /probe\.consumer\.cleanup\(&probe\.service, &probe\.diagnostics\)/)
    assert.match(live, /state\s*\.consumer\s*\.cleanup\(&state\.service, &state\.diagnostics\)/)
    assert.match(worker, /assert!\(cleanup\.is_ok\(\), "cleanup failed"\)/)
  })

  test(`pure diagnostics and source contracts remain in ordinary tests (${label})`, () => {
    assert.match(modules, /\nmod native_cli_launch_diagnostics;\n/)
    assert.doesNotMatch(modules, /#\[cfg\(windows\)\]\s*mod native_cli_launch_diagnostics/)
    assert.match(ci, /name: Run Node policy tests\s+run: [^\n]*tests\/scripts\/nativeLaunchDiagnostics\.node\.mjs/)
  })
}
