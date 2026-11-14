import {test,expect,type Page,type BrowserContext,type TestInfo} from '@playwright/test'
import {writeFile} from 'node:fs/promises'
import {entryFixture,type RunHandoffFixture} from './entry-fixture'

test.use({serviceWorkers:'block'})
test.beforeEach(async({context})=>{
 // 只旁观原私有端口的历史 descriptor，不替换消息、Worker 或原退役协议。
 await context.addInitScript(()=>{
  const original=MessagePort.prototype.start,seen=new WeakSet<MessagePort>()
  Object.defineProperty(MessagePort.prototype,'start',{value:function(this:MessagePort){if(!seen.has(this)){seen.add(this);this.addEventListener('message',event=>{if(event.data?.kind==='descriptor')Reflect.set(globalThis,'__ccDeskTestOnlyRunDescriptor',event.data.value)})}return original.call(this)}})
 })
})
async function installAndReload(page:Page,context:BrowserContext){
 await page.goto('https://desk.invalid/browser-remote.html')
 expect(await page.evaluate(async()=>{const installer=await import(location.origin+'/test-only-enrollment-install.mjs');return installer.install()})).toEqual({revision:'1',state:'active'})
 await page.close();const active=await context.newPage();await active.goto('https://desk.invalid/browser-remote.html');return active
}
async function storedConfiguration(page:Page){return page.evaluate(async()=>{
 const db=await new Promise<IDBDatabase>((r,j)=>{const q=indexedDB.open('CC_DESK_TEST_ONLY_ENTRY',1);q.onsuccess=()=>r(q.result);q.onerror=()=>j(q.error)})
 try{return await new Promise<any>((r,j)=>{const tx=db.transaction('enrollment','readonly'),q=tx.objectStore('enrollment').get('current');let record:any;q.onsuccess=()=>{record=q.result};tx.oncomplete=()=>r({revision:record.revision,state:record.state,configuration:JSON.parse(new TextDecoder().decode(record.configuration))});tx.onabort=()=>j(tx.error)})}finally{db.close()}
 })}
async function closeSession(page:Page){await page.evaluate(async()=>{const owner=Reflect.get(globalThis,'__ccDeskTestOnlyStore')?.session;if(owner){owner.close();await owner.waitClosed()}})}
async function evidence(info:TestInfo,value:unknown){const path=info.outputPath('authenticated-run-handoff.json');await writeFile(path,JSON.stringify(value,null,2));await info.attach('authenticated-run-handoff',{path,contentType:'application/json'})}

test('RunHandoff_ActualRelayWasmNewScopeNeedsFreshPresenceAndData_001',async({page,context},info)=>{
 const f=await entryFixture(context,undefined,true,true,'resume');let active:Page|undefined
 try{
  active=await installAndReload(page,context)
  expect(f.scope).not.toEqual(f.storedScope);expect(f.observations()).toEqual({browserConnections:0,artifactRequests:0})
  const beforeStored=await storedConfiguration(active);expect(beforeStored.configuration.grant.scope).toEqual(f.storedScope)
  await active.locator('[data-remote-refresh]').click();const dialog=active.getByRole('dialog',{name:'Approve remote session'});await expect(dialog).toBeVisible()
  const shown=JSON.parse(await dialog.locator('pre').innerText());expect(shown.scope).toEqual(f.scope);expect(shown.scope).not.toEqual(f.storedScope)
  const before=await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'));expect(before).toMatchObject({sourceReads:1,storeReads:1,nonExportable:true,presentations:1,assertions:0,grantSignatures:0,keyImports:0})
  await active.getByRole('button',{name:'Approve this connection',exact:true}).evaluate((button:HTMLButtonElement)=>button.click());expect(await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))).toEqual(before)
  await active.getByRole('button',{name:'Approve this connection',exact:true}).click();await expect(active.getByRole('status')).toContainText('Snapshot verified')
  const supplied=await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'));expect(supplied).toMatchObject({sourceReads:1,storeReads:1,presentations:1,assertions:1,grantSignatures:1,keyImports:1})
  const descriptor=await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyRunDescriptor'));expect(descriptor.target).toMatchObject(f.scope)
  await active.locator('[data-remote-input]').fill('TEST_ONLY_AUTHENTICATED_NEW_RUN');await active.locator('[data-remote-send]').click();await expect(active.locator('[data-remote-input]')).toHaveValue('')
  await active.locator('[data-remote-disconnect]').click();await expect(active.getByRole('status')).toContainText('Locked');await closeSession(active)
  const afterStored=await storedConfiguration(active);expect(afterStored).toEqual(beforeStored)
  const effect=await f.host.wait(value=>value.kind==='effects');expect(effect.writeCount).toBe(1);expect(effect.bytes).toBe(Buffer.from('TEST_ONLY_AUTHENTICATED_NEW_RUN').toString('hex'));await f.relay.wait(value=>value.originalRelayRetired===true)
  await active.locator('[data-remote-refresh]').click();await expect(active.getByRole('status')).toContainText('Remote connection unavailable');expect(f.observations()).toEqual({browserConnections:1,artifactRequests:2});expect(await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))).toEqual(supplied)
  await evidence(info,{boundary:'real Edge/IDB, original Relay, actual WASM and original memory Host; synthetic WebAuthn and Host approval, not native restart or deployment',artifacts:f.artifacts,shown,descriptor,before,supplied,beforeStored,afterStored,effect,transport:f.observations()})
 }finally{if(active)await closeSession(active).catch(()=>{});await f.dispose()}
})

for(const mode of ['exact-old','wrong-pin','wrong-principal','wrong-epoch','wrong-role'] as const satisfies readonly RunHandoffFixture[])test(`RunHandoff_ActualRelayWasmRejectsBeforePresentation_${mode}_002`,async({page,context},info)=>{
 const f=await entryFixture(context,undefined,true,true,mode);let active:Page|undefined
 try{
  active=await installAndReload(page,context);const beforeStored=await storedConfiguration(active)
  await active.locator('[data-remote-refresh]').click();await expect(active.getByRole('status')).toContainText('Remote connection unavailable');await expect(active.getByRole('dialog')).toHaveCount(0)
  const supplied=await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'));expect(supplied).toMatchObject({sourceReads:1,storeReads:1,presentations:0,assertions:0,grantSignatures:0,keyImports:0})
  expect(f.host.output()).not.toContain('"kind":"installed"');expect(f.host.output()).not.toContain('"kind":"dispatched"');expect(await storedConfiguration(active)).toEqual(beforeStored)
  await closeSession(active);await active.locator('[data-remote-refresh]').click();await expect(active.locator('[data-remote-refresh]')).toBeEnabled();expect(await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))).toEqual(supplied);expect(f.observations().browserConnections).toBe(1)
  await evidence(info,{mode,artifacts:f.artifacts,supplied,scope:f.scope,stored:beforeStored,host:f.host.output(),transport:f.observations()})
 }finally{if(active)await closeSession(active).catch(()=>{});await f.dispose()}
})

for(const action of ['cancel','deadline','forget'] as const)test(`RunHandoff_ActualRelayWasmFreshApprovalRefusal_${action}_003`,async({page,context},info)=>{
 const f=await entryFixture(context,undefined,true,true,'resume');let active:Page|undefined
 try{
  active=await installAndReload(page,context);await active.locator('[data-remote-refresh]').click();const dialog=active.getByRole('dialog',{name:'Approve remote session'});await expect(dialog).toBeVisible();expect(JSON.parse(await dialog.locator('pre').innerText()).scope).toEqual(f.scope)
  const before=await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'));expect(before).toMatchObject({presentations:1,assertions:0,grantSignatures:0,keyImports:0})
  if(action==='cancel')await active.getByRole('button',{name:'Cancel',exact:true}).click()
  if(action==='forget'){const admin=await context.newPage();await admin.goto('https://desk.invalid/browser-remote.html');expect(await admin.evaluate(async()=>{const api=await import(location.origin+'/test-only-pairing-helper.mjs');return api.createBrowserEnrollmentStore({origin:location.origin,databaseName:'CC_DESK_TEST_ONLY_ENTRY'}).revoke()})).toEqual({revision:'1',state:'revoked'})}
  await expect(active.getByRole('status')).toContainText('Remote connection unavailable');await expect(dialog).toHaveCount(0)
  const after=await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'));expect(after).toEqual(before);expect(f.host.output()).not.toContain('"kind":"installed"');expect(f.host.output()).not.toContain('"kind":"dispatched"')
  await closeSession(active);await active.locator('[data-remote-refresh]').click();await expect(active.locator('[data-remote-refresh]')).toBeEnabled();expect(await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyPairing'))).toEqual(after);expect(f.observations().browserConnections).toBe(1)
  await evidence(info,{action,artifacts:f.artifacts,before,after,host:f.host.output(),transport:f.observations()})
 }finally{if(active)await closeSession(active).catch(()=>{});await f.dispose()}
})
