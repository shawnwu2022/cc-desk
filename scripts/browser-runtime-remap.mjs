import os from 'node:os';
import path from 'node:path';

const separator=String.fromCharCode(31);

// Every compiler command that can contribute to distributable browser WASM must
// use the same neutral names. Raw compiler paths belong in local build evidence.
export function browserRuntimeCompilerEnvironment(root,target,env=process.env){
 const home=os.homedir();
 const sources=[
  [home,'/build/user'],
  [os.tmpdir(),'/build/tmp'],
  [root,'/build/source'],
  [env.CARGO_HOME||path.join(home,'.cargo'),'/build/cargo'],
  [target,'/build/target'],
 ];
 const mappings=new Map();
 for(const [value,dest] of sources){
  const source=path.resolve(root,value);
  if(source===path.parse(source).root)throw Error('Browser build path must not be a filesystem root');
  for(const prefix of new Set([source,source.replaceAll('\\','/')]))mappings.set(prefix,dest);
 }
 const flags=[...mappings].map(([prefix,dest])=>`--remap-path-prefix=${prefix}=${dest}`);
 const existing=env.CARGO_ENCODED_RUSTFLAGS===undefined
  ? (env.RUSTFLAGS??'').split(/\s+/).filter(Boolean)
  : env.CARGO_ENCODED_RUSTFLAGS.split(separator).filter(Boolean);
 return {flags,env:{...env,CARGO_ENCODED_RUSTFLAGS:[...existing,...flags].join(separator)}};
}
