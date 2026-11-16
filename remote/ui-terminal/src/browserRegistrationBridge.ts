import {createBrowserRegistrationProducer} from './browserRegistration.ts';
import {createBrowserRegistrationTlsSignerBroker,type BrowserTlsWorkerBridge} from './browserTlsSigner.ts';

export type BrowserRegistrationBridge=Readonly<{port:MessagePort;cancel:SharedArrayBuffer;origin:string;deadline:number;signer:BrowserTlsWorkerBridge}>;
type Keys=Readonly<{tlsKey:CryptoKey;grantKey:CryptoKey;relayKey:CryptoKey}>;
const now=()=>performance.timeOrigin+performance.now();
const invalid=()=>Error('browser registration bridge unavailable');
/** 独立获准的现有 key handles；不会生成密钥、建立信任或持久安装。 */
export function createBrowserRegistrationBridge(options:Readonly<{origin:string;keys:Keys;signal:AbortSignal;setupDeadline:number;registrationDeadline:number}>){
 const {origin,signal,setupDeadline,registrationDeadline:deadline}=options;
 if(!(signal instanceof AbortSignal)||signal.aborted||!Number.isFinite(deadline)||deadline<=now()||deadline-now()>120000||setupDeadline>deadline)throw invalid();
 const page=document,controller=new AbortController();let keys:Keys|undefined=Object.freeze({...options.keys});
 const producer=createBrowserRegistrationProducer(origin,controller.signal,deadline);
 let signer:ReturnType<typeof createBrowserRegistrationTlsSignerBroker>;
 try{signer=createBrowserRegistrationTlsSignerBroker(keys.tlsKey,origin,{setupDeadline,signal:controller.signal});}catch(error){producer.close();throw error;}
 const channel=new MessageChannel(),port=channel.port1,cancel=new SharedArrayBuffer(8),state=new Int32Array(cancel);
 let expected=1,pending:Promise<Uint8Array>|undefined,closed=false,draining=false,mainDone=false,workerDone=false,finished=false,failed=false;
 let resolve!:()=>void,reject!:(e:Error)=>void;
 const completion=new Promise<void>((a,b)=>{resolve=a;reject=b});void completion.catch(()=>{});
 function live(){return !closed&&!signal.aborted&&!controller.signal.aborted&&document===page&&location.origin===origin&&page.visibilityState==='visible'&&globalThis.top===globalThis.self&&globalThis.isSecureContext&&globalThis.crossOriginIsolated&&now()<deadline&&Atomics.load(state,0)===0;}
 function retire(){
  if(finished||!mainDone||pending||(!workerDone&&Atomics.load(state,1)!==0))return;
  finished=true;clearInterval(timer);signal.removeEventListener('abort',close);page.removeEventListener('visibilitychange',visibility);page.removeEventListener('freeze',close);globalThis.removeEventListener('pagehide',close);
  try{port.postMessage(failed?'retirement-failed':'retired');}catch{failed=true;}port.onmessage=null;port.onmessageerror=null;port.close();keys=undefined;
  if(failed)reject(Error('browser registration retirement uncertain'));else resolve();
 }
 function close(){
  if(!closed){closed=true;Atomics.store(state,0,1);controller.abort();producer.close();signer.close();try{port.postMessage('close');}catch{failed=true;}}
  if(!draining){draining=true;void Promise.all([producer.waitClosed(),signer.waitClosed()]).then(()=>{mainDone=true;retire();},()=>{failed=true;mainDone=true;retire();});}retire();
 }
 function visibility(){if(page.visibilityState!=='visible')close();}
 port.onmessage=(event:MessageEvent<unknown>)=>{
  const value=event.data;
  if(value==='retired'||value==='retirement-failed'){workerDone=true;if(value==='retirement-failed')failed=true;close();return;}
  if(value==='close'){close();return;}
  try{
   if(!live()||pending||!value||typeof value!=='object'||Reflect.ownKeys(value).length!==3)throw invalid();
   const data=value as {kind:string;seq:number;bytes:Uint8Array};
   if(data.kind!=='request'||data.seq!==expected||expected>2||!(data.bytes instanceof Uint8Array)||data.bytes.length<1||data.bytes.length>4096)throw invalid();
   const seq=expected++,bytes=data.bytes.slice();
   const work=seq===1?producer.create(bytes,keys!):producer.confirm(bytes);pending=work;
   void work.then(bytes=>{if(!live()||!(bytes instanceof Uint8Array)||bytes.length<1||bytes.length>16384){close();return;}const output=bytes.slice();port.postMessage({kind:'response',seq,bytes:output},[output.buffer]);},close).finally(()=>{pending=undefined;retire();});
  }catch{close();}
 };
 port.onmessageerror=()=>{failed=true;close();};port.start();
 const timer=setInterval(()=>{if(!live())close();},100);
 signal.addEventListener('abort',close,{once:true});page.addEventListener('visibilitychange',visibility);page.addEventListener('freeze',close);globalThis.addEventListener('pagehide',close);
 if(!live())close();
 return Object.freeze({worker:Object.freeze({port:channel.port2,cancel,origin,deadline,signer:signer.worker}),close,waitClosed:()=>completion});
}
