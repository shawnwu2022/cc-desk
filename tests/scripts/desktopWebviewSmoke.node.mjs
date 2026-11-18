import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const read = p => readFileSync(p, 'utf8')
test('DesktopSmoke_UsesInstalledOrdinaryProcessAndNativeObservation_001', () => {
 const s=read('scripts/test-desktop-webview.ps1')
 for(const token of ['github-hosted','UserInteractive','SessionId','ProcessIdProperty','WindowPattern','ToggleState]::Off','CopyFromScreen','WaitForExit','HasExited']) assert.ok(s.includes(token), token)
 assert.ok(!s.includes('--check-conpty'))
 assert.ok(!s.includes('Invoke-WebRequest'))
 assert.ok(!s.includes('__TAURI'))
})
test('DesktopSmoke_RunsBeforeDestructiveProbesAndPreservesFailures_002', () => {
 const s=read('scripts/test-conpty-install.ps1')
 assert.ok(s.indexOf('test-desktop-webview.ps1') > s.indexOf('for ($iteration'))
 assert.ok(s.indexOf('test-desktop-webview.ps1') < s.indexOf('$relocated ='))
 const w=read('.github/workflows/conpty-integration.yml')
 assert.match(w,/scripts\/test-desktop-webview\.ps1/)
 assert.match(w,/if: always\(\)[\s\S]*desktop-webview-smoke/)
})


test('DesktopSmoke_GuardsBeforeLaunchAndNeverInvokesAuthorityControls_003', () => {
 const s=read('scripts/test-desktop-webview.ps1')
 for(const guard of ['existing App','existing application data','Installed image must']) assert.ok(s.indexOf(guard)<s.indexOf('$app = Start-Process'))
 assert.deepEqual([...s.matchAll(/^    Invoke-Button (.*)$/gm)].map(m=>m[1]), ["@('Settings', '设置')","@('General', '通用')","@('Terminal', '终端')"])
 assert.match(s,/normalExit = \$false/)
 assert.ok(s.indexOf('$result.normalExit = $true') > s.indexOf('$child.WaitForExit(10000)'))
 assert.match(s,/finally \{\s*\$result \| ConvertTo-Json/)
})

test('DesktopSmoke_NonHostedRefusalPrecedesImageResolution_004', { skip: process.platform !== 'win32' }, async () => {
 const { mkdtempSync, rmSync } = await import('node:fs')
 const { tmpdir } = await import('node:os')
 const { join, resolve } = await import('node:path')
 const { spawnSync } = await import('node:child_process')
 const cwd=mkdtempSync(join(tmpdir(),'cc-webview-refusal-'))
 try {
  const run=spawnSync('pwsh',['-NoProfile','-File',resolve('scripts/test-desktop-webview.ps1'),'-Exe','X:/nonexistent-smoke-image.exe'], {cwd,env:{...process.env,GITHUB_ACTIONS:'false'},encoding:'utf8',timeout:15000})
  assert.equal(run.error, undefined)
  assert.notEqual(run.status,0)
  const report=JSON.parse(readFileSync(join(cwd,'desktop-webview-smoke/result.json'),'utf8').replace(/^\uFEFF/,''))
  assert.equal(report.status,'blocked')
  assert.equal(report.normalExit,false)
  assert.match(report.error,/disposable interactive hosted Windows required/)
  assert.equal(report.pid,undefined)
  assert.equal(report.image,undefined)
 } finally { rmSync(cwd,{recursive:true,force:true}) }
})

test('DesktopSmoke_RejectsReusedChildrenAndExistingWebviewState_005', () => {
 const s=read('scripts/test-desktop-webview.ps1')
 for(const token of ['CreationDate','ExecutablePath','MainModule.FileName','parent.StartTime','process.SessionId','process.Dispose()']) assert.ok(s.includes(token),token)
 assert.ok(s.indexOf("throw 'Unverified child identity")<s.indexOf('$children[$id] = $process'))
 assert.ok(s.indexOf('$children[$id] = $process')<s.indexOf('$owned[$id] = $process'))
 assert.ok(!s.includes('.Kill($true)'))
 for(const token of ['WEBVIEW2_*','LocalApplicationData','io.github.shawnwu2022.ccdesk','existing default WebView data']) assert.ok(s.indexOf(token)<s.indexOf('$app = Start-Process'))
})
