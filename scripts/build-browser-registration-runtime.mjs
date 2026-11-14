import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';

export const registrationSymbols=Object.freeze([
  'browser_registration_start','browser_registration_poll',
  'browser_registration_cancel','browser_registration_event',
]);
const inside=(root,file)=>{const relative=path.relative(root,file);return relative!==''&&!relative.startsWith(`..${path.sep}`)&&relative!=='..'&&!path.isAbsolute(relative);};
const hash=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const relative=(root,file)=>path.relative(root,file).replaceAll('\\','/');

export function validateRegistrationExports(exports){
  const expected=[['memory','memory'],['__wbindgen_start','function'],...registrationSymbols.map(name=>[name,'function'])];
  if(expected.some(([name,kind])=>!exports.some(value=>value.name===name&&value.kind===kind))
    ||exports.some(({name})=>/TEST_ONLY/i.test(name)||(name.startsWith('browser_')&&!registrationSymbols.includes(name)))){
    throw Error('browser registration runtime export policy');
  }
}

export function selectRegistrationArtifacts(rows,root,target){
  function select(name){
    const matches=rows.filter(row=>row.reason==='compiler-artifact'&&row.target.name===name);
    if(matches.length!==1)throw Error(`missing or ambiguous exact registration artifact ${name}`);
    const row=matches[0],files=row.filenames.filter(file=>file.endsWith('.rlib'));
    if(files.length!==1)throw Error(`missing or ambiguous exact registration artifact ${name}`);
    if(row.profile?.test!==false||!row.target.kind.includes('lib'))throw Error('registration runtime refuses test artifact');
    if(!inside(path.join(target,'wasm32-unknown-unknown/debug'),path.resolve(files[0])))throw Error('registration runtime requires exact wasm artifact');
    if(name==='cc_desk_remote_security'){
      if(path.resolve(row.target.src_path)!==path.join(root,'remote/crates/security/src/lib.rs'))throw Error('registration runtime source lineage');
      if(!row.features.includes('experimental'))throw Error('registration runtime experimental feature required');
    }
    return files[0];
  }
  return {security:select('cc_desk_remote_security'),rustls:select('rustls')};
}

// 路径和现存祖先都必须在项目内，不能经 junction 将产物或临时文件写出项目。
export function projectPath(root,input){
  const resolved=path.resolve(root,input);
  if(!inside(root,resolved))throw Error('registration build path must stay inside the project');
  let ancestor=resolved;
  while(!fs.existsSync(ancestor))ancestor=path.dirname(ancestor);
  const real=fs.realpathSync(ancestor);
  if(real!==root&&!inside(root,real))throw Error('registration build path leaves project through a symlink');
  return resolved;
}

export function buildBrowserRegistrationRuntime(){
  const root=fs.realpathSync(fileURLToPath(new URL('../',import.meta.url)));
  const audit=projectPath(root,'.audit/build-browser-registration-runtime');
  const target=projectPath(root,process.env.CARGO_TARGET_DIR??path.join(audit,'target'));
  const out=projectPath(root,'public/remote-registration-runtime');
  fs.mkdirSync(audit,{recursive:true});
  const runDir=fs.mkdtempSync(path.join(audit,'build-'));
  const temp=path.join(runDir,'temp'),stage=path.join(runDir,'generated');
  fs.mkdirSync(temp);fs.mkdirSync(stage);
  const env={...process.env,TEMP:temp,TMP:temp,TMPDIR:temp,CARGO_TARGET_DIR:target,CARGO_BUILD_JOBS:'2'};
  const commands=[];
  function run(label,exe,args){
    commands.push({exe,args});
    const result=spawnSync(exe,args,{cwd:root,env,encoding:'utf8',maxBuffer:32*1024*1024});
    fs.writeFileSync(path.join(runDir,`${label}.stdout`),result.stdout??'');
    fs.writeFileSync(path.join(runDir,`${label}.stderr`),result.stderr??'');
    if(result.status!==0){process.stderr.write(result.stderr??'');throw Error(`${label} failed (${result.status}): ${result.error?.message??''}`);}
    return result.stdout;
  }
  const bindgen=process.env.WASM_BINDGEN??'wasm-bindgen';
  const toolchain={rustc:run('rustc-version','rustc',['-Vv']).trim(),cargo:run('cargo-version','cargo',['-V']).trim(),wasmBindgen:run('wasm-bindgen-version',bindgen,['--version']).trim()};
  const walk=dir=>fs.readdirSync(path.join(root,dir),{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?walk(path.join(dir,entry.name)):entry.name.endsWith('.rs')?[path.join(dir,entry.name)]:[]);
  const sourceFiles=['remote/browser-registration-runtime.rs','scripts/build-browser-registration-runtime.mjs',
    'remote/Cargo.lock','remote/Cargo.toml','remote/crates/security/Cargo.toml','protocol/remote/inner/v1/Cargo.toml',
    ...walk('remote/crates/security/src'),...walk('protocol/remote/inner/v1/src')].sort();
  const sources=sourceFiles.map(file=>({file:file.replaceAll('\\','/'),sha256:hash(path.join(root,file))}));
  const build=run('cargo-artifacts','cargo',['build','--offline','--locked','--manifest-path','remote/Cargo.toml',
    '-p','cc-desk-remote-security','--no-default-features','--features','experimental','--target','wasm32-unknown-unknown','--lib','--message-format=json']);
  const rows=build.trim().split(/\r?\n/).map(line=>JSON.parse(line));
  const artifacts=selectRegistrationArtifacts(rows,root,target);
  for(const file of Object.values(artifacts))projectPath(root,file);
  const raw=path.join(runDir,'registration-runtime.wasm');
  const args=['--edition=2024','--crate-type=bin','--target=wasm32-unknown-unknown','--crate-name=cc_desk_browser_registration_runtime',
    '-L',`dependency=${path.dirname(artifacts.rustls)}`,'-L',`dependency=${path.join(target,'debug/deps')}`,
    '--extern',`cc_desk_remote_security=${artifacts.security}`,'remote/browser-registration-runtime.rs','-o',raw,
    '-C','link-arg=--no-entry','-C','link-arg=--export-memory'];
  for(const symbol of registrationSymbols)args.push('-C',`link-arg=--export=${symbol}`);
  run('rustc-link','rustc',args);
  run('wasm-bindgen',bindgen,['--target','bundler','--keep-lld-exports','--no-typescript','--out-name','fixture','--out-dir',stage,raw]);
  const wasm=path.join(stage,'fixture_bg.wasm'),module=new WebAssembly.Module(fs.readFileSync(wasm));
  const exports=WebAssembly.Module.exports(module);
  validateRegistrationExports(exports);
  const imports=WebAssembly.Module.imports(module);
  if(imports.some(value=>/TEST_ONLY/i.test(value.module+value.name))
    ||!imports.some(value=>value.module==='cc_desk_browser_registration_v1')
    ||!imports.some(value=>value.module==='./fixture_bg.js'))throw Error('browser registration runtime import policy');
  if(sources.some(({file,sha256})=>hash(path.join(root,file))!==sha256))throw Error('registration runtime sources changed during build');
  const files=['fixture_bg.wasm','fixture_bg.js'].map(name=>({name,bytes:fs.statSync(path.join(stage,name)).size,sha256:hash(path.join(stage,name))}));
  const manifest={kind:'OriginalBrowserRegistrationRuntime',toolchain,commands,
    inputs:Object.values(artifacts).map(file=>({file:relative(root,file),sha256:hash(file)})),
    cargoArtifacts:{file:relative(root,path.join(runDir,'cargo-artifacts.stdout')),sha256:hash(path.join(runDir,'cargo-artifacts.stdout'))},
    sources,files,exports:exports.map(value=>value.name),imports};
  fs.writeFileSync(path.join(stage,'manifest.json'),JSON.stringify(manifest,null,2)+'\n');
  // 所有检查通过后才复制三项部署产物；fixture.js 的自动启动入口不属于独立 Worker 合约。
  fs.mkdirSync(out,{recursive:true});
  for(const name of [...files.map(file=>file.name),'manifest.json'])fs.copyFileSync(path.join(stage,name),projectPath(root,path.join(out,name)));
  console.log(JSON.stringify({out:relative(root,out),files}));
  return manifest;
}

if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))buildBrowserRegistrationRuntime();
