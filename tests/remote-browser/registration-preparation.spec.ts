import {test,expect} from '@playwright/test'
import {readFile} from 'node:fs/promises'
import {resolve,sep,extname} from 'node:path'
test.use({serviceWorkers:'block'})
test.beforeEach(async({page,context})=>{
 await context.addInitScript(()=>{
  const state={generations:0,signatures:0,sockets:0,keys:[] as CryptoKey[],originalHandles:true};Reflect.set(globalThis,'TEST_ONLY',state)
  const generate=crypto.subtle.generateKey.bind(crypto.subtle),sign=crypto.subtle.sign.bind(crypto.subtle)
  crypto.subtle.generateKey=(async(...args:any[])=>{state.generations++;const pair=await Reflect.apply(generate,crypto.subtle,args) as CryptoKeyPair;state.keys.push(pair.privateKey);return pair}) as typeof crypto.subtle.generateKey
  crypto.subtle.sign=(async(algorithm,key,data)=>{state.signatures++;state.originalHandles&&=state.keys.includes(key);return sign(algorithm,key,data)}) as typeof crypto.subtle.sign
  Reflect.set(globalThis,'WebSocket',class{constructor(){state.sockets++;throw Error('TEST_ONLY stop before external socket')}})
  for(const name of ['create','get'])Object.defineProperty(navigator.credentials,name,{value:()=>{throw Error('TEST_ONLY no authenticator')}})
 })
 const root=resolve('dist')
 await context.route('https://desk.invalid/**',async route=>{
  const file=resolve(root,'.'+new URL(route.request().url()).pathname),headers={'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'}
  try{if(!file.startsWith(root+sep))throw Error('path');await route.fulfill({headers,contentType:({'.html':'text/html','.js':'text/javascript','.css':'text/css'} as Record<string,string>)[extname(file)]??'application/octet-stream',body:await readFile(file)})}catch{await route.fulfill({status:404,headers,body:''})}
 })
 await page.goto('https://desk.invalid/browser-registration.html')
})
function configuration(k:any){return {version:1,plan:{
 databaseName:'CC_DESK_BROWSER_ENROLLMENT_V1',expectedRevision:null,relaySpki:k.relaySpki,requestedUserHandle:'AQID',
 configuration:{version:1,profile:{origin:'https://desk.invalid',rpId:'desk.invalid'},localSpki:k.tlsSpki,hostSpki:k.tlsSpki,
 trust:{hostKeyEpoch:'1',clientKeyEpoch:'2',origin:'wss://relay.invalid/route'},
 grant:{issuerSpki:k.grantSpki,principal:'04'.repeat(32),scope:{session:'05'.repeat(16),run:'PUBLIC_TEST_ONLY',generation:'7'},role:'Controller',budgetMs:10000,peerInstall:{budgetMs:1000},profile:{origin:'https://desk.invalid',rpId:'desk.invalid',allowBackedUp:false}},
 relay:{mode:'rendezvous-v1',endpoint:'wss://relay.invalid/route',tunnel:Buffer.alloc(16,9).toString('base64url'),keyId:Buffer.alloc(16,8).toString('base64url')},
 artifacts:{wasm:'https://desk.invalid/terminal.wasm',glue:'https://desk.invalid/terminal.js'}}},
 artifacts:{wasm:'https://desk.invalid/registration.wasm',glue:'https://desk.invalid/registration.js'}}}
for(const mode of ['start','mismatch','cancel','mutation','hidden','expired','bidi'])test('Preparation_Production_'+mode,async({page})=>{
 expect(await page.evaluate(()=>Reflect.get(globalThis,'TEST_ONLY').generations)).toBe(0)
 await page.getByRole('button',{name:'Prepare registration',exact:true}).click()
 await page.getByRole('button',{name:'Generate ephemeral identity',exact:true}).dispatchEvent('click')
 expect(await page.evaluate(()=>Reflect.get(globalThis,'TEST_ONLY').generations)).toBe(0)
 await page.getByRole('button',{name:'Generate ephemeral identity',exact:true}).click()
 await expect(page.getByRole('textbox')).toBeVisible()
 const keys=JSON.parse(await page.locator('dialog pre').innerText()),config=configuration(keys)
 if(mode==='mismatch')config.plan.relaySpki=keys.tlsSpki
 if(mode==='bidi')config.plan.configuration.grant.scope.run='PUBLIC\u061c\u202e'
 await page.getByRole('textbox').fill(JSON.stringify(config))
 await page.getByRole('button',{name:'Review public configuration',exact:true}).click()
 if(mode!=='mismatch'){
  expect(JSON.parse(await page.locator('dialog pre').innerText())).toEqual(config)
  await page.getByRole('button',{name:'Approve this exact registration',exact:true}).dispatchEvent('click')
  await expect(page.getByRole('button',{name:'Start registration',exact:true})).toBeDisabled()
  if(mode==='bidi'){expect(await page.locator('dialog pre').innerText()).toContain('\\u061c');expect(await page.locator('dialog pre').innerText()).toContain('\\u202e')}
  if(mode==='cancel'||mode==='bidi')await page.getByRole('dialog').getByRole('button',{name:'Cancel',exact:true}).click()
  else if(mode==='mutation')await page.locator('dialog pre').evaluate(e=>e.textContent='changed')
  else{
   await page.getByRole('button',{name:'Approve this exact registration',exact:true}).click()
   await expect(page.getByRole('button',{name:'Start registration',exact:true})).toBeEnabled()
   expect(await page.evaluate(()=>Reflect.get(globalThis,'TEST_ONLY').sockets)).toBe(0)
   if(mode==='start'){await page.getByRole('button',{name:'Start registration',exact:true}).dispatchEvent('click');expect(await page.evaluate(()=>Reflect.get(globalThis,'TEST_ONLY').sockets)).toBe(0);await page.getByRole('button',{name:'Start registration',exact:true}).click()}
   if(mode==='hidden')await page.evaluate(()=>{Object.defineProperty(document,'visibilityState',{configurable:true,value:'hidden'});document.dispatchEvent(new Event('visibilitychange'))})
   if(mode==='expired')await page.evaluate(()=>{const now=Date.now();Date.now=()=>now+61000})
  }
 }
 await expect(page.getByRole('status')).toContainText('Locked')
 expect(await page.evaluate(()=>Reflect.get(globalThis,'TEST_ONLY').sockets)).toBe(mode==='start'?1:0)
 expect(await page.evaluate(()=>{const f=Reflect.get(globalThis,'TEST_ONLY');return {generations:f.generations,originalHandles:f.originalHandles,nonexportable:f.keys.every((k:CryptoKey)=>!k.extractable)}})).toEqual({generations:3,originalHandles:true,nonexportable:true})
})


test('Preparation_LateGenerationCancellation_007',async({page})=>{
 await page.evaluate(()=>{const original=crypto.subtle.generateKey.bind(crypto.subtle);crypto.subtle.generateKey=(async(...args:any[])=>{const pair=await Reflect.apply(original,crypto.subtle,args);await new Promise<void>(resolve=>Reflect.set(globalThis,'releaseGeneration',resolve));return pair}) as typeof crypto.subtle.generateKey})
 await page.getByRole('button',{name:'Prepare registration',exact:true}).click()
 await page.getByRole('button',{name:'Generate ephemeral identity',exact:true}).click()
 await expect.poll(()=>page.evaluate(()=>typeof Reflect.get(globalThis,'releaseGeneration'))).toBe('function')
 await page.getByRole('dialog').getByRole('button',{name:'Cancel',exact:true}).click()
 await expect(page.getByRole('status')).toHaveText('Preparing identity')
 await page.evaluate(()=>Reflect.get(globalThis,'releaseGeneration')())
 await expect(page.getByRole('status')).toContainText('Locked')
 expect(await page.evaluate(()=>Reflect.get(globalThis,'TEST_ONLY').generations)).toBe(1)
 await expect(page.getByRole('button',{name:'Prepare registration',exact:true})).toBeDisabled()
 await expect(page.getByRole('button',{name:'Start registration',exact:true})).toBeDisabled()
})
test('Preparation_AbsoluteExpiry_008',async({page})=>{
 await page.getByRole('button',{name:'Prepare registration',exact:true}).click()
 await page.getByRole('button',{name:'Generate ephemeral identity',exact:true}).click()
 await expect(page.getByRole('textbox')).toBeVisible()
 await page.evaluate(()=>{const now=Date.now();Date.now=()=>now+300001})
 await expect(page.getByRole('status')).toContainText('Locked')
 await expect(page.getByRole('dialog')).toHaveCount(0)
 await expect(page.getByRole('button',{name:'Prepare registration',exact:true})).toBeDisabled()
})
