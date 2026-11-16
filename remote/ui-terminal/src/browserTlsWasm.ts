import {createBrowserTlsWorkerSigner,type BrowserTlsWorkerBridge} from './browserTlsSigner.ts';
let ownerClaimed=false;

/** 私有、每实例一次的原始 WASM import 边；不提供 WASM loader 或通用内存接口。 */
export function createBrowserTlsWasmImports(bridge:BrowserTlsWorkerBridge,enrolledTlsSpki:Uint8Array,getMemory:()=>WebAssembly.Memory|undefined):Readonly<{imports:WebAssembly.Imports;close():void}>{
 // 官方 glue 的 ESM wasm 绑定属于整个 Worker；关闭后也不能换第二个 owner。
 if(ownerClaimed)throw Error('WASM TLS Worker owner already claimed');
 const signer=createBrowserTlsWorkerSigner(bridge),deadline=bridge.setupDeadline;
 const clock=()=>performance.timeOrigin+performance.now();
 const head=Uint8Array.from('3059301306072a8648ce3d020106082a8648ce3d03010703420004'.match(/../g)!,v=>parseInt(v,16));
 if(typeof deadline!=='number'||!Number.isFinite(deadline)||deadline<=clock()||!(enrolledTlsSpki instanceof Uint8Array)||enrolledTlsSpki.length!==91){signer.close();throw Error('WASM TLS enrollment/deadline unavailable');}
 const spki=Uint8Array.from(enrolledTlsSpki);
 if(!head.every((v,i)=>spki[i]===v)){signer.close();throw Error('WASM TLS SPKI');}
 ownerClaimed=true;
 let bound:WebAssembly.Memory|undefined,initialized=false,closed=false,active=false,expected=1;
 function close(){closed=true;signer.close();}
 function memory():ArrayBuffer{
  const value=getMemory();
  if(!(value instanceof WebAssembly.Memory)||value.buffer instanceof SharedArrayBuffer||(bound!==undefined&&bound!==value))throw Error('WASM TLS memory ownership');
  bound=value;return value.buffer;
 }
 function range(buffer:ArrayBuffer,pointer:number,length:number):Uint8Array{
  if(!Number.isInteger(pointer)||pointer<0||!Number.isInteger(length)||length<0||pointer>buffer.byteLength-length)throw Error('WASM TLS memory bounds');
  return new Uint8Array(buffer,pointer,length);
 }
 const imports={cc_desk_browser_tls_v1:{
  now:()=>closed?NaN:clock(),
  init:(pointer:number,length:number,originalDeadline:number):number=>{
   try{
    if(initialized||closed||originalDeadline!==deadline||clock()>=deadline||length!==91)throw Error('WASM TLS initialization');
    initialized=true;const candidate=range(memory(),pointer,length);
    if(!spki.every((v,i)=>candidate[i]===v))throw Error('WASM TLS identity');
    return 1;
   }catch{close();return -1;}
  },
  sign:(seq:number,originalDeadline:number,pointer:number,length:number,output:number,capacity:number):number=>{
   try{
    if(!initialized||closed||active||seq!==expected||seq<1||seq>0x7fffffff||originalDeadline!==deadline||clock()>=deadline||length!==130||capacity!==72)throw Error('WASM TLS request fence');
    active=true;expected++;
    const before=memory(),input=range(before,pointer,length).slice();range(before,output,capacity);
    if(pointer<output+capacity&&output<pointer+length)throw Error('WASM TLS overlapping buffers');
    const result=signer.sign(input);
    if(closed||clock()>=deadline||result.length<8||result.length>capacity)throw Error('WASM TLS completion');
    // memory.grow 会替换 buffer，绝不复用调用前的 view。
    const destination=range(memory(),output,capacity);destination.fill(0);destination.set(result);return result.length;
   }catch{close();return -1;}finally{active=false;}
  },
  close,
 }};
 return Object.freeze({imports,close});
}
