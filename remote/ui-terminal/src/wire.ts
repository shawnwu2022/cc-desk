// Inner framing/JSON only. These functions establish no peer identity or authority.
import {ProjectionReceiver, type Stamp} from './reducer.ts';
const encoder=new TextEncoder();
const decoder=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true});
// Preserve lexical integer evidence without changing ordinary JSON values.
const numberTokens=new WeakMap<object,Map<string,string>>();
const MAX_U64=18446744073709551615n;
export function counter(value:unknown,zero=false):asserts value is string {
  if(typeof value!=='string'||!/^(0|[1-9][0-9]{0,19})$/.test(value)||BigInt(value)>MAX_U64||(!zero&&value==='0'))throw Error('invalid counter');
}
export function hex(value:unknown,bytes:number,nonzero=false):asserts value is string {
  if(typeof value!=='string'||value.length!==bytes*2||!/^[0-9a-f]+$/.test(value)||(nonzero&&/^0+$/.test(value)))throw Error('invalid identifier');
}
export type RecordKind=2|3|4|5;
function header(kind:number,minor:number,selected:number,length:number){
  if(![2,3,4,5].includes(kind)||![0,1].includes(selected)||minor!==(kind===4?0:selected)||(kind===5&&minor!==1)||length<0||length>(kind===2||kind===4?16384:32768))throw Error('invalid record');
}
export function encodeRecord(kind:RecordKind,minor:number,payload:Uint8Array):Uint8Array {
  header(kind,minor,minor,payload.length);
  const bytes=new Uint8Array(payload.length+8),view=new DataView(bytes.buffer);
  view.setUint32(0,payload.length+4);view.setUint16(4,kind);view.setUint16(6,minor);bytes.set(payload,8);return bytes;
}
// One complete record, never an accumulating stream or unbounded receive queue.
export function decodeRecord(bytes:Uint8Array,selected:number):{kind:RecordKind;payload:Uint8Array} {
  if(bytes.length<8)throw Error('short record');
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),length=view.getUint32(0),kind=view.getUint16(4);
  header(kind,view.getUint16(6),selected,length-4);
  if(bytes.length!==length+4)throw Error('record length');
  return {kind:kind as RecordKind,payload:bytes.slice(8)};
}
// JSON.parse alone silently overwrites duplicate keys. Scan before constructing
// the value; decoded keys (including escaped duplicates) are compared per object.
export function parseJson(bytes:Uint8Array,limit:number):unknown {
  if(!Number.isInteger(limit)||limit<0||limit>32768||bytes.length>limit)throw Error('JSON size');
  const text=decoder.decode(bytes);let at=0;
  const space=()=>{while(at<text.length&&/[\t\r\n ]/.test(text[at]))at++;};
  const string=():string=>{
    const start=at;if(text[at++]!=='"')throw Error('JSON string');
    while(at<text.length){const c=text[at++];if(c==='"')return JSON.parse(text.slice(start,at));if(c==='\\')at++;}
    throw Error('JSON string');
  };
  const value=(depth:number,onNumber?:(token:string)=>void):unknown=>{
    space();if(depth>16||(depth===16&&(text[at]==='{'||text[at]==='[')))throw Error('JSON depth');
    const c=text[at];
    if(c==='"')return string();
    if(c==='{'||c==='['){
      at++;space();const object=c==='{',end=object?'}':']',keys=new Set<string>();
      const out:any=object?{}:[],tokens=new Map<string,string>();numberTokens.set(out,tokens);
      if(text[at]===end){at++;return out;}
      while(true){
        if(object){const key=string();if(keys.has(key))throw Error('duplicate JSON key');keys.add(key);space();if(text[at++]!==':')throw Error('JSON colon');Object.defineProperty(out,key,{value:value(depth+1,token=>tokens.set(key,token)),enumerable:true});}
        else {const index=String(out.length);out.push(value(depth+1,token=>tokens.set(index,token)));}
        space();const next=text[at++];if(next===end)return out;if(next!==',')throw Error('JSON delimiter');space();
      }
    }
    for(const literal of ['true','false','null'])if(text.startsWith(literal,at)){at+=literal.length;return JSON.parse(literal);}
    const match=/-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/.exec(text.slice(at));
    if(!match||match.index!==0)throw Error('JSON value');at+=match[0].length;onNumber?.(match[0]);const n=Number(match[0]);if(!Number.isFinite(n))throw Error('JSON number');return n;
  };
  const result=value(0);space();if(at!==text.length)throw Error('trailing JSON');return result;
}
// Rust unsigned integer fields reject 1.0, 1e0 and -0 even if JS rounds them to
// an integer. Check only declared numeric fields; additive fields may be floats.
function integerLexeme(owner:unknown,key:string|number):void {
  if(!owner||typeof owner!=='object')return;
  const token=numberTokens.get(owner)?.get(String(key));
  if(token!==undefined&&!/^(0|[1-9][0-9]*)$/.test(token))throw Error('noninteger numeric lexeme');
}
function json(value:unknown,limit=16270):Uint8Array {
  const bytes=encoder.encode(JSON.stringify(value));if(bytes.length>limit)throw Error('payload size');return bytes;
}
export function command(message:'Snapshot'|'Window'|'Disconnect',request:string):Uint8Array {
  if(!['Snapshot','Window','Disconnect'].includes(message))throw Error('unsupported command');counter(request);return json({message,profile:1,request});
}
export function projectionAck(stamp:Stamp):Uint8Array {
  // Reuse the reducer's canonical context/stamp checks; this is syntax only.
  new ProjectionReceiver(stamp.target,stamp.binding).begin(stamp);
  const t=stamp.target;
  return json({message:'ProjectionAck',profile:1,stamp:{target:{host:t.host,session:t.session,run:t.run,generation:t.generation,incarnation:t.incarnation},binding:stamp.binding,barrier:stamp.barrier,revision:stamp.revision,dimensionEpoch:stamp.dimensionEpoch,modeEpoch:stamp.modeEpoch,structureEpoch:stamp.structureEpoch,syncEpoch:stamp.syncEpoch}});
}
function base64url(bytes:Uint8Array):string {
  const alphabet='ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';let out='';
  for(let i=0;i<bytes.length;i+=3){const a=bytes[i],b=bytes[i+1],c=bytes[i+2];out+=alphabet[a>>2]+alphabet[((a&3)<<4)|((b??0)>>4)];if(i+1<bytes.length)out+=alphabet[((b&15)<<2)|((c??0)>>6)];if(i+2<bytes.length)out+=alphabet[c&63];}return out;
}
export function input(operation:string,sequence:string,window:string,notAfterHostMs:string,bytes:Uint8Array):Uint8Array {
  hex(operation,16,true);counter(sequence);hex(window,32,true);counter(notAfterHostMs);
  if(!(bytes instanceof Uint8Array)||bytes.length===0||bytes.length>8192)throw Error('input size');
  return json({message:'InputChunk',profile:1,operation,sequence,window,notAfterHostMs,bytes:base64url(bytes)});
}


export type Outcome='HostWritten'|'Rejected'|'Partial'|'Unknown';
export type Receipt={operation:string;sequence:string;outcome:Outcome;proof:string|null};
export type Reply=
  | {message:'WindowGranted';profile:1;request:string;window:string;notAfterHostMs:string}
  | {message:'ProjectionAcknowledged';profile:1;stamp:Stamp}
  | ({message:'InputReceipt';profile:1}&Receipt)
  | {message:'InputNotAccepted';profile:1;operation:string;sequence:string}
  | {message:'ReceiptStatus';profile:1;request:string;operation:string;sequence:string;status:'Known'|'Missing'|'TooOld';outcome:Outcome|null;proof:string|null}
  | {message:'ReceiptSettled';profile:1;request:string;operation:string;sequence:string;outcome:'HostWritten'|'Rejected';disposition:'Advanced'|'AlreadySettled'|'Rejected'};
function shape(value:unknown,keys:readonly string[]):asserts value is Record<string,any>{
  if(!value||typeof value!=='object'||Array.isArray(value)||Object.keys(value).length!==keys.length||keys.some(k=>!Object.hasOwn(value,k)))throw Error('reply shape');
}
function receiptFields(value:Record<string,any>,finalOnly=false):void {
  hex(value.operation,16,true);counter(value.sequence);
  if(value.outcome==='HostWritten'||value.outcome==='Rejected')hex(value.proof,32,true);
  else if(finalOnly||!['Partial','Unknown'].includes(value.outcome)||value.proof!==null)throw Error('receipt proof');
}
function stampShape(value:unknown):asserts value is Stamp {
  shape(value,['target','binding','barrier','revision','dimensionEpoch','modeEpoch','structureEpoch','syncEpoch']);
  shape(value.target,['host','session','run','generation','incarnation']);projectionAck(value as Stamp);
}
/** Syntax only. The native adapter must verify original received DataRecord
 * direction/channel/scope/fence before exposing any reply bytes to the client. */
export function decodeReply(payload:Uint8Array):Reply {
  const value:any=parseJson(payload,16270);
  integerLexeme(value,'profile');if(!value||value.profile!==1)throw Error('reply profile');
  switch(value.message){
    case 'WindowGranted':shape(value,['message','profile','request','window','notAfterHostMs']);counter(value.request);hex(value.window,32,true);counter(value.notAfterHostMs);break;
    case 'ProjectionAcknowledged':shape(value,['message','profile','stamp']);stampShape(value.stamp);break;
    case 'InputReceipt':shape(value,['message','profile','operation','sequence','outcome','proof']);receiptFields(value);break;
    case 'InputNotAccepted':shape(value,['message','profile','operation','sequence']);hex(value.operation,16,true);counter(value.sequence);break;
    case 'ReceiptStatus':
      shape(value,['message','profile','request','operation','sequence','status','outcome','proof']);counter(value.request);hex(value.operation,16,true);counter(value.sequence);
      if(value.status==='Known')receiptFields(value);
      else if(!['Missing','TooOld'].includes(value.status)||value.outcome!==null||value.proof!==null)throw Error('receipt status');break;
    case 'ReceiptSettled':
      shape(value,['message','profile','request','operation','sequence','outcome','disposition']);counter(value.request);hex(value.operation,16,true);counter(value.sequence);
      if(!['HostWritten','Rejected'].includes(value.outcome)||!['Advanced','AlreadySettled','Rejected'].includes(value.disposition))throw Error('settlement');break;
    default:throw Error('unsupported reply');
  }
  return value as Reply;
}
export function receiptQuery(request:string,operation:string,sequence:string):Uint8Array {
  counter(request);hex(operation,16,true);counter(sequence);return json({message:'ReceiptQuery',profile:1,request,operation,sequence});
}
export function receiptAck(request:string,receipt:Receipt):Uint8Array {
  counter(request);receiptFields(receipt,true);return json({message:'ReceiptAck',profile:1,request,operation:receipt.operation,sequence:receipt.sequence,outcome:receipt.outcome,proof:receipt.proof});
}


/** Projection permits additive fields where the Rust profile does; nested
 * target/stamp objects remain exact serde deny_unknown_fields contracts. */
export function decodeProjection(payload:Uint8Array):unknown {
  const value:any=parseJson(payload,32768);
  if(!value||value.profile!==1||!['SnapshotPart','Delta'].includes(value.message)||!value.body)throw Error('projection shape');
  integerLexeme(value,'profile');stampShape(value.body.stamp);
  const body=value.body,state=body.state;
  integerLexeme(body,'start');integerLexeme(state,'rows');integerLexeme(state,'columns');
  integerLexeme(state?.cursor,'row');integerLexeme(state?.cursor,'column');
  const cellNumbers=(cell:any)=>{
    integerLexeme(cell,'s');
    for(const color of [cell?.f,cell?.b]){
      integerLexeme(color,'Palette');
      if(Array.isArray(color?.Rgb))color.Rgb.forEach((_:unknown,index:number)=>integerLexeme(color.Rgb,index));
    }
  };
  if(Array.isArray(body.cells))body.cells.forEach(cellNumbers);
  if(Array.isArray(body.changes))body.changes.forEach((change:any)=>{integerLexeme(change,'index');cellNumbers(change?.cell);});
  return value;
}
