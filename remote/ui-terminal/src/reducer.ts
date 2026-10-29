// Semantic consumer only. Native authenticated, bounded record decode precedes
// this adapter. No VT parser, terminal replies, timers or input authority here.
export type Target = { host:string; session:string; run:string; generation:string; incarnation:string };
export type Stamp = { target:Target; binding:string; barrier:string; revision:string;
  dimensionEpoch:string; modeEpoch:string; structureEpoch:string; syncEpoch:string };
export type Color = { Palette:number } | { Rgb:[number,number,number] };
export type Cell = { c:string; r:string; w:boolean; f:Color; b:Color; s:number };
export type State = { rows:number; columns:number; buffer:string; focusReporting:boolean;
  cursor:{row:number; column:number; shape:string; blinking:boolean} };
export type Frame = { stamp:Stamp; state:State; cells:Cell[] };
const installedFrames=new WeakSet<object>();
export const isInstalledFrame=(value:unknown):value is Frame => !!value&&typeof value==='object'&&installedFrames.has(value);
type ObjectValue = Record<string, any>;
const object = (v:unknown):v is ObjectValue => !!v && typeof v === 'object' && !Array.isArray(v);
const integer = (v:unknown,lo:number,hi:number):v is number => Number.isInteger(v) && (v as number)>=lo && (v as number)<=hi;
const hex = (v:unknown,n:number):v is string => typeof v==='string' && v.length===n && /^[0-9a-f]+$/.test(v);
const decimal = (v:unknown):v is string => typeof v==='string' && v.length<=20 && /^(0|[1-9][0-9]{0,19})$/.test(v) && BigInt(v)<=18446744073709551615n;
const counters = ['revision','dimensionEpoch','modeEpoch','structureEpoch','syncEpoch'] as const;
function target(t:unknown):t is Target {
  return object(t) && hex(t.host,64) && hex(t.session,32) && typeof t.run==='string' && t.run.length>0 && t.run.length<=128 &&
    new TextEncoder().encode(t.run).length<=128 && !/[\p{Cc}\p{Cs}]/u.test(t.run) && decimal(t.generation) && decimal(t.incarnation);
}
function stamp(s:unknown):s is Stamp {
  return object(s) && target(s.target) && hex(s.binding,64) && hex(s.barrier,32) && counters.every(k=>decimal(s[k]));
}
function sameTarget(a:Target,b:Target) { return a.host===b.host && a.session===b.session && a.run===b.run && a.generation===b.generation && a.incarnation===b.incarnation; }
function sameStamp(a:Stamp,b:Stamp) { return sameTarget(a.target,b.target) && a.binding===b.binding && a.barrier===b.barrier && counters.every(k=>a[k]===b[k]); }
function copyStamp(s:Stamp):Stamp { return { target:{host:s.target.host,session:s.target.session,run:s.target.run,generation:s.target.generation,incarnation:s.target.incarnation},
  binding:s.binding,barrier:s.barrier,revision:s.revision,dimensionEpoch:s.dimensionEpoch,modeEpoch:s.modeEpoch,structureEpoch:s.structureEpoch,syncEpoch:s.syncEpoch }; }
function state(s:unknown):s is State { return object(s) && integer(s.rows,1,100) && integer(s.columns,2,240) &&
  ['Normal','Alternate'].includes(s.buffer) && typeof s.focusReporting==='boolean' && object(s.cursor) &&
  integer(s.cursor.row,0,s.rows-1) && integer(s.cursor.column,0,s.columns-1) &&
  ['Block','Underline','Beam','HollowBlock','Hidden'].includes(s.cursor.shape) && typeof s.cursor.blinking==='boolean'; }
function copyState(s:State):State { return {rows:s.rows,columns:s.columns,buffer:s.buffer,focusReporting:s.focusReporting,
  cursor:{row:s.cursor.row,column:s.cursor.column,shape:s.cursor.shape,blinking:s.cursor.blinking}}; }
function sameState(a:State,b:State) { return JSON.stringify(a)===JSON.stringify(b); }
function color(c:unknown):c is Color { return object(c) && Object.keys(c).length===1 &&
  (integer(c.Palette,0,268) || Array.isArray(c.Rgb) && c.Rgb.length===3 && c.Rgb.every((v:unknown)=>integer(v,0,255))); }
function copyColor(c:Color):Color { return 'Palette' in c ? {Palette:c.Palette} : {Rgb:[...c.Rgb]}; }
function cell(c:unknown):c is Cell { return object(c) && typeof c.c==='string' && c.c.length<=2 && [...c.c].length===1 &&
  !/[\p{Cc}\p{Cs}]/u.test(c.c) && ['Single','WideLead','WideSpacer','LeadingWideSpacer'].includes(c.r) &&
  typeof c.w==='boolean' && color(c.f) && color(c.b) && integer(c.s,0,383); }
function copyCell(c:Cell):Cell { return {c:c.c,r:c.r,w:c.w,f:copyColor(c.f),b:copyColor(c.b),s:c.s}; }
function freeze<T>(v:T):T { if (v && typeof v==='object') {Object.values(v).forEach(freeze);Object.freeze(v);} return v; }
function roles(cells:Cell[],columns:number):boolean {
  return cells.every((c,i)=> (!c.w || i%columns===columns-1) &&
    (c.r!=='WideLead' || i%columns<columns-1 && cells[i+1]?.r==='WideSpacer') &&
    (c.r!=='WideSpacer' || i%columns>0 && cells[i-1]?.r==='WideLead') &&
    (c.r!=='LeadingWideSpacer' || i%columns===columns-1));
}
function sameColor(a:Color,b:Color):boolean {return 'Palette' in a ? 'Palette' in b&&a.Palette===b.Palette :
  'Rgb' in b&&a.Rgb.every((n,i)=>n===b.Rgb[i]);}
function sameCells(a:Cell[],b:Cell[]):boolean {return a.length===b.length&&a.every((c,i)=>{
  const d=b[i];return c.c===d.c&&c.r===d.r&&c.w===d.w&&c.s===d.s&&sameColor(c.f,d.f)&&sameColor(c.b,d.b);
});}
function replacementConsistent(old:Frame,next:Frame):boolean {
  const a=old.stamp,b=next.stamp,x=old.state,y=next.state;
  if(x.focusReporting!==y.focusReporting&&b.modeEpoch===a.modeEpoch)return false;
  if((x.rows!==y.rows||x.columns!==y.columns)&&(b.dimensionEpoch===a.dimensionEpoch||b.structureEpoch===a.structureEpoch))return false;
  if(x.buffer!==y.buffer&&(b.structureEpoch===a.structureEpoch||b.modeEpoch===a.modeEpoch))return false;
  if(b.revision===a.revision) return ['dimensionEpoch','modeEpoch','structureEpoch'].every(k=>a[k as keyof Stamp]===b[k as keyof Stamp])&&
    sameState(x,y)&&sameCells(old.cells,next.cells);
  return true;
}
export class ProjectionReceiver {
  #target:Target; #binding:string; #frame:Frame|undefined; #synchronized=false;
  #announced:Stamp|undefined; #pending:{stamp:Stamp;state:State;cells:Cell[]}|undefined;
  constructor(t:Target,binding:string) {
    if (!target(t)||!hex(binding,64)) throw Error('invalid context');
    this.#target={...t};this.#binding=binding;
  }
  get frame():Frame|undefined {return this.#frame;}
  get synchronized():boolean {return this.#synchronized;}
  // Called only for an exact verified Host barrier announcement. Frames cannot
  // select their own target/binding/barrier and enable an unsolicited install.
  begin(s:Stamp):void {
    this.disconnect();
    if (!stamp(s)||!this.#context(s)) throw Error('invalid barrier');
    const old=this.#frame?.stamp;
    if (old && (BigInt(s.syncEpoch)<=BigInt(old.syncEpoch) || counters.some(k=>BigInt(s[k])<BigInt(old[k])))) throw Error('stale barrier');
    this.#announced=freeze(copyStamp(s));
  }
  disconnect():void {this.#synchronized=false;this.#pending=undefined;this.#announced=undefined;}
  ack():Stamp|undefined {return this.#synchronized && this.#frame ? freeze(copyStamp(this.#frame.stamp)):undefined;}
  #context(s:Stamp) {return sameTarget(this.#target,s.target)&&s.binding===this.#binding;}
  #gap():'Gap' {this.disconnect();return 'Gap';}
  accept(input:unknown):'Partial'|'Installed'|'Delta'|'Gap' {
    if (!object(input)||input.profile!==1||!object(input.body)) return this.#gap();
    const b=input.body;
    if (!stamp(b.stamp)||!this.#context(b.stamp)||!state(b.state)) return this.#gap();
    const s=copyStamp(b.stamp),v=copyState(b.state),total=v.rows*v.columns;
    if (input.message==='SnapshotPart') {
      if (!this.#announced||!sameStamp(s,this.#announced)||!integer(b.start,0,total-1)||!Array.isArray(b.cells)||
          b.cells.length===0||b.cells.length>128||b.start+b.cells.length>total||!b.cells.every(cell)) return this.#gap();
      if (!this.#pending) {
        if (b.start!==0) return this.#gap();
        this.#pending={stamp:s,state:v,cells:[]};
      }
      const p=this.#pending;
      if (b.start!==p.cells.length||!sameStamp(p.stamp,s)||!sameState(p.state,v)) return this.#gap();
      p.cells.push(...b.cells.map(copyCell));
      if (p.cells.length!==total) return 'Partial';
      if (!roles(p.cells,v.columns)) return this.#gap();
      if (this.#frame&&!replacementConsistent(this.#frame,p)) return this.#gap();
      this.#frame=freeze(p);this.#pending=undefined;this.#announced=undefined;this.#synchronized=true;
      installedFrames.add(this.#frame);
      return 'Installed';
    }
    if (input.message==='Delta') {
      const old=this.#frame;
      if (!old||!this.#synchronized||!decimal(b.base)||b.base!==old.stamp.revision||BigInt(s.revision)<=BigInt(b.base)||
          s.barrier!==old.stamp.barrier||s.dimensionEpoch!==old.stamp.dimensionEpoch||s.structureEpoch!==old.stamp.structureEpoch||
          s.syncEpoch!==old.stamp.syncEpoch||BigInt(s.modeEpoch)<BigInt(old.stamp.modeEpoch)||v.rows!==old.state.rows||
          v.columns!==old.state.columns||v.buffer!==old.state.buffer||
          (v.focusReporting!==old.state.focusReporting&&s.modeEpoch===old.stamp.modeEpoch)||
          !Array.isArray(b.changes)||b.changes.length>128) return this.#gap();
      let previous=-1;
      for (const c of b.changes) {
        if (!object(c)||!integer(c.index,0,total-1)||c.index<=previous||!cell(c.cell)) return this.#gap();
        previous=c.index;
      }
      const cells=old.cells.slice();for(const c of b.changes) cells[c.index]=copyCell(c.cell);
      if (!roles(cells,v.columns)) return this.#gap();
      this.#frame=freeze({stamp:s,state:v,cells});installedFrames.add(this.#frame);return 'Delta';
    }
    return this.#gap();
  }
}
