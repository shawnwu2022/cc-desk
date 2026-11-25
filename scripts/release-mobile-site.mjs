import {createHash} from 'node:crypto'
import {lstatSync,readFileSync,readdirSync,writeFileSync} from 'node:fs'
import {join,resolve} from 'node:path'
import {pathToFileURL} from 'node:url'
import {crc32} from './release-coverage-archive.mjs'

export const ZIP_NAME='cc-desk-mobile-site.zip', MANIFEST_NAME='cc-desk-mobile-site.manifest.json'
export const mobileReleaseFamily=version=>typeof version==='string'&&/^0\.28\.(?:0|[1-9]\d*)(?:-rc\.[1-9]\d*)?$/.test(version)
const MAX=48*1024*1024, MAX_FILE=8*1024*1024, MAX_COUNT=128
const fixed=new Set(['browser-remote.html','browser-registration.html','browser-profile.js','browser-registration.js','browser-registration.manifest.json',
 ...['remote-runtime','remote-registration-runtime'].flatMap(dir=>['manifest.json','fixture_bg.js','fixture_bg.wasm'].map(file=>dir+'/'+file))])
const dirs=new Set(['assets','remote-runtime','remote-registration-runtime'])
const asset=/^assets\/(?=[a-z0-9_.-]{1,150}$)[a-z0-9_-]+(?:\.[a-z0-9_-]+)*\.(?:js|css|svg|png|jpg|jpeg|webp|woff2?|ico)$/i
const allowed=name=>fixed.has(name)||asset.test(name)
const hash=bytes=>createHash('sha256').update(bytes).digest('hex')
function requireThat(ok){if(!ok)throw Error('Mobile release artifact policy failed')}
function read(file,max){
 const stat=lstatSync(file);requireThat(stat.isFile()&&!stat.isSymbolicLink()&&stat.size>0&&stat.size<=max)
 const bytes=readFileSync(file);requireThat(bytes.length===stat.size);return bytes
}
function verifyPrivatePaths(bytes){
 // A structural privacy check for accidental workstation paths, not a general
 // secret scan. Do not persist paths in diagnostic output.
 requireThat(!/(?:file:\/\/\/[a-z]:|[a-z]:[\\/]+(?:Users|DevTools|source|__w)[\\/]+|\/Users\/[^/]+\/|\/home\/(?:runner|[a-z0-9_-]+)\/(?:work|\.cargo|\.rustup)\/)/i.test(bytes.toString('latin1')))
}
function checkFiles(entries){
 requireThat(entries.length<=MAX_COUNT&&entries.length>fixed.size)
 const seen=new Set(), names=new Set();let total=0;
 for(const [name,bytes] of entries){
  const folded=name.toLowerCase()
  requireThat(allowed(name)&&!seen.has(folded)&&Buffer.isBuffer(bytes)&&bytes.length>0&&bytes.length<=MAX_FILE)
  total+=bytes.length;requireThat(total<=MAX);seen.add(folded);names.add(name);verifyPrivatePaths(bytes)
 }
 requireThat([...fixed].every(name=>names.has(name)))
 return entries.sort(([a],[b])=>a<b?-1:a>b?1:0)
}
export function siteFiles(site){
 const root=lstatSync(site);requireThat(root.isDirectory()&&!root.isSymbolicLink());
 const files=[];let total=0
 function add(name,file){
  requireThat(files.length<MAX_COUNT)
  const bytes=read(file,Math.min(MAX_FILE,MAX-total));total+=bytes.length
  files.push([name,bytes])
 }
 for(const entry of readdirSync(site,{withFileTypes:true})){
  // The browser-product Vite entry also builds these two desktop entry HTMLs;
  // they are neither site dependencies nor ZIP payloads.
  if(['index.html','version-manager.html'].includes(entry.name)&&entry.isFile())continue
  requireThat(!entry.isSymbolicLink())
  if(entry.isDirectory()){
   requireThat(dirs.has(entry.name))
   for(const item of readdirSync(join(site,entry.name),{withFileTypes:true})){
    const name=entry.name+'/'+item.name;requireThat(item.isFile()&&allowed(name))
    add(name,join(site,entry.name,item.name))
   }
  }else {
   requireThat(entry.isFile()&&allowed(entry.name))
   add(entry.name,join(site,entry.name))
  }
 }
 return checkFiles(files)
}
// Deterministic, uncompressed, classic ZIP. Verification parses only its exact
// bounded local form and then checks the entire central/local encoding.
function zip(entries){
 const local=[],central=[];let offset=0,centralSize=0
 for(const [name,bytes] of entries){
  const n=Buffer.from(name,'ascii'),l=Buffer.alloc(30),c=Buffer.alloc(46),crc=crc32(bytes)
  l.writeUInt32LE(0x04034b50,0);l.writeUInt16LE(20,4);l.writeUInt16LE(33,12);l.writeUInt32LE(crc,14);l.writeUInt32LE(bytes.length,18);l.writeUInt32LE(bytes.length,22);l.writeUInt16LE(n.length,26)
  c.writeUInt32LE(0x02014b50,0);c.writeUInt16LE(20,4);c.writeUInt16LE(20,6);c.writeUInt16LE(33,14);c.writeUInt32LE(crc,16);c.writeUInt32LE(bytes.length,20);c.writeUInt32LE(bytes.length,24);c.writeUInt16LE(n.length,28);c.writeUInt32LE(offset,42)
  local.push(l,n,bytes);central.push(c,n);offset+=l.length+n.length+bytes.length;centralSize+=c.length+n.length
 }
 const end=Buffer.alloc(22);end.writeUInt32LE(0x06054b50,0);end.writeUInt16LE(entries.length,8);end.writeUInt16LE(entries.length,10);end.writeUInt32LE(centralSize,12);end.writeUInt32LE(offset,16)
 return Buffer.concat([...local,...central,end])
}
function unzip(bytes){
 const entries=[];let at=0
 while(at+30<=bytes.length&&bytes.readUInt32LE(at)===0x04034b50){
  const size=bytes.readUInt32LE(at+22),nameSize=bytes.readUInt16LE(at+26),start=at+30+nameSize,end=start+size
  requireThat(entries.length<MAX_COUNT&&nameSize>0&&nameSize<=180&&size>0&&size<=MAX_FILE&&end<=bytes.length)
  const nameBytes=bytes.subarray(at+30,start),name=nameBytes.toString('ascii')
  requireThat(Buffer.from(name,'ascii').equals(nameBytes))
  entries.push([name,bytes.subarray(start,end)]);at=end
 }
 const sorted=checkFiles(entries)
 requireThat(zip(sorted).equals(bytes));return sorted
}
function manifest(owner,bytes,entries){
 requireThat(owner&&/^[a-f0-9]{40}$/.test(owner.sourceSha)&&mobileReleaseFamily(owner.version)&&/^[1-9]\d{0,19}$/.test(owner.runId)&&/^[1-9]\d{0,9}$/.test(owner.attempt))
 return {kind:'cc-desk-mobile-site',schema:1,sourceSha:owner.sourceSha,version:owner.version,runId:owner.runId,attempt:owner.attempt,deployReady:false,
  archive:{name:ZIP_NAME,bytes:bytes.length,sha256:hash(bytes)},files:entries.map(([name,data])=>({name,bytes:data.length,sha256:hash(data)}))}
}
const encode=o=>JSON.stringify(o,null,2)+'\n'
export function packageSite(site,out,owner){
 const dir=lstatSync(out);requireThat(dir.isDirectory()&&!dir.isSymbolicLink());
 const entries=siteFiles(site),bytes=zip(entries),record=manifest(owner,bytes,entries)
 writeFileSync(join(out,ZIP_NAME),bytes,{flag:'wx'});writeFileSync(join(out,MANIFEST_NAME),encode(record),{flag:'wx'})
 return verifySite(out,owner)
}
export function verifySite(out,owner){
 const dir=lstatSync(out);requireThat(dir.isDirectory()&&!dir.isSymbolicLink());
 const bytes=read(join(out,ZIP_NAME),MAX+128*512),metadata=read(join(out,MANIFEST_NAME),256*1024)
 const record=manifest(owner,bytes,unzip(bytes));requireThat(metadata.equals(Buffer.from(encode(record))));return record
}
if (process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
 const [mode,out,site,extra]=process.argv.slice(2),owner={sourceSha:process.env.GITHUB_SHA,version:process.env.CC_DESK_RELEASE_VERSION,runId:process.env.GITHUB_RUN_ID,attempt:process.env.GITHUB_RUN_ATTEMPT}
 if(extra||!out||!['package','verify'].includes(mode)||(mode==='package')!==!!site)throw Error('Mobile release artifact usage')
 try{
  const result=mode==='package'?packageSite(resolve(site),resolve(out),owner):verifySite(resolve(out),owner)
  console.log('Verified mobile site archive SHA-256 '+result.archive.sha256+'; deployment not verified')
 }catch{
  console.error('Required mobile release archive or manifest failed verification')
  process.exitCode=1
 }
}
