import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';

function git(root,args,encoding='utf8'){
 const env={...process.env};
 for(const key of Object.keys(env))if(key.toUpperCase().startsWith('GIT_'))delete env[key];
 try{return execFileSync('git',['--no-optional-locks','-C',root,...args],{env,encoding,windowsHide:true,stdio:['ignore','pipe','pipe']});}
 catch(error){throw Error('Remote trial requires a readable Git checkout',{cause:error});}
}
const generatedEolPaths=new Set(['src-tauri/Cargo.toml','src-tauri/gen/schemas/desktop-schema.json','src-tauri/gen/schemas/windows-schema.json']);
function canonicalEol(bytes){
 const result=[];
 for(let i=0;i<bytes.length;i++)if(bytes[i]!==13||bytes[i+1]!==10)result.push(bytes[i]);
 return Buffer.from(result);
}
export function captureIdentity(directory){
 const root=fs.realpathSync(directory);
 if(fs.realpathSync(git(root,['rev-parse','--show-toplevel']).trim())!==root)throw Error('Remote trial requires exact Git root');
 const head=git(root,['rev-parse','--verify','HEAD']).trim();
 if(!/^[0-9a-f]{40}$/.test(head))throw Error('Invalid Git HEAD');
 try{
  git(root,['diff','--no-ext-diff','--no-textconv','--cached','--quiet','HEAD','--']);
  git(root,['diff','--no-ext-diff','--no-textconv','--quiet','HEAD','--']);
 }catch(error){throw Error('Remote trial requires clean tracked sources',{cause:error});}
 if(git(root,['ls-files','--others','--exclude-standard','-z']))throw Error('Remote trial requires clean untracked sources');
 const entries=git(root,['ls-files','--stage','-z']).split('\0').filter(Boolean);
 const files=entries.map(entry=>{
  const match=/^(100644|100755) [0-9a-f]{40} 0\t(.+)$/.exec(entry);
  if(!match)throw Error('Remote trial rejects symlinks, submodules and unresolved index entries');
  return match[2];
 });
 if(git(root,['ls-files','-v','-z']).split('\0').filter(Boolean).some(entry=>entry[0]!=='H'))throw Error('Remote trial rejects hidden index flags');
 const hash=createHash('sha256');
 for(const file of files){
  const absolute=path.join(root,file);
  if(!fs.lstatSync(absolute).isFile()||fs.realpathSync(absolute)!==absolute)throw Error('Remote trial rejects indirect source paths');
  let bytes=fs.readFileSync(absolute);
  // 只为原 Tauri 三个重写路径接受 CRLF/LF；内容与 HEAD 必须逐字节等价。
  if(generatedEolPaths.has(file)){
   bytes=canonicalEol(bytes);
   if(!bytes.equals(canonicalEol(git(root,['show',head+':'+file],null))))throw Error('Remote trial generated source differs from HEAD');
  }
  hash.update(file);hash.update('\0');hash.update(createHash('sha256').update(bytes).digest());hash.update('\0');
 }
 if(git(root,['rev-parse','HEAD']).trim()!==head)throw Error('Remote trial HEAD changed during capture');
 return Object.freeze({root,head,source:hash.digest('hex')});
}
export function identityEnvironment(env,identity){
 const child={...env};
 // 前端也执行 git；不能让继承的仓库选择覆盖已核本树身份。
 for(const key of Object.keys(child))if(key.toUpperCase().startsWith('GIT_'))delete child[key];
 return {...child,CC_DESK_BUILD_SHA:identity.head,GITHUB_SHA:identity.head};
}
export function verifyIdentity(identity){
 const current=captureIdentity(identity.root);
 if(current.head!==identity.head||current.source!==identity.source)throw Error('Remote trial source changed during build');
}
