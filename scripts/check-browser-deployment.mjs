import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {build} from 'esbuild';
import ts from 'typescript';
import {validateRegistrationExports} from './build-browser-registration-runtime.mjs';
const root=fs.realpathSync(fileURLToPath(new URL('..',import.meta.url)));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const fail=code=>{throw Error(code)};
function inside(base,file){const rel=path.relative(base,file);return rel!==''&&rel!=='..'&&!rel.startsWith(`..${path.sep}`)&&!path.isAbsolute(rel)}
function directory(input){const p=path.resolve(root,input);if(!inside(root,p))fail('PATH_POLICY');let current=root;for(const part of path.relative(root,p).split(path.sep)){current=path.join(current,part);if(fs.lstatSync(current).isSymbolicLink())fail('PATH_POLICY')}if(!inside(root,fs.realpathSync(p))||!fs.statSync(p).isDirectory())fail('PATH_POLICY');return p}
const report={localArtifactIntegrity:'FAIL',scope:'Only the fixed public artifacts listed in inventory',deployReady:false,failures:[],inventory:[],unverified:['independent provenance and provider authorization','Host/browser/RP identities and public configuration','HTTPS WebPKI and COOP/COEP headers','native GUI and real authenticator acceptance','actual deployment','HTML/JS dependency closure and behavior']};
function read(base,name,limit){
 let p=base;for(const part of name.split('/')){p=path.join(p,part);if(fs.lstatSync(p).isSymbolicLink())fail('PATH_POLICY')}
 const fd=fs.openSync(p,'r');try{const size=fs.fstatSync(fd).size;if(!fs.fstatSync(fd).isFile()||size<1||size>limit)fail('BYTE_LIMIT');const bytes=Buffer.alloc(size);if(fs.readSync(fd,bytes,0,size,0)!==size||fs.fstatSync(fd).size!==size)fail('CHANGED_DURING_READ');report.inventory.push({file:name,bytes:size,sha256:hash(bytes)});return bytes}finally{fs.closeSync(fd)}
}
function check(file,fn){try{return fn()}catch(e){report.failures.push({file,code:e.code==='ENOENT'?'MISSING':/^[A-Z_]+$/.test(e.message)?e.message:'INVALID_ARTIFACT'});return undefined}}
function metadata(entry,name,bytes,key='name'){if(entry?.[key]!==name||Object.hasOwn(entry,key==='name'?'path':'name'))fail('MANIFEST_FILES');if(!Number.isSafeInteger(entry.bytes)||entry.bytes!==bytes.length||!/^[a-f0-9]{64}$/.test(entry.sha256??'')||entry.sha256!==hash(bytes))fail('INTEGRITY_MISMATCH')}
function runtime(site,dir,kind,symbols,modules){
 const manifest=JSON.parse(read(site,`${dir}/manifest.json`,262144));
 if(manifest.kind!==kind)fail('RUNTIME_KIND');
 const names=['fixture_bg.wasm','fixture_bg.js'];
 if(!Array.isArray(manifest.files)||manifest.files.length!==2||names.some(n=>manifest.files.filter(f=>f.name===n).length!==1))fail('MANIFEST_FILES');
 const contents=names.map(name=>{const bytes=read(site,`${dir}/${name}`,name.endsWith('wasm')?8388608:1048576);metadata(manifest.files.find(f=>f.name===name),name,bytes);return bytes});
 if(contents.some(b=>/TEST_ONLY/i.test(b.toString('latin1'))))fail('TEST_ONLY');
 const module=new WebAssembly.Module(contents[0]),exports=WebAssembly.Module.exports(module),imports=WebAssembly.Module.imports(module);
 if(kind==='OriginalBrowserRegistrationRuntime')validateRegistrationExports(exports);
 else if(!exports.some(e=>e.name==='memory'&&e.kind==='memory')||symbols.some(n=>!exports.some(e=>e.name===n&&e.kind==='function'))||exports.some(e=>e.name.startsWith('browser_')&&!symbols.includes(e.name)))fail('EXPORT_POLICY');
 if(!Array.isArray(manifest.exports)||JSON.stringify([...manifest.exports].sort())!==JSON.stringify(exports.map(e=>e.name).sort()))fail('EXPORT_MANIFEST');
 if(imports.some(i=>i.kind!=='function'||!modules.includes(i.module)||/TEST_ONLY/i.test(i.name))||modules.some(n=>!imports.some(i=>i.module===n)))fail('IMPORT_POLICY');
 if(manifest.imports&&JSON.stringify(manifest.imports)!==JSON.stringify(imports))fail('IMPORT_MANIFEST');
}
async function main(){
 const args=process.argv.slice(2),options={};for(let i=0;i<args.length;i+=2){if(!['--site','--source-manifest-dir'].includes(args[i])||!args[i+1]||args[i] in options)fail('USAGE');options[args[i]]=args[i+1]}
 if(!options['--site'])fail('USAGE');const site=directory(options['--site']);
 for(const name of ['browser-remote.html','browser-registration.html','browser-profile.js'])check(name,()=>{const b=read(site,name,2097152);if(/TEST_ONLY/i.test(b.toString('utf8')))fail('TEST_ONLY')});
 check('remote-runtime',()=>runtime(site,'remote-runtime','OriginalBrowserRuntime',['browser_client_start','browser_client_event','browser_tls_start','browser_tls_poll','browser_tls_cancel','browser_tls_event'],['cc_desk_browser_client_v1','cc_desk_browser_owner_v1','cc_desk_browser_grant_v1','cc_desk_browser_tls_v1','./fixture_bg.js']));
 check('remote-registration-runtime',()=>runtime(site,'remote-registration-runtime','OriginalBrowserRegistrationRuntime',[],['cc_desk_browser_owner_v1','cc_desk_browser_tls_v1','cc_desk_browser_registration_v1','./fixture_bg.js']));
 const source=check('browser-registration.js',()=>read(site,'browser-registration.js',1048576));
 if(source){try{
  if(!options['--source-manifest-dir'])fail('SOURCE_MANIFEST_REQUIRED');
  const manifest=JSON.parse(read(directory(options['--source-manifest-dir']),'manifest.json',262144));
  if(manifest.version!==1||JSON.stringify(manifest.exports)!==JSON.stringify(['readApprovedBrowserRegistrationInputs']))fail('SOURCE_EXPORT');
  metadata(manifest.artifact,'browser-registration.js',source,'path');
  if(/TEST_ONLY/i.test(source.toString('utf8')))fail('TEST_ONLY');
  const file=ts.createSourceFile('browser-registration.js',source.toString('utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
  function visit(n){if(ts.isImportDeclaration(n)||ts.isExportDeclaration(n)&&n.moduleSpecifier||ts.isCallExpression(n)&&n.expression.kind===ts.SyntaxKind.ImportKeyword)fail('SOURCE_IMPORT');ts.forEachChild(n,visit)}visit(file);
  const parsed=await build({stdin:{contents:source.toString('utf8'),loader:'js',sourcefile:'browser-registration.js'},bundle:false,format:'esm',write:false,metafile:true,logLevel:'silent'});
  const output=Object.values(parsed.metafile.outputs)[0];if(output.imports.length||JSON.stringify(output.exports)!==JSON.stringify(['readApprovedBrowserRegistrationInputs']))fail('SOURCE_EXPORT');
 }catch(e){report.failures.push({file:'browser-registration.js',code:/^[A-Z_]+$/.test(e.message)?e.message:'INVALID_SOURCE'})}}
 if(report.failures.length===0)report.localArtifactIntegrity='PASS';
}
try{await main()}catch(e){report.failures.push({file:'arguments',code:e.code==='ENOENT'?'MISSING':/^[A-Z_]+$/.test(e.message)?e.message:'INVALID_ARTIFACT'})}
console.log(JSON.stringify(report,null,2));process.exitCode=report.failures.length?1:0;
