import type {BrowserTlsTransport} from './browserTlsWorker.ts';
export type BrowserRelaySigningRequest=Readonly<{origin:string;keyId:Uint8Array;role:2;transcript:Uint8Array}>;
export type BrowserRelayIdentity=Readonly<{keyId:Uint8Array;sign:(request:BrowserRelaySigningRequest,signal:AbortSignal)=>Promise<Uint8Array>}>;
export type BrowserRelaySocket=Pick<WebSocket,'readyState'|'binaryType'|'bufferedAmount'|'send'|'close'|'addEventListener'|'removeEventListener'>;
export function createBrowserRelay(options:Readonly<{origin:string;tunnel:Uint8Array;identity:BrowserRelayIdentity;setupDeadline:number;ioBudgetMs:number;socket:BrowserRelaySocket;signal?:AbortSignal}>):Readonly<{ready:Promise<BrowserTlsTransport>;close():void;waitClosed():Promise<void>}>{
 const now=()=>performance.timeOrigin+performance.now(),{origin,socket,setupDeadline,ioBudgetMs,signal}=options;
 const url=new URL(origin),tunnel=Uint8Array.from(options.tunnel),keyId=Uint8Array.from(options.identity.keyId),sign=options.identity.sign;
 if(origin.length>512||!/^[\x00-\x7f]+$/.test(origin)||origin.includes('%')||origin.includes('\\')||url.href!==origin||url.protocol!=='wss:'||url.username||url.password||url.search||url.hash||!url.hostname||url.port==='0'||tunnel.length!==16||!tunnel.some(Boolean)||keyId.length!==16||typeof sign!=='function'||!Number.isFinite(setupDeadline)||setupDeadline<=now()||setupDeadline>now()+10000||!Number.isInteger(ioBudgetMs)||ioBudgetMs<1||ioBudgetMs>30000)throw Error('browser relay configuration');
 const controller=new AbortController();let phase:'challenge'|'signing'|'accepted'|'data'|'closed'='challenge',failed=false,physicalClosed=socket.readyState===3;
 let signer:Promise<void>|undefined,write:Promise<void>|undefined,read:{maximum:number;resolve:(v:Uint8Array|null)=>void;reject:(e:Error)=>void}|undefined;
 let queue:Uint8Array[]=[],queuedBytes=0,offset=0,readDeadline=setupDeadline,timer:ReturnType<typeof setTimeout>|undefined;
 let resolveReady!:(v:BrowserTlsTransport)=>void,rejectReady!:(e:Error)=>void,resolveClosed!:()=>void;
 const ready=new Promise<BrowserTlsTransport>((a,b)=>{resolveReady=a;rejectReady=b;}),closed=new Promise<void>(a=>{resolveClosed=a;});void ready.catch(()=>{});
 let retired=false;const drainWaiters=new Set<()=>void>();
 function retire(){if(phase!=='closed'||!physicalClosed||signer||write||retired)return;retired=true;socket.removeEventListener('message',message);socket.removeEventListener('close',ended);socket.removeEventListener('error',broken);signal?.removeEventListener('abort',close);resolveClosed();}
 function stop(error?:Error){if(error)failed=true;if(phase==='closed')return;phase='closed';controller.abort();if(timer!==undefined)clearTimeout(timer);queue=[];queuedBytes=0;offset=0;rejectReady(error??Error('browser relay closed'));if(read){const original=read;read=undefined;if(failed)original.reject(error??Error('browser relay failed'));else original.resolve(null);}for(const wake of drainWaiters)wake();try{socket.close();}catch{failed=true;}retire();}
 function close(){stop();}
 function broken(){stop(Error('browser relay socket'));}
 function ended(){physicalClosed=true;stop();retire();}
 function arm(){if(timer!==undefined)clearTimeout(timer);if(phase==='closed')return;timer=setTimeout(()=>{if(now()>=readDeadline)stop(Error('browser relay deadline'));else arm();},Math.max(0,readDeadline-now()));}
 function deliver(){if(!read||!queue.length)return;const original=read;read=undefined;const first=queue[0]!,count=Math.min(original.maximum,first.length-offset),result=first.slice(offset,offset+count);offset+=count;queuedBytes-=count;if(offset===first.length){queue.shift();offset=0;}original.resolve(result);}
 const transport:BrowserTlsTransport=Object.freeze({
  read(maximum:number){if(!Number.isInteger(maximum)||maximum<1||maximum>16384||read)return Promise.reject(Error('browser relay read bounds/concurrency'));if(phase==='closed')return failed?Promise.reject(Error('browser relay failed')):Promise.resolve(null);if(phase!=='data')return Promise.reject(Error('browser relay not admitted'));const result=new Promise<Uint8Array|null>((resolve,reject)=>{read={maximum,resolve,reject};});deliver();return result;},
  write(bytes:Uint8Array){if(phase!=='data'||write||!(bytes instanceof Uint8Array)||bytes.length<1||bytes.length>16384)return Promise.reject(Error('browser relay write bounds/concurrency'));
   const frame=new Uint8Array(24+bytes.length);frame.set([1,1,0,1]);frame.set(tunnel,4);new DataView(frame.buffer).setUint32(20,bytes.length);frame.set(bytes,24);const end=Math.min(readDeadline,now()+ioBudgetMs);
   const wait=()=>new Promise<void>(resolve=>{let poll:ReturnType<typeof setTimeout>;const wake=()=>{clearTimeout(poll);drainWaiters.delete(wake);resolve();};drainWaiters.add(wake);poll=setTimeout(wake,Math.min(5,Math.max(0,end-now())));});
   const check=()=>{if(phase!=='data'||socket.readyState!==1||now()>=end)throw Error('browser relay write deadline/closed');};
   const operation=Promise.resolve().then(async()=>{
    while(socket.bufferedAmount+frame.length>131072){check();await wait();}
    if(phase!=='data'||socket.readyState!==1||now()>=end)throw Error('browser relay write closed');socket.send(frame);
    while(socket.bufferedAmount!==0){check();await wait();}check();
   }).catch(error=>{stop(Error('browser relay write'));throw error;}).finally(()=>{write=undefined;retire();});write=operation;return operation;
  },close,waitClosed:()=>closed,
 });
 function message(event:Event){try{
  if(phase==='closed')return;if(now()>=readDeadline)throw Error('browser relay deadline');
  const data=(event as MessageEvent).data;if(!(data instanceof ArrayBuffer)||data.byteLength>65536)throw Error('browser relay binary bounds');const bytes=new Uint8Array(data);
  if(phase==='challenge'){
   if(bytes.length!==66||bytes[0]!==1||bytes[1]!==0)throw Error('browser relay challenge');phase='signing';
   const prefix=new TextEncoder().encode('CC_DESK_RELAY_ADMISSION\0\x01'),name=new TextEncoder().encode(origin),transcript=new Uint8Array(prefix.length+2+name.length+64+16+1);transcript.set(prefix);new DataView(transcript.buffer).setUint16(prefix.length,name.length);transcript.set(name,prefix.length+2);let p=prefix.length+2+name.length;transcript.set(bytes.subarray(2),p);p+=64;transcript.set(keyId,p);transcript[p+16]=2;
   const operation=Promise.resolve().then(()=>{if(phase!=='signing'||controller.signal.aborted||now()>=setupDeadline)throw Error('browser relay signer cancelled');return sign(Object.freeze({origin,keyId:keyId.slice(),role:2,transcript}),controller.signal);}).then(signature=>{if(phase!=='signing'||now()>=setupDeadline)return;if(!(signature instanceof Uint8Array)||signature.length!==64||socket.readyState!==1||socket.bufferedAmount+83>131072)throw Error('browser relay proof');const proof=new Uint8Array([1,1,...keyId,2,...signature]);phase='accepted';socket.send(proof);}).catch(()=>stop(Error('browser relay signer'))).finally(()=>{signer=undefined;retire();});signer=operation;return;
  }
  if(phase==='accepted'){if(bytes.length!==2||bytes[0]!==1||bytes[1]!==2)throw Error('browser relay acceptance');phase='data';readDeadline=now()+ioBudgetMs;arm();resolveReady(transport);return;}
  if(phase!=='data'||bytes.length<4||bytes[0]!==1||bytes[1]!==1)throw Error('browser relay outer header');const view=new DataView(bytes.buffer),count=view.getUint16(2);if(count>32)throw Error('browser relay outer count');let cursor=4;const frames:Uint8Array[]=[];
  for(let i=0;i<count;i++){if(cursor+20>bytes.length)throw Error('browser relay outer length');if(!tunnel.every((v,j)=>bytes[cursor+j]===v)||frames.length!==0)throw Error('browser relay route');const end=cursor+20+view.getUint32(cursor+16);if(end>bytes.length)throw Error('browser relay payload length');frames.push(bytes.subarray(cursor+20,end));cursor=end;}
  if(cursor!==bytes.length||queue.length+frames.length>4||queuedBytes+frames.reduce((n,b)=>n+b.length,0)>65536)throw Error('browser relay incoming backpressure');for(const frame of frames)if(frame.length){queue.push(frame.slice());queuedBytes+=frame.length;}readDeadline=now()+ioBudgetMs;arm();deliver();
 }catch{stop(Error('browser relay message'));}}
 socket.binaryType='arraybuffer';socket.addEventListener('message',message);socket.addEventListener('close',ended);socket.addEventListener('error',broken);signal?.addEventListener('abort',close,{once:true});arm();if(signal?.aborted||physicalClosed)close();
 return Object.freeze({ready,close,waitClosed:()=>closed});
}
