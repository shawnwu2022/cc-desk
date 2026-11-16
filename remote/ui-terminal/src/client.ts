import {ProjectionReceiver, type Stamp, type Target} from './reducer.ts';
import {render} from './renderer.ts';
import {command,input,inputCursor,projectionAck,decodeProjection,decodeReply,receiptQuery,receiptAck,leaseState,leaseRenew,type LeaseStateReply,type LeaseRenewed,type Receipt,type Reply,type InputCursorReply} from './wire.ts';
/** An injected native adapter must own the verified mutual-RPK/AppBind channel
 * and validate original received DataRecord direction/channel/scope/fence. These
 * methods return bounded JSON payloads, never authentication booleans. This
 * interface has no browser WebSocket implementation or plaintext fallback.
 */
export interface NativeConnection {
  onInvalidate?(listener:()=>void):()=>void;
  snapshot(command:Uint8Array,sink:{begin(stamp:Stamp):void;part(payload:Uint8Array):void}):Promise<void>;
  acknowledge(payload:Uint8Array):Promise<Uint8Array>;
  window(command:Uint8Array):Promise<Uint8Array>;
  input(command:Uint8Array):Promise<Uint8Array>;
  query(command:Uint8Array):Promise<Uint8Array>;
  settle(command:Uint8Array):Promise<Uint8Array>;
  close():void;
}
export type Phase='NotConnected'|'Idle'|'Busy'|'Synced'|'Ready'|'NeedsSettlement';
export type AutoRenewalState='Disabled'|'Starting'|'Waiting'|'Due'|'Renewing'|'Stopping'|'Stopped'|'Unknown';
type LogicalOperation={epoch:number;done:Promise<void>;finish:()=>void;raw:Set<Promise<unknown>>};
type AutoRenewal={epoch:number;native:NativeConnection&{waitClosed():Promise<void>};join:()=>Promise<void>;state:AutoRenewalState;next:number;last:number;timer?:ReturnType<typeof setTimeout>;task?:Promise<void>;retirement?:Promise<void>};
const AUTO_PERIOD=5000,AUTO_LATE=3000;
function completion(){let resolve!:()=>void,reject!:(e:unknown)=>void;const promise=new Promise<void>((a,b)=>{resolve=a;reject=b});return{promise,resolve,reject};}
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
  #lease:{lease:string;fence:string;nextSequence:bigint;deadline:bigint}|undefined;
  #budget:number;
  #unlisten:(()=>void)|undefined;
  #operation:LogicalOperation|undefined;
  #auto:AutoRenewal|undefined;
  constructor(target:Target,binding:string,budgetMs=1000){if(!Number.isInteger(budgetMs)||budgetMs<1||budgetMs>1000)throw Error('invalid deadline');this.#budget=budgetMs;this.#receiver=new ProjectionReceiver(target,binding);this.#binding=binding;}
  get autoRenewalState():AutoRenewalState{return this.#auto?.state??'Disabled';}
  get phase():Phase{return this.#phase;}
  get frame(){return this.#receiver.frame;}
  render(document:Parameters<typeof render>[0],palette?:readonly string[]){const frame=this.frame;if(!frame)throw Error('no installed frame');return render(document,frame,palette);}
  connect(native:NativeConnection,binding=this.#binding):void {
    if(this.#native)throw Error('already connected');
    if(this.#auto&&this.#auto.state!=='Stopped')throw Error('automatic owner not retired');
    if(!native||!['snapshot','acknowledge','window','input','query','settle','close'].every(k=>typeof (native as any)[k]==='function'))throw Error('native adapter required');
    if(binding!==this.#binding){this.#receiver.rebind(binding);this.#binding=binding;}
    this.#auto=undefined;this.#native=native;this.#epoch++;this.#request=0n;this.#phase=this.#pending?'NeedsSettlement':'Idle';
    const epoch=this.#epoch;
    this.#unlisten=native.onInvalidate?.(()=>{if(this.#native===native&&this.#epoch===epoch)this.disconnect();});
  }
  disconnect():void {
    const auto=this.#auto;if(auto&&auto.epoch===this.#epoch){this.#requestStop(auto);return;}this.#disconnectNow();
  }
  #disconnectNow():void {
    const unlisten=this.#unlisten;this.#unlisten=undefined;const native=this.#native;
    this.#epoch++;this.#native=undefined;this.#window=undefined;this.#lease=undefined;this.#receiver.disconnect();this.#phase='NotConnected';
    try{unlisten?.();}finally{native?.close();}
  }
  #enter(allowed:Phase[],reconcile=false,automatic=false):{native:NativeConnection;epoch:number;operation:LogicalOperation}{
    if(!automatic){this.#pulse();if(this.#auto&&['Starting','Due','Renewing'].includes(this.#auto.state))throw Error('automatic renewal admission');}
    if(!this.#native||!allowed.includes(this.#phase)||(!reconcile&&this.#pending))throw Error('client unavailable');
    const done=completion(),operation={epoch:this.#epoch,done:done.promise,finish:done.resolve,raw:new Set<Promise<unknown>>()};this.#operation=operation;
    this.#phase='Busy';return {native:this.#native,epoch:this.#epoch,operation};
  }
  #finish(operation:LogicalOperation):void {
    if(this.#operation===operation)this.#operation=undefined;operation.finish();if(operation.epoch===this.#epoch)this.#pulse();
  }
  #sample(auto:AutoRenewal):number {
    const now=performance.now();if(!Number.isFinite(now)||now<auto.last)throw Error('automatic monotonic clock');auto.last=now;return now;
  }
  #arm(auto:AutoRenewal):void {
    if(auto.timer!==undefined)clearTimeout(auto.timer);
    if(!['Waiting','Due'].includes(auto.state))return;
    auto.timer=setTimeout(()=>{if(this.#auto!==auto||auto.epoch!==this.#epoch)return;auto.timer=undefined;this.#pulse();},Math.max(0,auto.next+(auto.state==='Due'?AUTO_LATE:0)-auto.last));
  }
  #pulse():void {
    const auto=this.#auto;if(!auto||auto.epoch!==this.#epoch||!['Waiting','Due'].includes(auto.state))return;
    try{
      const now=this.#sample(auto);if(now>=auto.next+AUTO_LATE)throw Error('automatic renewal late');
      if(now>=auto.next){auto.state='Due';if(!this.#operation&&!auto.task){this.#launchAuto(auto,false);return;}}
      this.#arm(auto);
    }catch{this.#requestStop(auto);}
  }
  #launchAuto(auto:AutoRenewal,first:boolean):Promise<void> {
    // 先登记原任务，再进入可能同步回调/关闭的连接方法。
    const task=completion();auto.task=task.promise;void task.promise.catch(()=>{});auto.state=first?'Starting':'Renewing';
    if(auto.timer!==undefined){clearTimeout(auto.timer);auto.timer=undefined;}
    const began=auto.last;
    const work=this.#renewLease(true).then(()=>{
      if(this.#auto!==auto||auto.epoch!==this.#epoch||['Stopping','Stopped','Unknown'].includes(auto.state))throw Error('automatic renewal stopped');
      const now=this.#sample(auto);if((first&&now>=began+this.#budget)||(!first&&now>=auto.next+AUTO_LATE))throw Error('automatic renewal late');
      auto.next=first?now+AUTO_PERIOD:auto.next+AUTO_PERIOD;auto.state='Waiting';this.#arm(auto);
    });
    void work.then(()=>{if(auto.task===task.promise)auto.task=undefined;task.resolve();},error=>{this.#requestStop(auto);if(auto.task===task.promise)auto.task=undefined;task.reject(error);});
    return task.promise;
  }
  startAutoRenewal():Promise<void> {
    const native=this.#native as (NativeConnection&{waitClosed?:()=>Promise<void>})|undefined;
    if(this.#auto||!native||typeof native.waitClosed!=='function'||!this.#lease||this.#phase==='Busy')return Promise.reject(Error('automatic renewal unavailable'));
    let now:number;try{now=performance.now();if(!Number.isFinite(now)||now<0)throw Error('clock');}catch{this.disconnect();return Promise.reject(Error('automatic monotonic clock'));}
    const auto:AutoRenewal={epoch:this.#epoch,native:native as AutoRenewal['native'],join:native.waitClosed.bind(native),state:'Starting',next:Infinity,last:now};this.#auto=auto;
    return this.#launchAuto(auto,true);
  }
  stopAutoRenewal():Promise<void> {
    const auto=this.#auto;if(!auto)return Promise.reject(Error('automatic owner not started'));this.#requestStop(auto);return auto.retirement!;
  }
  #requestStop(auto:AutoRenewal):void {
    if(auto.retirement)return;
    // 停止栅栏与退休 Promise 必须早于外部 close，防止同步失效通知重入。
    const retired=completion();auto.retirement=retired.promise;void retired.promise.catch(()=>{});auto.state='Stopping';
    if(auto.timer!==undefined){clearTimeout(auto.timer);auto.timer=undefined;}
    const operation=this.#operation?.epoch===auto.epoch?this.#operation:undefined,task=auto.task;
    let failed=false;if(auto.epoch===this.#epoch){try{this.#disconnectNow();}catch{failed=true;auto.state='Unknown';}}
    // 等逻辑 finally 后再收原 Promise 集合，覆盖调用返回前同步 stop 的登记交错。
    const logical=async()=>{if(operation){await operation.done;await Promise.allSettled([...operation.raw]);}};
    const joined=Promise.resolve().then(()=>auto.join()).catch(error=>{auto.state='Unknown';throw error;});
    void Promise.allSettled([logical(),task,joined]).then(results=>{
      if(failed||results[2].status==='rejected'){auto.state='Unknown';retired.reject(Error('automatic retirement unknown'));}
      else{auto.state='Stopped';retired.resolve();}
    });
  }
  async #wait<T>(promise:Promise<T>,epoch:number):Promise<T>{
    const operation=this.#operation?.epoch===epoch?this.#operation:undefined;
    operation?.raw.add(promise);void promise.then(()=>operation?.raw.delete(promise),()=>operation?.raw.delete(promise));
    let timer:ReturnType<typeof setTimeout>|undefined;
    try{const result=await Promise.race([promise,new Promise<never>((_,reject)=>{timer=setTimeout(()=>reject(Error('native deadline exceeded')),this.#budget);})]);this.#check(epoch);return result;}
    finally{if(timer!==undefined)clearTimeout(timer);}
  }
  #check(epoch:number):void{if(epoch!==this.#epoch||!this.#native)throw Error('stale connection');}
  #next():string{if(this.#request===18446744073709551615n)throw Error('request counter exhausted');return String(++this.#request);}
  #fail(epoch:number):void{if(epoch===this.#epoch)this.disconnect();}
  #identity(reply:{operation:string;sequence:string},pending:Pending):void{if(reply.operation!==pending.operation||reply.sequence!==pending.sequence)throw Error('receipt identity');}
  // 浏览器状态只关联原通道回复，真实权限由原 NativeClient/Host 复查。
  // 同一 epoch 只捕获一次 lease，不能用再次读取重置不确定的续租序号。
  async leaseState():Promise<Readonly<LeaseStateReply>> {
    if(this.#lease)throw Error('lease already captured');
    const phase=this.#phase,{native,epoch,operation:logical}=this.#enter(['Idle','Synced','Ready','NeedsSettlement'],true);
    try{
      const request=this.#next(),reply=decodeReply(await this.#wait(native.query(leaseState(request)),epoch));this.#check(epoch);
      if(reply.message!=='LeaseStateReply'||reply.request!==request)throw Error('lease state mismatch');
      this.#lease={lease:reply.lease,fence:reply.fence,nextSequence:BigInt(reply.nextRenewalSequence),deadline:BigInt(reply.notAfterHostMs)};
      this.#phase=phase;return Object.freeze(reply);
    }catch(error){this.#fail(epoch);throw error;}finally{this.#finish(logical);}
  }
  renewLease():Promise<Readonly<LeaseRenewed>> {
    if(this.#auto)return Promise.reject(Error('automatic owner owns renewal'));return this.#renewLease();
  }
  async #renewLease(automatic=false):Promise<Readonly<LeaseRenewed>> {
    const lease=this.#lease;if(!lease)throw Error('lease not captured');
    const phase=this.#phase,{native,epoch,operation:logical}=this.#enter(['Idle','Synced','Ready','NeedsSettlement'],true,automatic);
    try{
      const request=this.#next(),sequence=String(lease.nextSequence),reply=decodeReply(await this.#wait(native.query(leaseRenew(request,lease.lease,lease.fence,sequence)),epoch));this.#check(epoch);
      if(reply.message!=='LeaseRenewed'||reply.request!==request||reply.lease!==lease.lease||reply.fence!==lease.fence||reply.renewalSequence!==sequence||BigInt(reply.notAfterHostMs)<lease.deadline)throw Error('lease renewal mismatch');
      lease.nextSequence++;lease.deadline=BigInt(reply.notAfterHostMs);this.#phase=phase;return Object.freeze(reply);
    }catch(error){this.#fail(epoch);throw error;}finally{this.#finish(logical);}
  }
  async snapshot():Promise<void> {
    const {native,epoch,operation:logical}=this.#enter(['Idle','Synced','Ready']);this.#window=undefined;let begun=false,installed=false,total=0;
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
    }catch(error){this.#fail(epoch);throw error;}finally{this.#finish(logical);}
  }
  async window():Promise<void> {
    const {native,epoch,operation:logical}=this.#enter(['Synced','Ready']);this.#window=undefined;
    try{
      const request=this.#next(),reply=decodeReply(await this.#wait(native.window(command('Window',request)),epoch));this.#check(epoch);
      if(reply.message!=='WindowGranted'||reply.request!==request)throw Error('window mismatch');
      this.#window={token:reply.window,notAfterHostMs:reply.notAfterHostMs};this.#phase='Ready';
    }catch(error){this.#fail(epoch);throw error;}finally{this.#finish(logical);}
  }
  /** Read original Host durable facts; never a replacement for snapshot/ACK/window.
   * Missing Host pending state cannot erase an exact local settlement witness. */
  async cursor():Promise<Readonly<InputCursorReply>> {
    const {native,epoch,operation:logical}=this.#enter(['Idle','Synced','Ready','NeedsSettlement'],true);this.#window=undefined;
    try{
      const request=this.#next(),reply=decodeReply(await this.#wait(native.query(inputCursor(request)),epoch));this.#check(epoch);
      if(reply.message!=='InputCursorState'||reply.request!==request)throw Error('cursor mismatch');
      const found=reply.pending;
      if(found){
        if(this.#pending)this.#identity(found,this.#pending);
        const old=this.#pending?.receipt;
        if(old&&(old.outcome==='HostWritten'||old.outcome==='Rejected')&&
          (found.outcome!==old.outcome||found.proof!==old.proof))throw Error('contradictory receipt');
        this.#pending??={operation:found.operation,sequence:found.sequence};
        if(found.outcome==='HostWritten'||found.outcome==='Rejected')this.#pending.receipt={...found};
      }
      this.#phase=this.#pending?'NeedsSettlement':this.#receiver.synchronized?'Synced':'Idle';
      return Object.freeze({...reply,pending:found?Object.freeze({...found}):null});
    }catch(error){this.#fail(epoch);throw error;}finally{this.#finish(logical);}
  }
  async submit(operation:string,sequence:string,bytes:Uint8Array):Promise<Extract<Reply,{message:'InputReceipt'|'InputNotAccepted'}>> {
    const window=this.#window;if(!window)throw Error('no current input window');const payload=input(operation,sequence,window.token,window.notAfterHostMs,bytes);
    const {native,epoch,operation:logical}=this.#enter(['Ready']);this.#window=undefined;const pending:Pending={operation,sequence};this.#pending=pending;
    try{
      const reply=decodeReply(await this.#wait(native.input(payload),epoch));this.#check(epoch);
      if(reply.message!=='InputReceipt'&&reply.message!=='InputNotAccepted')throw Error('input reply');this.#identity(reply,pending);
      if(reply.message==='InputNotAccepted'){this.#pending=undefined;this.#phase='Synced';}
      else{pending.receipt={operation,sequence,outcome:reply.outcome,proof:reply.proof};this.#phase='NeedsSettlement';}
      return Object.freeze(reply);
    }catch(error){this.#fail(epoch);throw error;}finally{this.#finish(logical);}
  }
  async query():Promise<Extract<Reply,{message:'ReceiptStatus'}>> {
    const pending=this.#pending;if(!pending)throw Error('no pending receipt');const {native,epoch,operation:logical}=this.#enter(['NeedsSettlement'],true);
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
    }catch(error){this.#fail(epoch);throw error;}finally{this.#finish(logical);}
  }
  async settle():Promise<void> {
    const pending=this.#pending;if(!pending?.receipt)throw Error('no final receipt');
    // 保留原语法拒绝不改变结算状态，但尚不占用共享请求序号。
    receiptAck('1',pending.receipt);
    const {native,epoch,operation:logical}=this.#enter(['NeedsSettlement'],true);
    try{
      const request=this.#next(),payload=receiptAck(request,pending.receipt);
      const reply=decodeReply(await this.#wait(native.settle(payload),epoch));this.#check(epoch);
      if(reply.message!=='ReceiptSettled'||reply.request!==request)throw Error('settlement mismatch');this.#identity(reply,pending);
      if(reply.outcome!==pending.receipt.outcome)throw Error('settlement outcome');
      if(reply.disposition==='Rejected'){this.#phase='NeedsSettlement';return;}
      this.#pending=undefined;this.#window=undefined;this.#receiver.disconnect();this.#phase='Idle';
    }catch(error){this.#fail(epoch);throw error;}finally{this.#finish(logical);}
  }
}
