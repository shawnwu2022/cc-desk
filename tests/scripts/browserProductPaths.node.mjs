import assert from 'node:assert/strict'
import {mkdtempSync,readFileSync,readdirSync,rmSync} from 'node:fs'
import {resolve,join} from 'node:path'
import {tmpdir} from 'node:os'
import test from 'node:test'
import {build,resolveConfig} from 'vite'

const origin='https://desk.invalid'
const mounts=['/','/candidate/0.28/']
const entries=['browser-remote.html','browser-registration.html']
const matches=(value,pattern)=>[...value.matchAll(pattern)].map(m=>m[1])

test('browser build references stay inside the preview mount; desktop base stays unchanged',async()=>{
 assert.equal((await resolveConfig({},'build','production')).base,'/')
 const outDir=mkdtempSync(join(tmpdir(),'ccdesk-browser-path-check-'))
 try{
  await build({mode:'browser-product',logLevel:'silent',build:{outDir,emptyOutDir:true}})
  const content=file=>readFileSync(resolve(outDir,file),'utf8')
  const local=(raw,from,mount)=>{
    const resolved=new URL(raw,new URL(mount+from,origin))
    assert.doesNotMatch(raw,/^(?:\/|[a-z][a-z\d+.-]*:|\\\\)/i,from)
    assert.equal(resolved.origin,origin,from)
    assert.ok(resolved.pathname.startsWith(mount),from)
    return resolved.pathname.slice(mount.length)
  }
  for(const mount of mounts)for(const entry of entries){
    const refs=matches(content(entry),/(?:src|href)="([^"]+)"/g).filter(s=>/assets\//.test(s))
    assert.ok(refs.length>3,entry)
    for(const ref of refs)assert.ok(content(local(ref,entry,mount)).length)
  }
  const output=readdirSync(resolve(outDir,'assets')).filter(f=>/^browser(?:Remote|Registration)-.+\.js$/.test(f))
  assert.equal(output.length,2)
  for(const name of output){
    const file='assets/'+name,s=content(file)
    const worker=matches(s,/["']([^"']*browser(?:Registration)?Entry\.worker-[^"']+\.js)["']/g)
    assert.equal(worker.length,1,file)
    for(const mount of mounts)assert.ok(content(local(worker[0],file,mount)).length)
    const provider=matches(s,/new URL\(["']([^"']*browser-(?:registration|profile)\.js)["'],location\.href\)/g)
    assert.equal(provider.length,1,file)
    const page=name.startsWith('browserRegistration')?'browser-registration.html':'browser-remote.html'
    const expected=name.startsWith('browserRegistration')?'browser-registration.js':'browser-profile.js'
    for(const mount of mounts){
      assert.equal(local(provider[0],page,mount),expected)
      assert.ok(content(expected).length)
    }
  }
 }finally{rmSync(outDir,{recursive:true,force:true})}
})
