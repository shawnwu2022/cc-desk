import fs from 'node:fs';
import {captureIdentity,verifyIdentity,identityEnvironment} from './remote-trial-identity.mjs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {root,trialTarget,selectedTools,offlineEnvironment,run} from './remote-trial-build-policy.mjs';

export function buildArguments(args,cargo){
 if(args.some(arg=>!['--debug','--no-bundle'].includes(arg))||new Set(args).size!==args.length)throw Error('Only --debug and --no-bundle are accepted; installer bundling is not enabled');
 return ['node_modules/@tauri-apps/cli/tauri.js','build','--config','src-tauri/tauri.remote-trial.conf.json','--features','remote-host','--no-sign','--no-bundle','--runner',cargo,...args.filter(arg=>arg!=='--no-bundle')];
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 if(process.platform!=='win32'||process.arch!=='x64')throw Error('Remote trial build currently supports Windows x64 only');
 const identity=captureIdentity(root);
 const tools=selectedTools(process.env),env=identityEnvironment(offlineEnvironment(process.env,tools),identity);
 env.CARGO_TARGET_DIR=path.join(trialTarget,'native');
 const runnerDir=path.join(trialTarget,'tools');
 fs.mkdirSync(runnerDir,{recursive:true});
 const runner=path.join(runnerDir,'remote-trial-cargo.exe');
 run(tools.CC_DESK_TRIAL_RUSTC,['--edition=2024','--crate-name','cc_desk_trial_cargo','-D','warnings','scripts/remote-trial-cargo.rs','-o',runner],{...env,CC_DESK_TRIAL_BUILD_ROOT:root,CC_DESK_TRIAL_BUILD_CARGO:tools.CC_DESK_TRIAL_CARGO});
 const args=buildArguments(process.argv.slice(2),runner);
 run(process.execPath,args,env);
 verifyIdentity(identity);
 console.log(`Local remote-trial executable (not an installer or release): ${env.CARGO_TARGET_DIR}`);
}
