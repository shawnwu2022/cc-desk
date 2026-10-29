import type { Frame } from './reducer.ts';
import { isInstalledFrame } from './reducer.ts';
type ElementLike = { textContent:string|null; className:string; style:any;
  setAttribute(name:string,value:string):void; appendChild(child:any):any };
type DocumentLike = {createElement(tag:string):ElementLike};
// Output must be an installed reducer frame. createElement/textContent and fixed
// numeric CSS are the only effects: no HTML, links, clipboard, VT or native IPC.
export function render(document:DocumentLike, frame:Frame, palette:readonly string[]=defaultPalette()):ElementLike {
  if(!isInstalledFrame(frame)||palette.length!==269||palette.some(c=>!/^#[0-9a-fA-F]{6}$/.test(c))) throw Error('invalid render state');
  const root=document.createElement('div');root.className='remote-terminal-grid';
  root.style.display='grid';root.style.gridTemplateColumns=`repeat(${frame.state.columns}, 1ch)`;
  root.style.whiteSpace='pre';root.setAttribute('role','img');
  root.setAttribute('aria-label',Array.from({length:frame.state.rows},(_,row)=>frame.cells.slice(row*frame.state.columns,(row+1)*frame.state.columns)
    .map(c=>c.r==='WideSpacer'?'':c.r==='LeadingWideSpacer'||(c.s&16)?' ':c.c).join('')).join('\n'));
  frame.cells.forEach((c,i)=>{
    const span=document.createElement('span');span.textContent=c.r==='Single'||c.r==='WideLead'?c.c:'';
    span.setAttribute('aria-hidden','true');
    span.style.gridRow=String(Math.floor(i/frame.state.columns)+1);
    span.style.gridColumn=`${i%frame.state.columns+1} / span ${c.r==='WideLead'?2:1}`;
    span.style.overflow='hidden';
    span.className=`cell role-${c.r} style-${c.s}`;
    const [fg,bg]=(c.s&8)?[c.b,c.f]:[c.f,c.b];
    for (const [name,color] of [['color',fg],['backgroundColor',bg]] as const) {
      span.style[name]='Rgb' in color?`rgb(${color.Rgb.join(',')})`:palette[color.Palette];
    }
    span.style.fontWeight=(c.s&1)?'bold':'normal';span.style.fontStyle=(c.s&4)?'italic':'normal';
    if(c.s&2) span.style.opacity='0.66';
    const underline=c.s>>6;
    span.style.textDecorationLine=[...(underline?['underline']:[]),...(c.s&32?['line-through']:[])].join(' ');
    span.style.textDecorationStyle=['solid','solid','double','wavy','dotted','dashed'][underline];
    if(c.s&16) span.style.color='transparent';
    // Wide spacer occupies a Host cell but must not cover the preceding glyph.
    if (c.r==='WideSpacer') span.style.visibility='hidden';
    root.appendChild(span);
  });
  const cursor=frame.state.cursor;
  if(cursor.shape!=='Hidden') {
    const marker=document.createElement('span');marker.textContent='';
    marker.setAttribute('aria-hidden','true');
    marker.className=`cursor cursor-${cursor.shape}${cursor.blinking?' cursor-blinking':''}`;
    marker.style.gridRow=String(cursor.row+1);marker.style.gridColumn=String(cursor.column+1);
    marker.style.pointerEvents='none';root.appendChild(marker);
  }
  return root;
}
// Explicit trial theme. A signed shell may inject a validated269-entry theme;
// exact desktop palette/IME/browser shaping remains a separate real-device gate.
function defaultPalette():string[] {
  const base=['#000000','#cd0000','#00cd00','#cdcd00','#0000ee','#cd00cd','#00cdcd','#e5e5e5',
    '#7f7f7f','#ff0000','#00ff00','#ffff00','#5c5cff','#ff00ff','#00ffff','#ffffff'];
  const hex=(n:number)=>n.toString(16).padStart(2,'0');const cube=[0,95,135,175,215,255];
  const colors=[...base];for(const r of cube)for(const g of cube)for(const b of cube)colors.push(`#${hex(r)}${hex(g)}${hex(b)}`);
  for(let i=0;i<24;i++){const h=hex(8+10*i);colors.push(`#${h}${h}${h}`);}
  colors.push('#e5e5e5','#000000','#ffffff');
  for(const c of base.slice(0,8))colors.push('#'+[1,3,5].map(i=>hex(Math.floor(parseInt(c.slice(i,i+2),16)/2))).join(''));
  colors.push('#727272','#ffffff');return colors;
}
