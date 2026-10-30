import {ProjectionReceiver, type Stamp, type Target} from './reducer.ts';
import {render} from './renderer.ts';
import {command,input,projectionAck,decodeProjection,decodeReply,receiptQuery,receiptAck,type Receipt,type Reply} from './wire.ts';
/** An injected native adapter must own the verified mutual-RPK/AppBind channel
 * and validate original received DataRecord direction/channel/scope/fence. These
 * methods return bounded JSON payloads, never authentication booleans. This
 * interface has no browser WebSocket implementation or plaintext fallback.
 */
export interface NativeConnection {
  snapshot(command:Uint8Array,sink:{begin(stamp:Stamp):void;part(payload:Uint8Array):void}):Promise<void>;
  acknowledge(payload:Uint8Array):Promise<Uint8Array>;
  window(command:Uint8Array):Promise<Uint8Array>;
  input(command:Uint8Array):Promise<Uint8Array>;
  query(command:Uint8Array):Promise<Uint8Array>;
  settle(command:Uint8Array):Promise<Uint8Array>;
  close():void;
}
export type Phase='NotConnected'|'Idle'|'Busy'|'Synced'|'Ready'|'NeedsSettlement';
type Pending={operation:string;sequence:string;receipt?:Receipt};
/** One selected Host/target/binding. One in-flight request, no receive/input
 * queue, automatic reconnect, relaunch or replay. HostWritten means Host
 * IO only. Receipt uncertainty survives disconnect and blocks all new input.
 */
export class TerminalClient {
  #receiver:ProjectionReceiver;
  #binding:string;
  #native:NativeConnection|undefined;
  #epoch=0;
  #request=0n;
  #phase:Phase='NotConnected';
  #window:{token:string;notAfterHostMs:string}|undefined;
  #pending:Pending|undefined;
  #budget:number;
  constructor(target:Target,binding:string,budgetMs=1000){if(!Number.isInteger(budgetMs)||budgetMs<1||budgetMs>1000)throw Error('invalid deadline');this.#budget=budgetMs;this.#receiver=new ProjectionReceiver(target,binding);this.#binding=binding;}
  get phase():Phase{return this.#phase;}
  get frame(){return this.#receiver.frame;}
  render(document:Parameters<typeof render>[0],palette?:readonly string[]){const frame=this.frame;if(!frame)throw Error('no installed frame');return render(document,frame,palette);}
  connect(native:NativeConnection,binding=this.#binding):void {
    if(this.#native)throw Error('already connected');
    if(!native||!['snapshot','acknowledge','window','input','query','settle','close'].every(k=>typeof (native as any)[k]==='function'))throw Error('native adapter required');
    if(binding!==this.#binding){this.#receiver.rebind(binding);this.#binding=binding;}
    this.#native=native;this.#epoch++;this.#request=0n;this.#phase=this.#pending?'NeedsSettlement':'Idle';
  }
  disconnect():void {
    const native=this.#native;this.#epoch++;this.#native=undefined;this.#window=undefined;this.#receiver.disconnect();this.#phase='NotConnected';native?.close();
  }
  #enter(allowed:Phase[],reconcile=false):{native:NativeConnection;epoch:number}{
    if(!this.#native||!allowed.includes(this.#phase)||(!reconcile&&this.#pending))throw Error('client unavailable');
    this.#phase='Busy';return {native:this.#native,epoch:this.#epoch};
  }
  async #wait<T>(promise:Promise<T>,epoch:number):Promise<T>{
    let timer:ReturnType<typeof setTimeout>|undefined;
    try{const result=await Promise.race([promise,new Promise<never>((_,reject)=>{timer=setTimeout(()=>reject(Error('native deadline exceeded')),this.#budget);})]);this.#check(epoch);return result;}
    finally{if(timer!==undefined)clearTimeout(timer);}
  }
  #check(epoch:number):void{if(epoch!==this.#epoch||!this.#native)throw Error('stale connection');}
  #next():string{if(this.#request===18446744073709551615n)throw Error('request counter exhausted');return String(++this.#request);}
  #fail(epoch:number):void{if(epoch===this.#epoch)this.disconnect();}
  #identity(reply:{operation:string;sequence:string},pending:Pending):void{if(reply.operation!==pending.operation||reply.sequence!==pending.sequence)throw Error('receipt identity');}
  async snapshot():Promise<void> {
    const {native,epoch}=this.#enter(['Idle','Synced','Ready']);this.#window=undefined;let begun=false,installed=false,total=0;
    try{
      await this.#wait(native.snapshot(command('Snapshot',this.#next()),{
        begin:stamp=>{this.#check(epoch);if(begun)throw Error('repeated barrier');this.#receiver.begin(stamp);begun=true;},
        part:payload=>{
          this.#check(epoch);if(!begun||installed)throw Error('unsolicited projection');total+=payload.length;if(total>4*1024*1024)throw Error('snapshot budget');
          const result=this.#receiver.accept(decodeProjection(payload));if(result==='Gap'||result==='Delta')throw Error('invalid snapshot part');installed=result==='Installed';
        },
      }),epoch);
      this.#check(epoch);const stamp=this.#receiver.ack();if(!installed||!stamp)throw Error('incomplete snapshot');
      const ack=projectionAck(stamp),reply=decodeReply(await this.#wait(native.acknowledge(ack),epoch));this.#check(epoch);
      if(reply.message!=='ProjectionAcknowledged'||new TextDecoder().decode(projectionAck(reply.stamp))!==new TextDecoder().decode(ack))throw Error('ACK mismatch');this.#phase='Synced';
    }catch(error){this.#fail(epoch);throw error;}
  }
  async window():Promise<void> {
    const {native,epoch}=this.#enter(['Synced','Ready']);this.#window=undefined;
    try{
      const request=this.#next(),reply=decodeReply(await this.#wait(native.window(command('Window',request)),epoch));this.#check(epoch);
      if(reply.message!=='WindowGranted'||reply.request!==request)throw Error('window mismatch');
      this.#window={token:reply.window,notAfterHostMs:reply.notAfterHostMs};this.#phase='Ready';
    }catch(error){this.#fail(epoch);throw error;}
  }
  async submit(operation:string,sequence:string,bytes:Uint8Array):Promise<Extract<Reply,{message:'InputReceipt'|'InputNotAccepted'}>> {
    const window=this.#window;if(!window)throw Error('no current input window');const payload=input(operation,sequence,window.token,window.notAfterHostMs,bytes);
    const {native,epoch}=this.#enter(['Ready']);this.#window=undefined;const pending:Pending={operation,sequence};this.#pending=pending;
    try{
      const reply=decodeReply(await this.#wait(native.input(payload),epoch));this.#check(epoch);
      if(reply.message!=='InputReceipt'&&reply.message!=='InputNotAccepted')throw Error('input reply');this.#identity(reply,pending);
      if(reply.message==='InputNotAccepted'){this.#pending=undefined;this.#phase='Synced';}
      else{pending.receipt={operation,sequence,outcome:reply.outcome,proof:reply.proof};this.#phase='NeedsSettlement';}
      return Object.freeze(reply);
    }catch(error){this.#fail(epoch);throw error;}
  }
  async query():Promise<Extract<Reply,{message:'ReceiptStatus'}>> {
    const pending=this.#pending;if(!pending)throw Error('no pending receipt');const {native,epoch}=this.#enter(['NeedsSettlement'],true);
    try{
      const request=this.#next(),reply=decodeReply(await this.#wait(native.query(receiptQuery(request,pending.operation,pending.sequence)),epoch));this.#check(epoch);
      if(reply.message!=='ReceiptStatus'||reply.request!==request)throw Error('query mismatch');this.#identity(reply,pending);
      if(reply.status==='Known'){
        const old=pending.receipt,next={operation:pending.operation,sequence:pending.sequence,outcome:reply.outcome!,proof:reply.proof};
        if(old&&(old.outcome==='HostWritten'||old.outcome==='Rejected')&&
            (next.outcome!==old.outcome||next.proof!==old.proof))throw Error('contradictory receipt');
        pending.receipt=next;
      }
      // Missing/TooOld cannot erase a final proof already held by this operation.
      // Only a subsequent exact Host-validated settlement may unblock new input.
      this.#phase='NeedsSettlement';return Object.freeze(reply);
    }catch(error){this.#fail(epoch);throw error;}
  }
  async settle():Promise<void> {
    const pending=this.#pending;if(!pending?.receipt)throw Error('no final receipt');
    const request=this.#next(),payload=receiptAck(request,pending.receipt),{native,epoch}=this.#enter(['NeedsSettlement'],true);
    try{
      const reply=decodeReply(await this.#wait(native.settle(payload),epoch));this.#check(epoch);
      if(reply.message!=='ReceiptSettled'||reply.request!==request)throw Error('settlement mismatch');this.#identity(reply,pending);
      if(reply.outcome!==pending.receipt.outcome)throw Error('settlement outcome');
      if(reply.disposition==='Rejected'){this.#phase='NeedsSettlement';return;}
      this.#pending=undefined;this.#window=undefined;this.#receiver.disconnect();this.#phase='Idle';
    }catch(error){this.#fail(epoch);throw error;}
  }
}
