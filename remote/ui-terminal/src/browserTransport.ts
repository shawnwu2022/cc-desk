import type {BrowserTlsTransport} from './browserTlsWorker.ts';
const used=new WeakSet<MessagePort>();
function claim(port:MessagePort){if(used.has(port))throw Error('browser transport port reused');used.add(port);}
export function serveBrowserTransport(port:MessagePort,transport:BrowserTlsTransport){
 claim(port);let closed=false,read:Promise<void>|undefined,write:Promise<void>|undefined,sequence=0,retirement:Promise<void>|undefined;
 let resolve!:()=>void,reject!:(e:Error)=>void;const completion=new Promise<void>((a,b)=>{resolve=a;reject=b;});void completion.catch(()=>{});
 function close(){if(retirement)return;closed=true;let failed=false;try{transport.close();}catch{failed=true;}
  retirement=Promise.resolve().then(()=>transport.waitClosed()).then(()=>false,()=>true).then(async uncertain=>{await Promise.all([read,write]);port.postMessage({kind:'retired',uncertain:uncertain||failed});port.close();if(uncertain||failed)reject(Error('browser transport retirement uncertain'));else resolve();});void retirement.catch(()=>reject(Error('browser transport retirement uncertain')));
 }
 port.onmessage=event=>{const m=event.data;if(m?.kind==='close'){close();return;}if(closed)return;
  if(!m||Object.keys(m).length!==3||m.seq!==sequence+1||!Number.isSafeInteger(m.seq)||m.seq>4294967295||!['read','write'].includes(m.kind)||(m.kind==='read'?read||!Number.isInteger(m.maximum)||m.maximum<1||m.maximum>16384:write||!(m.bytes instanceof Uint8Array)||m.bytes.length<1||m.bytes.length>16384)){close();return;}sequence=m.seq;
  const work=Promise.resolve().then(async():Promise<Uint8Array|null|void>=>{if(closed)throw Error('browser transport cancelled');return m.kind==='read'?await transport.read(m.maximum):await transport.write(m.bytes.slice());}).then(value=>{if(value!==undefined&&value!==null&&(!(value instanceof Uint8Array)||value.length<1||value.length>m.maximum))throw Error('browser transport read bounds');port.postMessage({kind:'result',seq:m.seq,ok:!closed,value});},()=>{port.postMessage({kind:'result',seq:m.seq,ok:false});close();}).catch(()=>close()).finally(()=>{if(m.kind==='read')read=undefined;else write=undefined;});if(m.kind==='read')read=work;else write=work;
 };port.onmessageerror=close;port.start();return Object.freeze({close,waitClosed:()=>completion});
}
export function browserTransport(port:MessagePort):BrowserTlsTransport{
 claim(port);let closed=false,sequence=0,reading=false,writing=false;const pending=new Map<number,{kind:string;resolve:(v:any)=>void;reject:(e:Error)=>void}>();let resolve!:()=>void,reject!:(e:Error)=>void;const completion=new Promise<void>((a,b)=>{resolve=a;reject=b;});void completion.catch(()=>{});
 function close(){if(closed)return;closed=true;port.postMessage({kind:'close'});}
 port.onmessage=event=>{const m=event.data;if(m?.kind==='retired'){closed=true;for(const p of pending.values())p.reject(Error('browser transport closed'));pending.clear();port.close();if(m.uncertain)reject(Error('browser transport retirement uncertain'));else resolve();return;}const p=pending.get(m?.seq);if(m?.kind!=='result'||!p){close();return;}pending.delete(m.seq);if(p.kind==='read')reading=false;else writing=false;if(closed||m.ok!==true)p.reject(Error('browser transport rejected'));else p.resolve(m.value);};port.onmessageerror=close;port.start();
 function run(kind:'read'|'write',value:number|Uint8Array){if(closed||sequence===4294967295||(kind==='read'?reading:writing))return Promise.reject(Error('browser transport unavailable'));const seq=++sequence;if(kind==='read')reading=true;else writing=true;const result=new Promise<any>((resolve,reject)=>{pending.set(seq,{kind,resolve,reject});});port.postMessage(kind==='read'?{kind,seq,maximum:value}:{kind,seq,bytes:(value as Uint8Array).slice()});return result;}
 return Object.freeze({read(maximum:number){if(!Number.isInteger(maximum)||maximum<1||maximum>16384)return Promise.reject(Error('browser transport read bounds'));return run('read',maximum);},write(bytes:Uint8Array){if(!(bytes instanceof Uint8Array)||bytes.length<1||bytes.length>16384)return Promise.reject(Error('browser transport write bounds'));return run('write',bytes);},close,waitClosed:()=>completion});
}
