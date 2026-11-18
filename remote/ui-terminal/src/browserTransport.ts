import type {BrowserTlsTransport} from './browserTlsWorker.ts';
const used=new WeakSet<MessagePort>();
function claim(port:MessagePort){if(used.has(port))throw Error('browser transport port reused');used.add(port);}
type LiveActivation=Readonly<{activation:Readonly<{activate(end:number):void}>;setupEnd:number;maxEnd:number;clock:()=>number}>;
export function serveBrowserTransport(port:MessagePort,transport:BrowserTlsTransport,live?:LiveActivation){
 claim(port);let closed=false,read:Promise<void>|undefined,write:Promise<void>|undefined,sequence=0,retirement:Promise<void>|undefined;
 let resolve!:()=>void,reject!:(e:Error)=>void;const completion=new Promise<void>((a,b)=>{resolve=a;reject=b;});void completion.catch(()=>{});
 let liveSlot:'Dormant'|'Pending'|'Active'|'Closed'='Dormant',liveSeq=0,sampleBefore=0,lastClock=-Infinity,liveTimer:ReturnType<typeof setTimeout>|undefined;
 function sample(){const n=live?.clock();if(typeof n!=='number'||!Number.isSafeInteger(n)||n<0||n<lastClock)throw Error('live handoff clock');lastClock=n;return n;}
 function clearLive(){if(liveTimer!==undefined)clearTimeout(liveTimer);liveTimer=undefined;}
 function close(){if(retirement)return;liveSlot='Closed';clearLive();closed=true;let failed=false;try{transport.close();}catch{failed=true;}
  retirement=Promise.resolve().then(()=>transport.waitClosed()).then(()=>false,()=>true).then(async uncertain=>{await Promise.all([read,write]);port.postMessage({kind:'retired',uncertain:uncertain||failed});port.close();if(uncertain||failed)reject(Error('browser transport retirement uncertain'));else resolve();});void retirement.catch(()=>reject(Error('browser transport retirement uncertain')));
 }
 port.onmessage=event=>{const m=event.data;if(m?.kind==='close'){close();return;}if(closed)return;
  if(m?.kind==='live-prepare'||m?.kind==='live-commit'){
   try{
    if(!live||!Number.isSafeInteger(live.setupEnd)||!Number.isSafeInteger(live.maxEnd)||live.maxEnd<live.setupEnd)throw Error('live handoff unavailable');
    if(m.kind==='live-prepare'){
     if(Object.keys(m).length!==2||liveSlot!=='Dormant'||m.seq!==sequence+1||!Number.isSafeInteger(m.seq)||m.seq>4294967295)throw Error('live handoff replay');
     liveSlot='Pending';sequence=m.seq;liveSeq=m.seq;sampleBefore=sample();if(sampleBefore>=live.setupEnd)throw Error('live handoff deadline');
     liveTimer=setTimeout(close,Math.max(0,Math.min(1000,live.setupEnd-sampleBefore)));port.postMessage({kind:'live-challenge',seq:liveSeq});return;
    }
    if(Object.keys(m).length!==3||liveSlot!=='Pending'||m.seq!==liveSeq||!Number.isSafeInteger(m.remaining)||m.remaining<1||m.remaining>900000)throw Error('live handoff commit');
    const now=sample(),end=Math.min(live.maxEnd,sampleBefore+m.remaining);if(!Number.isSafeInteger(end)||now>=end||now>=live.setupEnd||now-sampleBefore>=1000)throw Error('live handoff expired');
    live.activation.activate(end);if(sample()>=end)throw Error('live handoff changed');liveSlot='Active';clearLive();port.postMessage({kind:'result',seq:liveSeq,ok:true});return;
   }catch{close();return;}
  }
  if(!m||Object.keys(m).length!==3||m.seq!==sequence+1||!Number.isSafeInteger(m.seq)||m.seq>4294967295||!['read','write'].includes(m.kind)||(m.kind==='read'?read||!Number.isInteger(m.maximum)||m.maximum<1||m.maximum>16384:write||!(m.bytes instanceof Uint8Array)||m.bytes.length<1||m.bytes.length>16384)){close();return;}sequence=m.seq;
  const work=Promise.resolve().then(async():Promise<Uint8Array|null|void>=>{if(closed)throw Error('browser transport cancelled');return m.kind==='read'?await transport.read(m.maximum):await transport.write(m.bytes.slice());}).then(value=>{if(value!==undefined&&value!==null&&(!(value instanceof Uint8Array)||value.length<1||value.length>m.maximum))throw Error('browser transport read bounds');port.postMessage({kind:'result',seq:m.seq,ok:!closed,value});},()=>{port.postMessage({kind:'result',seq:m.seq,ok:false});close();}).catch(()=>close()).finally(()=>{if(m.kind==='read')read=undefined;else write=undefined;});if(m.kind==='read')read=work;else write=work;
 };port.onmessageerror=close;port.start();return Object.freeze({close,waitClosed:()=>completion});
}
export function browserTransport(port:MessagePort):BrowserTlsTransport{
 claim(port);let closed=false,sequence=0,reading=false,writing=false,liveUsed=false,challenged=false,control:Promise<void>|undefined,sampleRemaining:(()=>Promise<number>)|undefined;const pending=new Map<number,{kind:string;resolve:(v:any)=>void;reject:(e:Error)=>void}>();let resolve!:()=>void,reject!:(e:Error)=>void;const completion=new Promise<void>((a,b)=>{resolve=a;reject=b;});void completion.catch(()=>{});
 function close(){if(closed)return;closed=true;port.postMessage({kind:'close'});}
 port.onmessage=event=>{const m=event.data;if(m?.kind==='retired'){closed=true;for(const p of pending.values())p.reject(Error('browser transport closed'));pending.clear();port.close();void Promise.resolve(control).then(()=>{if(m.uncertain)reject(Error('browser transport retirement uncertain'));else resolve();},()=>reject(Error('browser transport retirement uncertain')));return;}const p=pending.get(m?.seq);
 if(m?.kind==='live-challenge'){
  if(closed||!p||p.kind!=='keepalive'||challenged||Object.keys(m).length!==2||!sampleRemaining){close();return;}challenged=true;
  control=Promise.resolve().then(()=>{if(closed)throw Error('live handoff cancelled');return sampleRemaining!();}).then(remaining=>{if(closed)return;if(!Number.isSafeInteger(remaining)||remaining<1||remaining>900000)throw Error('live handoff remaining');port.postMessage({kind:'live-commit',seq:m.seq,remaining});},()=>{close();}).catch(()=>close());return;
 }
 if(m?.kind!=='result'||!p){close();return;}if(p.kind==='keepalive'&&(!challenged||m.value!==undefined||Object.keys(m).length!==3)){close();return;}pending.delete(m.seq);if(p.kind==='read')reading=false;else if(p.kind==='write')writing=false;if(closed||m.ok!==true)p.reject(Error('browser transport rejected'));else p.resolve(m.value);};port.onmessageerror=close;port.start();
 function run(kind:'read'|'write',value:number|Uint8Array){if(closed||sequence===4294967295||(kind==='read'?reading:writing))return Promise.reject(Error('browser transport unavailable'));const seq=++sequence;if(kind==='read')reading=true;else writing=true;const result=new Promise<any>((resolve,reject)=>{pending.set(seq,{kind,resolve,reject});});port.postMessage(kind==='read'?{kind,seq,maximum:value}:{kind,seq,bytes:(value as Uint8Array).slice()});return result;}
 return Object.freeze({activateKeepalive(remaining:()=>Promise<number>){if(closed||liveUsed||sequence===4294967295)return Promise.reject(Error('live handoff unavailable'));liveUsed=true;sampleRemaining=remaining;const seq=++sequence;const operation=new Promise<void>((resolve,reject)=>pending.set(seq,{kind:'keepalive',resolve,reject}));try{port.postMessage({kind:'live-prepare',seq});}catch{close();}return operation;},read(maximum:number){if(!Number.isInteger(maximum)||maximum<1||maximum>16384)return Promise.reject(Error('browser transport read bounds'));return run('read',maximum);},write(bytes:Uint8Array){if(!(bytes instanceof Uint8Array)||bytes.length<1||bytes.length>16384)return Promise.reject(Error('browser transport write bounds'));return run('write',bytes);},close,waitClosed:()=>completion});
}
