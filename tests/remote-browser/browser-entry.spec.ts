import {test,expect,type TestInfo} from '@playwright/test'
import {createServer,type Server} from 'node:http'
import {readFile,writeFile,mkdir} from 'node:fs/promises'
import {resolve,extname,sep,dirname} from 'node:path'
import {entryFixture,type PairingMutation} from './entry-fixture'
test.use({serviceWorkers:'block'})
let server:Server
async function writeEvidence(info:TestInfo,name:string,value:unknown){const file=info.outputPath(name+'.json');await mkdir(dirname(file),{recursive:true});await writeFile(file,JSON.stringify(value,null,2));await info.attach(name,{contentType:'application/json',path:file})}
test.beforeAll(async()=>{const root=resolve('dist');server=createServer(async(req,res)=>{try{const path=resolve(root,'.'+new URL(req.url!,'http://127.0.0.1').pathname);if(!path.startsWith(root+sep))throw Error('path');const bytes=await readFile(path);res.writeHead(200,{'content-type':({'.html':'text/html','.js':'text/javascript','.css':'text/css','.wasm':'application/wasm'} as Record<string,string>)[extname(path)]??'application/octet-stream','Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'});res.end(bytes)}catch{res.writeHead(404);res.end()}});await new Promise<void>(r=>server.listen(1478,'127.0.0.1',r))})
test.afterAll(async()=>{server.closeAllConnections();await new Promise<void>((r,j)=>server.close(e=>e?j(e):r()))})
test('BrowserEntry_ActualEdgeMissingApprovedSourceLocked_001',async({page})=>{let sockets=0;const errors:string[]=[];page.on('websocket',()=>{sockets++});page.on('pageerror',error=>errors.push(error.message));await page.setViewportSize({width:390,height:844});await page.goto('/browser-remote.html');await expect(page.locator('[data-remote-terminal]')).toBeVisible();await expect(page.getByRole('status')).toContainText('Locked');await page.locator('[data-remote-refresh]').click();await expect(page.getByRole('status')).toContainText('Locked');await expect(page.locator('textarea')).toHaveCount(0);expect(sockets).toBe(0);expect(errors).toEqual([]);expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);})
test('BrowserEntry_ActualEdgeOriginalRelayWasmHostSyntheticPresence_002',async({page,context},testInfo)=>{
 const f=await entryFixture(context),errors:string[]=[],consoleErrors:string[]=[];let workers=0
 page.on('pageerror',error=>errors.push(error.message));page.on('worker',()=>workers++)
 page.on('console',message=>{if(message.type()==='error')consoleErrors.push(message.text())})
 try{
  await page.goto('https://desk.invalid/browser-remote.html')
  await expect(page.locator('.remote-hint')).toContainText('approved profile')
  await page.locator('[data-remote-refresh]').click()
  await expect(page.getByRole('button',{name:'Approve TEST_ONLY'})).toBeVisible({timeout:1500})
  await page.getByRole('button',{name:'Approve TEST_ONLY'}).click()
  await expect(page.getByRole('status')).toContainText('Snapshot verified')
  const supplied=await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))
  expect(supplied).toMatchObject({sourceReads:1,providerReads:1,providerArgumentCount:1,providerSignalMatches:true,originalHandles:true,nonExportable:true,presentations:1,assertions:1})
  expect(supplied.signatures).toBeGreaterThanOrEqual(4)
  expect(f.observations()).toEqual({browserConnections:1,artifactRequests:2});expect(workers).toBe(1)
  await expect(page.locator('[role="img"]')).toBeVisible()
  await page.locator('[data-remote-input]').fill('TEST_ONLY_BROWSER_INPUT')
  await page.locator('[data-remote-send]').click()
  await expect(page.locator('[data-remote-input]')).toHaveValue('')
  // Refresh uses the original source and peer, not another supplier read or relay connection.
  await expect(page.locator('[data-remote-refresh]')).toBeEnabled()
  await page.locator('[data-remote-refresh]').click()
  await expect(page.locator('[data-remote-refresh]')).toBeEnabled()
  expect(await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))).toEqual(supplied)
  expect(f.observations()).toEqual({browserConnections:1,artifactRequests:2})
  await page.locator('[data-remote-disconnect]').click()
  await expect(page.getByRole('status')).toContainText('Locked')
  const effect=await f.host.wait(value=>value.kind==='effects')
  expect(effect.writeCount).toBe(1);expect(effect.bytes).toBe(Buffer.from('TEST_ONLY_BROWSER_INPUT').toString('hex'))
  await f.relay.wait(value=>value.originalRelayRetired===true)
  expect(errors).toEqual([]);expect(consoleErrors).toEqual([])
  await writeEvidence(testInfo,'supplier-relay-host-evidence',{artifacts:f.artifacts,supplied,transport:f.observations(),workers,effect,relay:f.relay.output(),host:f.host.output()})
 }catch(error){throw Error(JSON.stringify({error:String(error),errors,consoleErrors,text:await page.locator('body').innerText(),relay:f.relay.output(),host:f.host.output()}))}
 finally{await f.dispose()}
})

for(const mutation of ['session','run','generation','hostSpki'] as const satisfies readonly PairingMutation[]){
 test(`BrowserEntry_SupplierMismatchBeforeSigningOrRelay_${mutation}_003`,async({page,context},testInfo)=>{
  const f=await entryFixture(context,mutation),errors:string[]=[];let workers=0,sockets=0
  page.on('pageerror',error=>errors.push(error.message));page.on('worker',()=>workers++);page.on('websocket',()=>sockets++)
  try{
   await page.goto('https://desk.invalid/browser-remote.html')
   for(let attempt=0;attempt<2;attempt++){
    await page.locator('[data-remote-refresh]').click()
    await expect(page.getByRole('status')).toContainText('Remote connection unavailable')
    await expect(page.locator('[data-remote-refresh]')).toBeEnabled()
    await expect(page.locator('[data-remote-input]')).toHaveCount(0)
    await expect(page.getByRole('button',{name:'Approve TEST_ONLY'})).toHaveCount(0)
   }
   const supplied=await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))
   expect(supplied).toEqual({sourceReads:1,providerReads:1,providerArgumentCount:1,providerSignalMatches:true,originalHandles:false,nonExportable:false,signatures:0,presentations:0,assertions:0})
   expect(f.observations()).toEqual({browserConnections:0,artifactRequests:0})
   expect(workers).toBe(0);expect(sockets).toBe(0);expect(errors).toEqual([])
   expect(f.host.output()).not.toContain('"kind":"installed"')
   expect(f.host.output()).not.toContain('"kind":"dispatched"')
   await writeEvidence(testInfo,'supplier-locked-evidence',{artifacts:f.artifacts,mutation,supplied,transport:f.observations(),workers,sockets})
  }finally{await f.dispose()}
 })
}

test('BrowserEntry_EnrollmentRevokesActiveSourceAndCachedReopen_004',async({page,context},info)=>{
 const f=await entryFixture(context)
 try{
  await page.goto('https://desk.invalid/browser-remote.html')
  await page.locator('[data-remote-refresh]').click()
  await page.getByRole('button',{name:'Approve TEST_ONLY'}).click()
  await expect(page.getByRole('status')).toContainText('Snapshot verified')
  const before=await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))
  await page.evaluate(async()=>{const owner=Reflect.get(globalThis,'__ccDeskTestOnlyEnrollment');owner.revoke();await owner.waitClosed()})
  await page.locator('[data-remote-refresh]').click()
  await expect(page.getByRole('status')).toContainText('Remote connection unavailable')
  await expect(page.locator('[data-remote-input]')).toHaveCount(0)
  expect(await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))).toEqual(before)
  expect(f.observations()).toEqual({browserConnections:1,artifactRequests:2})
  const effect=await f.host.wait(value=>value.kind==='effects')
  expect(effect.writeCount).toBe(0)
  await f.relay.wait(value=>value.originalRelayRetired===true)
  await writeEvidence(info,'enrollment-active-revocation',{artifacts:f.artifacts,before,transport:f.observations(),effect})
 }finally{await f.dispose()}
})

test('BrowserEntry_EnrollmentRevocationRetainsPendingPresenter_005',async({page,context},info)=>{
 await context.addInitScript(()=>Reflect.set(globalThis,'__ccDeskTestOnlyHoldPresentation',true))
 const f=await entryFixture(context)
 try{
  await page.goto('https://desk.invalid/browser-remote.html')
  await page.locator('[data-remote-refresh]').click()
  await expect(page.getByRole('button',{name:'Approve TEST_ONLY'})).toBeVisible()
  const before=await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))
  const pending=await page.evaluate(async()=>{
   const owner=Reflect.get(globalThis,'__ccDeskTestOnlyEnrollment');let settled=false
   owner.revoke();void owner.waitClosed().then(()=>{settled=true},()=>{settled=true})
   await new Promise(resolve=>setTimeout(resolve,20));return !settled
  })
  expect(pending).toBe(true)
  await page.evaluate(async()=>{Reflect.get(globalThis,'__ccDeskTestOnlyReleasePresentation')();await Reflect.get(globalThis,'__ccDeskTestOnlyEnrollment').waitClosed()})
  await expect(page.getByRole('status')).toContainText('Remote connection unavailable')
  await expect(page.getByRole('button',{name:'Approve TEST_ONLY'})).toHaveCount(0)
  await page.locator('[data-remote-refresh]').click()
  await expect(page.locator('[data-remote-refresh]')).toBeEnabled()
  const after=await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))
  expect(after).toEqual(before);expect(after.assertions).toBe(0)
  expect(f.observations().browserConnections).toBe(1)
  expect(f.host.output()).not.toContain('"kind":"installed"')
  await writeEvidence(info,'enrollment-pending-revocation',{artifacts:f.artifacts,before,after,pending,transport:f.observations()})
 }finally{await f.dispose()}
})

test('BrowserEntry_EnrollmentRevokesDuringRetentionBeforeSideEffects_006',async({page,context},info)=>{
 await context.addInitScript(()=>Reflect.set(globalThis,'__ccDeskTestOnlyRevokeOnRetain',true))
 const f=await entryFixture(context);let workers=0,sockets=0
 page.on('worker',()=>workers++);page.on('websocket',()=>sockets++)
 try{
  await page.goto('https://desk.invalid/browser-remote.html')
  await page.locator('[data-remote-refresh]').click()
  await expect(page.getByRole('status')).toContainText('Remote connection unavailable')
  await page.evaluate(async()=>{await Reflect.get(globalThis,'__ccDeskTestOnlyEnrollment').waitClosed()})
  await page.locator('[data-remote-refresh]').click()
  await expect(page.locator('[data-remote-refresh]')).toBeEnabled()
  const supplied=await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))
  expect(supplied).toMatchObject({sourceReads:1,providerReads:1,signatures:0,presentations:0,assertions:0})
  expect(f.observations()).toEqual({browserConnections:0,artifactRequests:0})
  expect(workers).toBe(0);expect(sockets).toBe(0)
  await expect(page.locator('[data-remote-input]')).toHaveCount(0)
  await writeEvidence(info,'enrollment-admission-revocation',{artifacts:f.artifacts,supplied,transport:f.observations(),workers,sockets})
 }finally{await f.dispose()}
})

test('BrowserEntry_ProductionPresenterApprovesOriginalGrant_007',async({page,context},info)=>{
 const f=await entryFixture(context,undefined,true)
 try{
  await page.goto('https://desk.invalid/browser-remote.html');await page.locator('[data-remote-refresh]').click()
  const dialog=page.getByRole('dialog',{name:'Approve remote session'});await expect(dialog).toBeVisible()
  const shown=JSON.parse(await dialog.locator('pre').innerText()),ready=await f.host.wait(value=>value.kind==='ready')
  expect(shown.scope).toEqual(ready.scope);expect(shown.role).toBe('Controller');expect(shown.principalId).toBe('04'.repeat(32))
  const before=await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))
  expect(before.assertions).toBe(0)
  await page.getByRole('button',{name:'Approve this connection',exact:true}).evaluate((button:HTMLButtonElement)=>button.click())
  expect(await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))).toEqual(before)
  await page.getByRole('button',{name:'Approve this connection',exact:true}).click()
  await expect(page.getByRole('status')).toContainText('Snapshot verified');await expect(dialog).toHaveCount(0)
  await page.locator('[data-remote-input]').fill('TEST_ONLY_PRODUCTION_PRESENTER');await page.locator('[data-remote-send]').click();await expect(page.locator('[data-remote-input]')).toHaveValue('')
  await page.locator('[data-remote-disconnect]').click();await expect(page.getByRole('status')).toContainText('Locked')
  const effect=await f.host.wait(value=>value.kind==='effects');expect(effect.writeCount).toBe(1);expect(effect.bytes).toBe(Buffer.from('TEST_ONLY_PRODUCTION_PRESENTER').toString('hex'))
  await f.relay.wait(value=>value.originalRelayRetired===true)
  await writeEvidence(info,'production-presenter-approved',{artifacts:f.artifacts,shown,before,after:await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing')),effect})
 }finally{await f.dispose()}
})

for(const action of ['cancel','revoke','deadline'] as const){
 test(`BrowserEntry_ProductionPresenterRefusalBlocksGrant_${action}_008`,async({page,context},info)=>{
  const f=await entryFixture(context,undefined,true)
  try{
   await page.goto('https://desk.invalid/browser-remote.html');await page.locator('[data-remote-refresh]').click()
   await expect(page.getByRole('dialog',{name:'Approve remote session'})).toBeVisible()
   const before=await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))
   expect(before.assertions).toBe(0)
   if(action==='cancel')await page.getByRole('button',{name:'Cancel',exact:true}).click()
   else if(action==='revoke')await page.evaluate(async()=>{const owner=Reflect.get(globalThis,'__ccDeskTestOnlyEnrollment');owner.revoke();await owner.waitClosed()})
   await expect(page.getByRole('status')).toContainText('Remote connection unavailable');await expect(page.getByRole('dialog')).toHaveCount(0)
   const after=await page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'));expect(after).toEqual(before)
   expect(f.host.output()).not.toContain('"kind":"installed"');expect(f.host.output()).not.toContain('"kind":"dispatched"')
   await expect(page.locator('[data-remote-input]')).toHaveCount(0)
   await writeEvidence(info,'production-presenter-refused',{artifacts:f.artifacts,action,before,after,host:f.host.output()})
  }finally{await f.dispose()}
 })
}
