import {test,expect,type Page} from '@playwright/test'
import {build} from 'esbuild'
import {readFile} from 'node:fs/promises'

let module:string,style:string
const grant={version:1,context:{hostEndpointId:'01'.repeat(32),clientEndpointId:'02'.repeat(32),hostKeyEpoch:'1',clientKeyEpoch:'2',activation:'03'.repeat(32),incarnation:'1',policyEpoch:'1',issuerEpoch:'1',origin:'wss://relay.invalid/route'},principalId:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',challenge:'06'.repeat(32),grantId:'07'.repeat(32),createdHostMs:'1000',expiresHostMs:'5000'}
test.use({serviceWorkers:'block'})
test.beforeAll(async()=>{
 module=(await build({entryPoints:['remote/ui-terminal/src/browserGrantPresenter.ts'],bundle:true,platform:'browser',format:'esm',target:'es2022',write:false})).outputFiles[0].text
 style=await readFile('src/styles/browser-remote.css','utf8')
})
async function prepare(page:Page){
 await page.route('**/*',async route=>{const url=new URL(route.request().url());const headers={'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'};if(url.origin!=='https://desk.invalid')return route.abort();await route.fulfill({headers,contentType:url.pathname==='/presenter.mjs'?'text/javascript':'text/html',body:url.pathname==='/presenter.mjs'?module:`<!doctype html><style>${style}</style><button id="original">Original focus</button>`})})
 await page.goto('https://desk.invalid/')
 await page.evaluate(async value=>{const api=await import(location.origin+'/presenter.mjs');const controller=new AbortController();const present=api.createBrowserGrantPresenter(location.origin);Reflect.set(globalThis,'fixture',{controller,present,value,state:'not-started'});},grant)
}
async function start(page:Page){await page.evaluate(()=>{const f=Reflect.get(globalThis,'fixture');f.state='pending';f.promise=f.present(f.value,f.controller.signal).then(()=>{f.state='approved'},()=>{f.state='rejected'})})}
async function state(page:Page){return page.evaluate(()=>Reflect.get(globalThis,'fixture').state)}

test('GrantPresenter_CompleteFrozenDisplayAndTrustedClick_001',async({page})=>{
 await prepare(page);await page.locator('#original').focus();await start(page)
 const dialog=page.getByRole('dialog',{name:'Approve remote session'})
 await expect(dialog).toBeVisible();await expect(page.getByRole('button',{name:'Cancel',exact:true})).toBeFocused()
 expect(JSON.parse(await dialog.locator('pre').innerText())).toEqual(grant)
 await page.evaluate(()=>{Reflect.get(globalThis,'fixture').value.scope.run='CHANGED_AFTER_DISPLAY'})
 await expect(dialog).toContainText('PUBLIC_TEST_ONLY');await expect(dialog).not.toContainText('CHANGED_AFTER_DISPLAY')
 expect(await state(page)).toBe('pending');await page.getByRole('button',{name:'Approve this connection',exact:true}).click()
 await expect.poll(()=>state(page)).toBe('approved');await expect(dialog).toHaveCount(0);await expect(page.locator('#original')).toBeFocused()
})

test('GrantPresenter_ProgrammaticClickCannotApprove_002',async({page})=>{
 await prepare(page);await start(page)
 await page.getByRole('button',{name:'Approve this connection',exact:true}).evaluate((button:HTMLButtonElement)=>button.click())
 expect(await state(page)).toBe('pending')
 await page.getByRole('button',{name:'Cancel',exact:true}).click();await expect.poll(()=>state(page)).toBe('rejected')
})

for(const action of ['escape','abort','remove','change','attribute','hide','pagehide','body','close','freeze'] as const){
 test(`GrantPresenter_LifecycleRejects_${action}_003`,async({page})=>{
  await prepare(page);await start(page);await expect(page.getByRole('dialog')).toBeVisible()
  if(action==='escape')await page.keyboard.press('Escape')
  else await page.evaluate(action=>{const f=Reflect.get(globalThis,'fixture'),dialog=document.querySelector('dialog')!;if(action==='abort')f.controller.abort();if(action==='remove')dialog.remove();if(action==='change')dialog.querySelector('pre')!.textContent='changed';if(action==='attribute')dialog.querySelector('pre')!.hidden=true;if(action==='hide'){Object.defineProperty(document,'visibilityState',{configurable:true,value:'hidden'});document.dispatchEvent(new Event('visibilitychange'))}if(action==='pagehide')dispatchEvent(new Event('pagehide'));if(action==='body')document.body.replaceWith(document.createElement('body'));if(action==='close')dialog.close();if(action==='freeze')document.dispatchEvent(new Event('freeze'))},action)
  await expect.poll(()=>state(page)).toBe('rejected');await expect(page.getByRole('dialog')).toHaveCount(0)
 })
}

test('GrantPresenter_RepeatedAndConcurrentCallsStayClosed_004',async({page})=>{
 await prepare(page);await start(page)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'fixture');return f.present(f.value,new AbortController().signal).then(()=>false,()=>true)})).toBe(true)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'fixture'),api=await import(location.origin+'/presenter.mjs');return api.createBrowserGrantPresenter(location.origin)(f.value,new AbortController().signal).then(()=>false,()=>true)})).toBe(true)
 await page.getByRole('button',{name:'Cancel',exact:true}).click();await expect.poll(()=>state(page)).toBe('rejected')
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'fixture');return f.present(f.value,new AbortController().signal).then(()=>false,()=>true)})).toBe(true)
})

test('GrantPresenter_AccessorAndMalformedPayloadRejectWithoutDisplay_005',async({page})=>{
 await prepare(page)
 expect(await page.evaluate(async()=>{const f=Reflect.get(globalThis,'fixture');let calls=0;Object.defineProperty(f.value,'role',{get(){calls++;return 'Controller'}});await f.present(f.value,f.controller.signal).catch(()=>{});return calls})).toBe(0)
 await expect(page.getByRole('dialog')).toHaveCount(0)
 await prepare(page);await page.evaluate(()=>{Reflect.get(globalThis,'fixture').value.role='Admin'});await start(page);await expect.poll(()=>state(page)).toBe('rejected')
})

test('GrantPresenter_InertTextAndNarrowViewport_006',async({page},info)=>{
 await prepare(page);await page.setViewportSize({width:390,height:844})
 await page.evaluate(()=>{Reflect.get(globalThis,'fixture').value.scope.run='<img src=x onerror=alert(1)>\u202ePUBLIC_TEST_ONLY'})
 await start(page);const dialog=page.getByRole('dialog');await expect(dialog).toBeVisible()
 expect(await dialog.locator('img').count()).toBe(0);await expect(dialog.locator('pre')).toContainText('\\u202e')
 expect(await dialog.evaluate(element=>element.scrollWidth<=element.clientWidth)).toBe(true)
 expect(await dialog.evaluate(element=>element.scrollTop)).toBe(0)
 await expect(page.getByRole('button',{name:'Cancel',exact:true})).toBeInViewport()
 await expect(page.getByRole('button',{name:'Approve this connection',exact:true})).toBeInViewport()
 await page.screenshot({path:info.outputPath('grant-presenter-mobile.png')})
 await page.getByRole('button',{name:'Cancel',exact:true}).click()
})

test('GrantPresenter_PreAbortedAndWrongOriginCannotPresent_007',async({page})=>{
 await prepare(page);await page.evaluate(()=>Reflect.get(globalThis,'fixture').controller.abort());await start(page)
 await expect.poll(()=>state(page)).toBe('rejected');await expect(page.getByRole('dialog')).toHaveCount(0)
 expect(await page.evaluate(async()=>{const api=await import(location.origin+'/presenter.mjs');try{api.createBrowserGrantPresenter('https://other.invalid');return false}catch{return true}})).toBe(true)
})

test('GrantPresenter_AbortCleansQuotaWithoutRevivingOriginal_008',async({page})=>{
 await prepare(page);await start(page);await page.evaluate(async()=>{const f=Reflect.get(globalThis,'fixture');f.controller.abort();await f.promise})
 await expect(page.getByRole('dialog')).toHaveCount(0)
 await page.evaluate(async()=>{const f=Reflect.get(globalThis,'fixture'),api=await import(location.origin+'/presenter.mjs');f.state='pending';f.promise=api.createBrowserGrantPresenter(location.origin)(f.value,new AbortController().signal).then(()=>{f.state='approved'},()=>{f.state='rejected'})})
 await expect(page.getByRole('dialog')).toBeVisible();await page.getByRole('button',{name:'Cancel',exact:true}).click();await expect.poll(()=>state(page)).toBe('rejected')
})
