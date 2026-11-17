/** 同源读取的原始字节有界，执行捕获的模块而不是再请求可能变化的 URL。 */
export async function readRegistrationArtifact(address:string,origin:string,limit:number,signal:AbortSignal):Promise<Uint8Array<ArrayBuffer>>{
 const url=new URL(address);
 if(url.href!==address||url.origin!==origin||url.protocol!=='https:'||url.username||url.password||url.search||url.hash||address.length>2048)throw Error('browser registration artifact origin');
 const response=await fetch(address,{signal,credentials:'omit',redirect:'error',cache:'no-store'});
 if(!response.ok||!response.body||Number(response.headers.get('content-length'))>limit){await response.body?.cancel();throw Error('browser registration artifact unavailable');}
 const reader=response.body.getReader(),storage=new Uint8Array(limit);let length=0;
 try{for(;;){if(signal.aborted)throw Error('browser registration artifact cancelled');const item=await reader.read();if(item.done)break;if(item.value.length>limit-length)throw Error('browser registration artifact bounds');storage.set(item.value,length);length+=item.value.length;}}
 finally{await reader.cancel();reader.releaseLock();}
 if(signal.aborted||!length)throw Error('browser registration artifact cancelled');return storage.slice(0,length);
}
export async function importRegistrationArtifact(bytes:Uint8Array<ArrayBuffer>,signal:AbortSignal):Promise<Record<string,any>>{
 if(signal.aborted)throw Error('browser registration artifact cancelled');
 const address=URL.createObjectURL(new Blob([bytes],{type:'text/javascript'}));
 try{const module=await import(/* @vite-ignore */ address);if(signal.aborted)throw Error('browser registration artifact cancelled');return module;}finally{URL.revokeObjectURL(address);}
}
