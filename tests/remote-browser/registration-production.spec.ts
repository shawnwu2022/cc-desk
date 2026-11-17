import {test,expect} from '@playwright/test'
import {readFile} from 'node:fs/promises'
import {resolve,sep,extname} from 'node:path'
test.use({serviceWorkers:'block'})
test('RegistrationEntry_ProductionCspExplicitLocked_001',async({page,context})=>{
 const root=resolve('dist');let sourceReads=0,sockets=0,workers=0;const errors:string[]=[]
 page.on('websocket',()=>sockets++);page.on('worker',()=>workers++);page.on('pageerror',e=>errors.push(e.message))
 await context.addInitScript(()=>{Reflect.set(globalThis,'ceremonies',0);for(const name of ['create','get'])Object.defineProperty(navigator.credentials,name,{configurable:true,value:()=>{Reflect.set(globalThis,'ceremonies',Reflect.get(globalThis,'ceremonies')+1);throw Error('TEST_ONLY unexpected WebAuthn')}})})
 await context.route('https://desk.invalid/**',async route=>{
  const path=new URL(route.request().url()).pathname;if(path==='/browser-registration.js')sourceReads++
  const file=resolve(root,'.'+path),headers={'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'}
  try{if(!file.startsWith(root+sep))throw Error('path');await route.fulfill({headers,contentType:({'.html':'text/html','.js':'text/javascript','.css':'text/css','.wasm':'application/wasm'} as Record<string,string>)[extname(file)]??'application/octet-stream',body:await readFile(file)})}catch{await route.fulfill({status:404,headers,body:''})}
 })
 await page.goto('https://desk.invalid/browser-registration.html')
 await expect(page.getByRole('status')).toHaveText('Locked');expect(sourceReads).toBe(0)
 await expect(page.getByRole('button',{name:'Start registration',exact:true})).toBeDisabled()
 await page.getByRole('button',{name:'Prepare registration',exact:true}).click()
 await expect(page.getByRole('dialog')).toBeVisible()
 await page.getByRole('dialog').getByRole('button',{name:'Cancel',exact:true}).click()
 await expect(page.getByRole('status')).toHaveText('Locked');await expect(page.getByRole('button',{name:'Start registration',exact:true})).toBeDisabled()
 expect(sourceReads).toBe(1);expect(sockets).toBe(0);expect(workers).toBe(0);expect(await page.evaluate(()=>Reflect.get(globalThis,'ceremonies'))).toBe(0);expect(errors).toEqual([])
 await page.goto('https://desk.invalid/browser-remote.html');await expect(page.getByRole('status')).toContainText('Locked')
 await page.locator('[data-remote-refresh]').click();await expect(page.getByRole('status')).toContainText('Locked');expect(sockets).toBe(0);expect(workers).toBe(0);expect(errors).toEqual([])
})
