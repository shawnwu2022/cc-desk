// 显式执行：node tests/scripts/opencodeObserver.real.mjs <installed-opencode.exe>
// 只使用临时 HOME/XDG、官方元数据 HTTP；不发提示词或调用模型。
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import http from 'node:http'
import net from 'node:net'
import { spawn, execFileSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'
import { randomUUID } from 'node:crypto'
import assert from 'node:assert/strict'

const program=process.argv[2]
assert(program && path.isAbsolute(program),'An explicit installed CLI executable is required')
const root=fs.mkdtempSync(path.join(os.tmpdir(),'ccdesk-observer-real-')), env={}
for(const key of ['PATH','SystemRoot','WINDIR','ComSpec','PATHEXT'])if(process.env[key])env[key]=process.env[key]
for(const key of ['HOME','USERPROFILE','APPDATA','LOCALAPPDATA','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_STATE_HOME','TEMP','TMP']){env[key]=path.join(root,key);fs.mkdirSync(env[key],{recursive:true})}
Object.assign(env,{OPENCODE_DISABLE_AUTOUPDATE:'true',OPENCODE_DISABLE_MODELS_FETCH:'true',OPENCODE_DISABLE_DEFAULT_PLUGINS:'true',OPENCODE_DISABLE_EXTERNAL_SKILLS:'true',OPENCODE_DISABLE_PROJECT_CONFIG:'true',OPENCODE_EXPERIMENTAL_DISABLE_FILEWATCHER:'true',OPENCODE_PURE:'true',OPENCODE_DB:'observer-fixture.db',OPENCODE_SERVER_USERNAME:'opencode',OPENCODE_SERVER_PASSWORD:randomUUID(),CC_DESK_OBSERVER_RUN:randomUUID(),CC_DESK_OBSERVER_GENERATION:'1',CC_DESK_OBSERVER_CAPABILITY:randomUUID().replaceAll('-','')})
const version=execFileSync(program,['--version'],{env,windowsHide:true,encoding:'utf8'}).trim()
env.OPENCODE_PURE='false'
const config=path.join(env.XDG_CONFIG_HOME,'opencode');fs.mkdirSync(path.join(config,'node_modules'),{recursive:true})
const pkg={dependencies:{'@opencode-ai/plugin':version}}
fs.writeFileSync(path.join(config,'package.json'),JSON.stringify(pkg));fs.writeFileSync(path.join(config,'package-lock.json'),JSON.stringify({lockfileVersion:3,packages:{'':pkg}}))
const project=path.join(root,'project');fs.mkdirSync(project)
const events=[], capture=http.createServer(async(request,response)=>{let body='';for await(const bytes of request)body+=bytes;assert.equal(request.headers['x-cc-desk-capability'],env.CC_DESK_OBSERVER_CAPABILITY);events.push({generation:request.headers['x-cc-desk-generation'],...JSON.parse(body)});response.writeHead(204).end()})
await new Promise(resolve=>capture.listen(0,'127.0.0.1',resolve));env.CC_BOX_HOOK_PORT=String(capture.address().port)
const plugin=pathToFileURL(path.resolve('src-tauri/src/opencode_observer.mjs')).href
const instrumented=path.join(root,'instrumented.mjs'), observed=path.join(root,'plugin-events.jsonl')
fs.writeFileSync(instrumented,`import fs from 'node:fs';import { CCDeskObserver } from ${JSON.stringify(plugin)};export default async(input,options)=>{fs.appendFileSync(${JSON.stringify(observed)},JSON.stringify({type:'initialized',hasClient:!!input.client})+'\\n');const hooks=await CCDeskObserver(input,options);return {...hooks,event:async message=>{fs.appendFileSync(${JSON.stringify(observed)},JSON.stringify({type:message.event?.type,keys:Object.keys(message.event?.properties??{})})+'\\n');return hooks.event?.(message)}}}`)
let child,base
async function until(predicate){for(let n=0;n<100;n++){if(predicate())return;await new Promise(r=>setTimeout(r,100))}throw Error('bounded fixture wait expired')}
async function stop(){if(child){const previous=child;child=null;await new Promise(resolve=>{previous.once('exit',resolve);previous.kill()})}}
async function start(id){
  const socket=net.createServer();await new Promise(resolve=>socket.listen(0,'127.0.0.1',resolve));const port=socket.address().port;await new Promise(resolve=>socket.close(resolve))
  env.OPENCODE_CONFIG_CONTENT=JSON.stringify({plugin:[[pathToFileURL(instrumented).href,{sessionID:id,directory:project}]]})
  child=spawn(program,['serve','--hostname','127.0.0.1','--port',String(port)],{env,cwd:project,windowsHide:true,stdio:'ignore'});base=`http://127.0.0.1:${port}`
  for(let n=0;n<100;n++){try{const r=await request('GET','/global/health');if(r.healthy)return}catch{}if(child.exitCode!==null)throw Error('fixture server exited');await new Promise(r=>setTimeout(r,100))}throw Error('server health timeout')
}
async function request(method,route,body){const response=await fetch(base+route,{method,headers:{authorization:'Basic '+Buffer.from(`opencode:${env.OPENCODE_SERVER_PASSWORD}`).toString('base64'),'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(3000)});assert(response.ok,`HTTP ${response.status}`);return response.json()}
const report={version,root,checks:[]}
try {
  await start()
  const config=await request('GET','/config');report.configPluginCount=config.plugin?.length ?? 0
  const created=await request('POST','/session',{})
  await until(()=>events.some(e=>e.session_id===created.id&&e.hook_event_name==='SessionStart'))
  report.checks.push('installed CLI loads the production plugin and binds its created main ID')
  await request('POST',`/session/${created.id}/abort`,{})
  await until(()=>events.some(e=>e.session_id===created.id&&e.notification_type==='opencode_idle'))
  report.checks.push('official abort publishes session.status idle through the real plugin')
  const other=await request('POST','/session',{})
  await request('POST',`/session/${other.id}/abort`,{})
  await request('PATCH',`/session/${created.id}`,{title:'Official persistent rename'})
  const reread=await request('GET',`/session/${created.id}`);assert.equal(reread.title,'Official persistent rename')
  assert(!events.some(e=>e.session_id===other.id))
  report.checks.push('other main sessions do not rebind the owned observer')
  await stop();env.CC_DESK_OBSERVER_GENERATION='2';await start(created.id);await request('GET','/config')
  await until(()=>events.some(e=>e.generation==='2'&&e.session_id===created.id&&e.hook_event_name==='SessionStart'))
  assert.equal((await request('GET',`/session/${created.id}`)).title,'Official persistent rename')
  report.checks.push('resumed identity and renamed title survive a new server generation')
  assert(!events.some(e=>e.hook_event_name==='Stop'))
  report.checks.push('idle never emitted a fabricated completion hook')
  report.passed=true
} catch(error){report.error=error.message;report.pluginEvents=fs.existsSync(observed)?fs.readFileSync(observed,'utf8').trim().split('\n').map(x=>JSON.parse(x)):[];process.exitCode=1}
finally{await stop();capture.closeAllConnections();await new Promise(resolve=>capture.close(resolve));fs.writeFileSync(path.join(root,'result.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report))}
