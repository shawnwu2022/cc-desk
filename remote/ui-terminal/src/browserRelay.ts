import type {BrowserTlsTransport} from './browserTlsWorker.ts';
export type BrowserRelaySigningRequest=Readonly<{origin:string;keyId:Uint8Array;role:2;transcript:Uint8Array}>;
export type BrowserRelayIdentity=Readonly<{keyId:Uint8Array;sign:(request:BrowserRelaySigningRequest,signal:AbortSignal)=>Promise<Uint8Array>}>;
export const RENDEZVOUS_SUBPROTOCOL='cc-desk-relay-rendezvous-v1';
export type BrowserRelayMode='legacy'|'rendezvous-v1';
export type BrowserRelaySocket=Pick<WebSocket,'readyState'|'binaryType'|'bufferedAmount'|'send'|'close'|'addEventListener'|'removeEventListener'>&Readonly<{protocol?:string}>;
type BrowserRelayOptions=Readonly<{origin:string;tunnel:Uint8Array;identity:BrowserRelayIdentity;setupDeadline:number;ioBudgetMs:number;mode?:BrowserRelayMode;socket:BrowserRelaySocket;signal?:AbortSignal}>;
type BrowserRelayOwner=Readonly<{ready:Promise<BrowserTlsTransport>;close():void;waitClosed():Promise<void>}>;
export function createBrowserRegistrationRelay(options:BrowserRelayOptions&Readonly<{registrationDeadline:number;signal:AbortSignal}>):BrowserRelayOwner{
 const remaining=options.registrationDeadline-performance.timeOrigin-performance.now();
 if(!Number.isFinite(remaining)||remaining<=0||remaining>120000||options.ioBudgetMs<30||!(options.signal instanceof AbortSignal)||options.signal.aborted)throw Error('registration relay original lifetime');
 return createRelay(options,options.registrationDeadline);
}
export function createBrowserRelay(options:BrowserRelayOptions):BrowserRelayOwner{return createRelay(options);}
type DataKeepalivePolicy={maxEnd:number;end?:number;used:boolean};
export type BrowserDataKeepaliveActivation=Readonly<{activate(end:number):void}>;
// 仅原 transport 实例的一次保活机制，不能作为认证或授权事实。
export function createBrowserDataKeepaliveRelay(options:BrowserRelayOptions&Readonly<{maxDeadline:number}>):Readonly<{owner:BrowserRelayOwner;activation:BrowserDataKeepaliveActivation}>{
 if(!Number.isSafeInteger(options.maxDeadline)||!Number.isSafeInteger(options.setupDeadline)||options.maxDeadline<options.setupDeadline||options.maxDeadline>performance.timeOrigin+performance.now()+900000||options.ioBudgetMs<2000)throw Error('data keepalive lifetime');
 const relay=createRelay(options,undefined,{maxEnd:options.maxDeadline,used:false});
 return Object.freeze({owner:relay,activation:relay.activation!});
}
function createRelay(options:BrowserRelayOptions,registrationEnd?:number,dataPolicy?:DataKeepalivePolicy):BrowserRelayOwner&Readonly<{activation?:BrowserDataKeepaliveActivation}>{
 const {origin,socket,setupDeadline,ioBudgetMs,signal}=options;
 const anchorPerf=performance.now(),anchorWall=Date.now(),epoch=performance.timeOrigin+anchorPerf;
 let lastPerf=anchorPerf,lastWall=anchorWall,high=epoch;
 const now=()=>{if(registrationEnd===undefined&&!dataPolicy)return performance.timeOrigin+performance.now();const p=performance.now(),w=Date.now();if(!Number.isFinite(p)||!Number.isFinite(w)||(dataPolicy&&(!Number.isSafeInteger(w)||p<0||p>Number.MAX_SAFE_INTEGER||!Number.isFinite(epoch)||epoch>Number.MAX_SAFE_INTEGER))||p<lastPerf||w<lastWall)return high=Infinity;lastPerf=p;lastWall=w;return high=Math.max(high,epoch+Math.max(p-anchorPerf,w-anchorWall));};
 const cap=(end:number)=>dataPolicy?Math.min(end,dataPolicy.end??setupDeadline):registrationEnd===undefined?end:Math.min(end,registrationEnd);
 const url=new URL(origin),tunnel=Uint8Array.from(options.tunnel),keyId=Uint8Array.from(options.identity.keyId),sign=options.identity.sign;
 if(origin.length>512||!/^[\x00-\x7f]+$/.test(origin)||origin.includes('%')||origin.includes('\\')||url.href!==origin||url.protocol!=='wss:'||url.username||url.password||url.search||url.hash||!url.hostname||url.port==='0'||tunnel.length!==16||!tunnel.some(Boolean)||keyId.length!==16||typeof sign!=='function'||!Number.isFinite(setupDeadline)||setupDeadline<=now()||setupDeadline>now()+10000||!Number.isInteger(ioBudgetMs)||ioBudgetMs<1||ioBudgetMs>30000)throw Error('browser relay configuration');
 const mode=options.mode??'legacy';if(mode!=='legacy'&&mode!=='rendezvous-v1')throw Error('browser relay mode');
 const controller=new AbortController();let phase:'challenge'|'signing'|'accepted'|'joining'|'data'|'closed'='challenge',failed=false,physicalClosed=socket.readyState===3;
 let signer:Promise<void>|undefined,write:Promise<void>|undefined,read:{maximum:number;resolve:(v:Uint8Array|null)=>void;reject:(e:Error)=>void}|undefined;
 let lastRead=now();
 let applicationWait:Promise<void>|undefined,writeKind:'heartbeat'|'application'|undefined;
 let queue:Uint8Array[]=[],queuedBytes=0,offset=0,readDeadline=cap(setupDeadline),timer:ReturnType<typeof setTimeout>|undefined,heartbeat:number|undefined;
 const empty=new Uint8Array(24);empty.set([1,1,0,1]);empty.set(tunnel,4);
 let resolveReady!:(v:BrowserTlsTransport)=>void,rejectReady!:(e:Error)=>void,resolveClosed!:()=>void;
 const ready=new Promise<BrowserTlsTransport>((a,b)=>{resolveReady=a;rejectReady=b;}),closed=new Promise<void>(a=>{resolveClosed=a;});void ready.catch(()=>{});
 let retired=false;const drainWaiters=new Set<()=>void>();
 function retire(){if(phase!=='closed'||!physicalClosed||signer||write||applicationWait||retired)return;retired=true;socket.removeEventListener('message',message);socket.removeEventListener('close',ended);socket.removeEventListener('error',broken);signal?.removeEventListener('abort',close);resolveClosed();}
 function stop(error?:Error){if(error)failed=true;if(phase==='closed')return;phase='closed';controller.abort();if(timer!==undefined)clearTimeout(timer);queue=[];queuedBytes=0;offset=0;rejectReady(error??Error('browser relay closed'));if(read){const original=read;read=undefined;if(failed)original.reject(error??Error('browser relay failed'));else original.resolve(null);}for(const wake of drainWaiters)wake();try{socket.close();}catch{failed=true;}retire();}
 function close(){stop();}
 function broken(){stop(Error('browser relay socket'));}
 function ended(){physicalClosed=true;stop();retire();}
 function pulse(){
  const current=now();if(signal?.aborted||current>=readDeadline)throw Error('registration relay deadline/cancelled');
  if(heartbeat!==undefined&&phase==='data'&&current>=heartbeat){heartbeat=current+(dataPolicy?500:ioBudgetMs/3);if(!write&&!applicationWait&&socket.readyState===1&&socket.bufferedAmount===0){if(dataPolicy){void ownedWrite(empty.slice(),true).catch(()=>{});}else{socket.send(empty.slice());if(now()>=readDeadline)throw Error('registration relay deadline');}}}
 }
 function arm(){if(timer!==undefined)clearTimeout(timer);if(phase==='closed')return;const target=Math.min(readDeadline,heartbeat??Infinity);timer=setTimeout(()=>{try{if(registrationEnd!==undefined||dataPolicy)pulse();else if(now()>=readDeadline){stop(Error('browser relay deadline'));return;}arm();}catch{stop(Error('browser relay deadline'));}},Math.max(0,target-now()));}
 function admitted(){if((registrationEnd!==undefined||dataPolicy!==undefined)&&(signal?.aborted||now()>=readDeadline))throw Error('registration relay deadline');phase='data';lastRead=now();readDeadline=cap(lastRead+ioBudgetMs);if(registrationEnd!==undefined)heartbeat=now()+ioBudgetMs/3;arm();resolveReady(transport);}
 function deliver(){if(!read||!queue.length)return;const original=read;read=undefined;const first=queue[0]!,count=Math.min(original.maximum,first.length-offset),result=first.slice(offset,offset+count);offset+=count;queuedBytes-=count;if(offset===first.length){queue.shift();offset=0;}original.resolve(result);}
 function ownedWrite(frame:Uint8Array,heartbeatWrite=false,callerEnd=Infinity):Promise<void>{const end=Math.min(readDeadline,now()+ioBudgetMs,callerEnd);
   const wait=()=>new Promise<void>(resolve=>{let poll:ReturnType<typeof setTimeout>;const wake=()=>{clearTimeout(poll);drainWaiters.delete(wake);resolve();};drainWaiters.add(wake);poll=setTimeout(wake,Math.min(5,Math.max(0,cap(end)-now())));});
   const check=()=>{if(phase!=='data'||socket.readyState!==1||now()>=cap(end))throw Error('browser relay write deadline/closed');};
   const operation=Promise.resolve().then(async()=>{
    while(socket.bufferedAmount+frame.length>131072){check();await wait();}
    if(phase!=='data'||socket.readyState!==1||now()>=cap(end))throw Error('browser relay write closed');socket.send(frame);
    while(socket.bufferedAmount!==0){check();await wait();}check();
   }).catch(error=>{stop(Error('browser relay write'));throw error;}).finally(()=>{write=undefined;writeKind=undefined;retire();});write=operation;writeKind=heartbeatWrite?'heartbeat':'application';return operation;}
 const transport:BrowserTlsTransport=Object.freeze({
  read(maximum:number){if((registrationEnd!==undefined||dataPolicy!==undefined)&&(signal?.aborted||now()>=readDeadline))stop(Error('registration relay deadline'));if(!Number.isInteger(maximum)||maximum<1||maximum>16384||read)return Promise.reject(Error('browser relay read bounds/concurrency'));if(phase==='closed')return failed?Promise.reject(Error('browser relay failed')):Promise.resolve(null);if(phase!=='data')return Promise.reject(Error('browser relay not admitted'));const result=new Promise<Uint8Array|null>((resolve,reject)=>{read={maximum,resolve,reject};});deliver();return result;},
  write(bytes:Uint8Array){if(phase!=='data'||applicationWait||(write&&writeKind!=='heartbeat')||!(bytes instanceof Uint8Array)||bytes.length<1||bytes.length>16384)return Promise.reject(Error('browser relay write bounds/concurrency'));
   const frame=new Uint8Array(24+bytes.length);frame.set([1,1,0,1]);frame.set(tunnel,4);new DataView(frame.buffer).setUint32(20,bytes.length);frame.set(bytes,24);
   if(write){
    // 最多一个原应用调用等待原 heartbeat；固定调用期限，不另发帧或重置预算。
    const original=write,end=Math.min(readDeadline,now()+ioBudgetMs);
    const waiting=Promise.resolve().then(async()=>{await original;if(phase!=='data'||now()>=cap(end))throw Error('browser relay waiting write closed');await ownedWrite(frame,false,end);}).catch(error=>{stop(Error('browser relay waiting write'));throw error;}).finally(()=>{applicationWait=undefined;retire();});
    applicationWait=waiting;return waiting;
   }
   return ownedWrite(frame);
  },close,waitClosed:()=>closed,
 });
 function message(event:Event){try{
  if(phase==='closed')return;if(((registrationEnd!==undefined||dataPolicy!==undefined)&&signal?.aborted)||now()>=readDeadline)throw Error('browser relay deadline');
  const data=(event as MessageEvent).data;if(!(data instanceof ArrayBuffer)||data.byteLength>65536)throw Error('browser relay binary bounds');const bytes=new Uint8Array(data);
  if(phase==='challenge'){
   if(mode==='rendezvous-v1'&&socket.protocol!==RENDEZVOUS_SUBPROTOCOL)throw Error('browser relay negotiation');
   if(bytes.length!==66||bytes[0]!==1||bytes[1]!==0)throw Error('browser relay challenge');phase='signing';
   const prefix=new TextEncoder().encode('CC_DESK_RELAY_ADMISSION\0\x01'),name=new TextEncoder().encode(origin),transcript=new Uint8Array(prefix.length+2+name.length+64+16+1);transcript.set(prefix);new DataView(transcript.buffer).setUint16(prefix.length,name.length);transcript.set(name,prefix.length+2);let p=prefix.length+2+name.length;transcript.set(bytes.subarray(2),p);p+=64;transcript.set(keyId,p);transcript[p+16]=2;
   const operation=Promise.resolve().then(()=>{if(phase!=='signing'||controller.signal.aborted||now()>=readDeadline)throw Error('browser relay signer cancelled');return sign(Object.freeze({origin,keyId:keyId.slice(),role:2,transcript}),controller.signal);}).then(signature=>{if(phase!=='signing'||now()>=readDeadline)return;if(!(signature instanceof Uint8Array)||signature.length!==64||socket.readyState!==1||socket.bufferedAmount+83>131072)throw Error('browser relay proof');const proof=new Uint8Array([1,1,...keyId,2,...signature]);phase='accepted';socket.send(proof);}).catch(()=>stop(Error('browser relay signer'))).finally(()=>{signer=undefined;retire();});signer=operation;return;
  }
  if(phase==='accepted'){if(bytes.length!==2||bytes[0]!==1||bytes[1]!==2)throw Error('browser relay acceptance');
   if(mode==='rendezvous-v1'){if(socket.protocol!==RENDEZVOUS_SUBPROTOCOL||socket.readyState!==1||socket.bufferedAmount+18>131072)throw Error('browser relay join');phase='joining';socket.send(new Uint8Array([2,0,...tunnel]));return;}
   admitted();return;
  }
  if(phase==='joining'){if(bytes.length!==18||bytes[0]!==2||bytes[1]!==1||!tunnel.every((v,i)=>bytes[i+2]===v))throw Error('browser relay readiness');admitted();return;}
  if(phase!=='data'||bytes.length<4||bytes[0]!==1||bytes[1]!==1)throw Error('browser relay outer header');const view=new DataView(bytes.buffer),count=view.getUint16(2);if(count>32)throw Error('browser relay outer count');let cursor=4;const frames:Uint8Array[]=[];
  for(let i=0;i<count;i++){if(cursor+20>bytes.length)throw Error('browser relay outer length');if(!tunnel.every((v,j)=>bytes[cursor+j]===v)||frames.length!==0)throw Error('browser relay route');const end=cursor+20+view.getUint32(cursor+16);if(end>bytes.length)throw Error('browser relay payload length');frames.push(bytes.subarray(cursor+20,end));cursor=end;}
  if(cursor!==bytes.length||queue.length+frames.filter(frame=>frame.length!==0).length>4||queuedBytes+frames.reduce((n,b)=>n+b.length,0)>65536)throw Error('browser relay incoming backpressure');if(registrationEnd!==undefined||dataPolicy)pulse();for(const frame of frames)if(frame.length){queue.push(frame.slice());queuedBytes+=frame.length;}lastRead=now();readDeadline=cap(lastRead+ioBudgetMs);arm();deliver();
 }catch{stop(Error('browser relay message'));}}
 socket.binaryType='arraybuffer';socket.addEventListener('message',message);socket.addEventListener('close',ended);socket.addEventListener('error',broken);signal?.addEventListener('abort',close,{once:true});arm();if(signal?.aborted||physicalClosed)close();
 const activation=dataPolicy?Object.freeze({activate(end:number){
  const current=now();if(dataPolicy.used||phase!=='data'||current>=setupDeadline||current>=readDeadline||!Number.isSafeInteger(end)||end<=current||end>dataPolicy.maxEnd){stop(Error('data keepalive activation'));throw Error('data keepalive activation');}
  dataPolicy.used=true;dataPolicy.end=end;readDeadline=cap(lastRead+ioBudgetMs);heartbeat=current+500;arm();
 }}):undefined;
 return Object.freeze({ready,close,waitClosed:()=>closed,...(activation?{activation}:{})});
}
