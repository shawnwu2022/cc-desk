import {test,expect,type Page,type BrowserContext,type TestInfo} from '@playwright/test'
import {writeFile} from 'node:fs/promises'
import {entryFixture} from './entry-fixture'

test.use({serviceWorkers:'block'})
const databaseName='CC_DESK_BROWSER_ENROLLMENT_V1'
async function observe(page:Page){return page.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyProduct'))}
async function stored(page:Page){return page.evaluate(async databaseName=>{
 const db=await new Promise<IDBDatabase>((r,j)=>{const q=indexedDB.open(databaseName,1);q.onsuccess=()=>r(q.result);q.onerror=()=>j(q.error)})
 try{return await new Promise<any>((r,j)=>{const tx=db.transaction('enrollment','readonly'),q=tx.objectStore('enrollment').get('current');let record:any;q.onsuccess=()=>{record=q.result};tx.oncomplete=()=>r(record?{revision:record.revision,state:record.state,owner:record.owner,configuration:record.configuration?JSON.parse(new TextDecoder().decode(record.configuration)):undefined}:undefined);tx.onabort=()=>j(tx.error)})}finally{db.close()}
 },databaseName)}
async function installAndReload(page:Page,context:BrowserContext){
 await page.goto('https://desk.invalid/browser-remote.html')
 expect(await page.evaluate(async()=>{const installer=await import(location.origin+'/test-only-enrollment-install.mjs');return installer.install()})).toEqual({revision:'1',state:'active'})
 await page.close();const active=await context.newPage();await active.goto('https://desk.invalid/browser-remote.html');return active
}
async function revoke(context:BrowserContext){const admin=await context.newPage();await admin.goto('https://desk.invalid/browser-remote.html');return admin.evaluate(async databaseName=>{const api=await import(location.origin+'/test-only-pairing-helper.mjs');return api.createBrowserEnrollmentStore({origin:location.origin,databaseName}).revoke()},databaseName)}
async function evidence(info:TestInfo,value:unknown){const path=info.outputPath('product-provider-chain.json');await writeFile(path,JSON.stringify(value,null,2));await info.attach('product-provider-chain',{path,contentType:'application/json'})}

test('ProductProvider_ActualBundleInstalledRecordNewRunAndGracefulRetirement_001',async({page,context},info)=>{
 const f=await entryFixture(context,undefined,true,true,'resume',true)
 try{
  const active=await installAndReload(page,context),errors:string[]=[];active.on('pageerror',error=>errors.push(error.message))
  expect(await active.evaluate(async()=>Object.keys(await import(location.origin+'/browser-profile.js')))).toEqual(['readApprovedBrowserInputs'])
  expect(await observe(active)).toEqual({assertions:0,imports:0,signatures:0,databaseOpens:0});expect(f.observations()).toEqual({browserConnections:0,artifactRequests:0})
  const before=await stored(active);expect(before.configuration.relay.mode).toBe(process.env.CC_DESK_TEST_ONLY_ENTRY_RELAY_MODE??'legacy');expect(before.configuration.grant.scope).toEqual(f.storedScope);expect(f.scope).not.toEqual(f.storedScope)
  await active.locator('[data-remote-refresh]').click();const dialog=active.getByRole('dialog',{name:'Approve remote session'});await expect(dialog).toBeVisible()
  const shown=JSON.parse(await dialog.locator('pre').innerText());expect(shown.scope).toEqual(f.scope)
  const pending=await observe(active);expect(pending).toMatchObject({assertions:0,imports:0})
  await active.getByRole('button',{name:'Approve this connection',exact:true}).evaluate((button:HTMLButtonElement)=>button.click());expect(await observe(active)).toEqual(pending)
  await active.getByRole('button',{name:'Approve this connection',exact:true}).click();await expect(active.getByRole('status')).toContainText('Snapshot verified')
  const approved=await observe(active);expect(approved).toMatchObject({assertions:1,imports:1})
  const descriptor=await active.evaluate(()=>Reflect.get(globalThis,'__ccDeskTestOnlyRunDescriptor'));expect(descriptor.target).toMatchObject(f.scope)
  await active.locator('[data-remote-input]').fill('TEST_ONLY_PRODUCT_PROVIDER');await active.locator('[data-remote-send]').click();await expect(active.locator('[data-remote-input]')).toHaveValue('')
  await active.locator('[data-remote-disconnect]').click();await expect(active.getByRole('status')).toContainText('Locked')
  // 不调用任何 fixture session.close；产品 Disconnect 必须让原持久 owner 正常退役。
  await expect.poll(async()=>{const record=await stored(active);return record?.owner}).toBeUndefined()
  const after=await stored(active);expect(after).toEqual(before)
  const effect=await f.host.wait(value=>value.kind==='effects');expect(effect).toMatchObject({writeCount:1,bytes:Buffer.from('TEST_ONLY_PRODUCT_PROVIDER').toString('hex')});await f.waitBridgeRetired();expect(f.transport()).toMatchObject({errors:[],hostAccepted:true,hostReady:process.env.CC_DESK_TEST_ONLY_ENTRY_RELAY_MODE==='rendezvous-v1'})
  await active.locator('[data-remote-refresh]').click();await expect(active.getByRole('status')).toContainText('Remote connection unavailable');expect(f.observations()).toEqual({browserConnections:1,artifactRequests:2})
  expect((await observe(active)).assertions).toBe(1);expect(errors).toEqual([])
  await f.retireRelay();await evidence(info,{boundary:'real built product provider, original Relay/Host/WASM and Edge IDB; synthetic installed identity and authenticator, not production enrollment/deployment',artifacts:f.artifacts,before,after,shown,descriptor,pending,approved,effect,transport:f.observations(),relayTransport:f.transport()})
 }finally{await f.dispose()}
})

for(const mode of ['wrong-pin','wrong-principal','wrong-epoch','wrong-role'] as const)test(`ProductProvider_OriginalIdentityStillRejects_${mode}_002`,async({page,context},info)=>{
 const f=await entryFixture(context,undefined,true,true,mode,true)
 try{
  const active=await installAndReload(page,context),before=await stored(active)
  await active.locator('[data-remote-refresh]').click();await expect(active.getByRole('status')).toContainText('Remote connection unavailable');await expect(active.getByRole('dialog')).toHaveCount(0)
  expect(await observe(active)).toMatchObject({assertions:0,imports:0});expect(f.host.output()).not.toContain('"kind":"installed"');expect(f.host.output()).not.toContain('"kind":"dispatched"')
  await expect.poll(async()=>{const record=await stored(active);return record?.owner}).toBeUndefined();expect(await stored(active)).toEqual(before)
  await f.retireRelay();await evidence(info,{mode,artifacts:f.artifacts,observed:await observe(active),stored:before,host:f.host.output(),transport:f.observations(),relayTransport:f.transport()})
 }finally{await f.dispose()}
})

for(const action of ['cancel','deadline','forget'] as const)test(`ProductProvider_OriginalFreshApprovalRefusal_${action}_003`,async({page,context},info)=>{
 const f=await entryFixture(context,undefined,true,true,'resume',true)
 try{
  const active=await installAndReload(page,context)
  await active.locator('[data-remote-refresh]').click();const dialog=active.getByRole('dialog',{name:'Approve remote session'});await expect(dialog).toBeVisible();expect(JSON.parse(await dialog.locator('pre').innerText()).scope).toEqual(f.scope)
  if(action==='cancel')await active.getByRole('button',{name:'Cancel',exact:true}).click()
  if(action==='forget')expect(await revoke(context)).toEqual({revision:'1',state:'revoked'})
  await expect(active.getByRole('status')).toContainText('Remote connection unavailable');await expect(dialog).toHaveCount(0)
  expect(await observe(active)).toMatchObject({assertions:0,imports:0});expect(f.host.output()).not.toContain('"kind":"installed"');expect(f.host.output()).not.toContain('"kind":"dispatched"')
  await expect.poll(async()=>{const record=await stored(active);return record?.owner}).toBeUndefined()
  if(action==='forget'){
   const next=await context.newPage();await next.goto('https://desk.invalid/browser-remote.html');await next.locator('[data-remote-refresh]').click();await expect(next.getByRole('status')).toContainText('Locked');expect(await observe(next)).toMatchObject({assertions:0,imports:0,signatures:0});expect(f.observations().browserConnections).toBe(1)
  }
  await f.retireRelay();await evidence(info,{action,artifacts:f.artifacts,observed:await observe(active),record:await stored(active),host:f.host.output(),transport:f.observations(),relayTransport:f.transport()})
 }finally{await f.dispose()}
})

test('ProductProvider_MissingInstallationAndUrlSelectorsStayLocked_004',async({page,context},info)=>{
 const f=await entryFixture(context,undefined,true,true,'resume',true)
 try{
  await page.goto('https://desk.invalid/browser-remote.html?databaseName=PUBLIC_TEST_ONLY_ATTACKER&scopePolicy=authenticated-host-run#approved')
  expect(await observe(page)).toEqual({assertions:0,imports:0,signatures:0,databaseOpens:0})
  await page.locator('[data-remote-refresh]').click();await expect(page.getByRole('status')).toContainText('Locked');await expect(page.locator('textarea')).toHaveCount(0)
  expect(await observe(page)).toMatchObject({assertions:0,imports:0,signatures:0});expect(f.observations()).toEqual({browserConnections:0,artifactRequests:0})
  await f.retireRelay();await evidence(info,{artifacts:f.artifacts,observed:await observe(page),transport:f.observations(),relayTransport:f.transport()})
 }finally{await f.dispose()}
})
