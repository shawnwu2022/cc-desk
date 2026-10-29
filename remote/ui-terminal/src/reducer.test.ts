import test from 'node:test';
import assert from 'node:assert/strict';
import { ProjectionReceiver } from './reducer.ts';
import { render } from './renderer.ts';
import { readFileSync } from 'node:fs';

function fixture() {
  return JSON.parse(readFileSync(new URL('../../../protocol/remote/inner/v1/test-vectors/projection-snapshot.json',import.meta.url),'utf8'));
}
function receiver(f = fixture()) {
  const r = new ProjectionReceiver(f.body.stamp.target, f.body.stamp.binding);
  r.begin(f.body.stamp); return r;
}
test('snapshot is installed atomically and ACK preserves exact u64 strings', () => {
  const f = fixture(), r = receiver(f); const tail = structuredClone(f);
  f.body.cells.pop(); tail.body.cells.shift(); tail.body.start = 1;
  assert.equal(r.accept(f), 'Partial'); assert.equal(r.frame, undefined); assert.equal(r.ack(), undefined);
  assert.equal(r.accept(tail), 'Installed'); assert.equal(r.ack()?.revision, '9007199254740993');
  assert.equal(r.frame?.cells[0].c, '<'); assert.equal(r.synchronized, true);
});
test('reordered duplicate missing and mismatched chunks cannot install or ACK', () => {
  for (const fault of ['start', 'binding', 'barrier', 'target', 'mode', 'dimensions']) {
    const f = fixture(), r = receiver(f);
    if (fault === 'start') f.body.start = 1;
    if (fault === 'binding') f.body.stamp.binding = '55'.repeat(32);
    if (fault === 'barrier') f.body.stamp.barrier = '55'.repeat(16);
    if (fault === 'target') f.body.stamp.target.host = '55'.repeat(32);
    if (fault === 'mode') f.body.stamp.modeEpoch = '1';
    if (fault === 'dimensions') f.body.state.columns = 3;
    assert.equal(r.accept(f), fault === 'dimensions' ? 'Partial' : 'Gap');
    assert.equal(r.ack(), undefined); assert.equal(r.synchronized, false);
  }
  const f = fixture(), r = receiver(f); f.body.cells.pop();
  assert.equal(r.accept(f), 'Partial'); assert.equal(r.accept(f), 'Gap'); assert.equal(r.ack(), undefined);
});
test('delta exact base mode-only change and structural gap preserve atomic install', () => {
  const f = fixture(), r = receiver(f); r.accept(f);
  const d = { profile: 1, message: 'Delta', body: { stamp: structuredClone(f.body.stamp),
    state: structuredClone(f.body.state), base: f.body.stamp.revision, changes: [] } };
  d.body.stamp.revision = '9007199254740994'; d.body.stamp.modeEpoch = '1'; d.body.state.focusReporting = true;
  assert.equal(r.accept(d), 'Delta'); assert.equal(r.frame?.stamp.modeEpoch, '1');
  assert.equal(r.frame?.cells[0].c, '<');
  d.body.base = d.body.stamp.revision; d.body.stamp.revision = '9007199254740995'; d.body.stamp.structureEpoch = '1';
  assert.equal(r.accept(d), 'Gap'); assert.equal(r.synchronized, false);
  assert.equal(r.frame?.stamp.revision, '9007199254740994');
});
test('stale base future context and repeated delta do not become new effects', () => {
  for (const fault of ['base', 'binding', 'generation', 'repeat']) {
    const f = fixture(), r = receiver(f); r.accept(f);
    const d = { profile: 1, message: 'Delta', body: { stamp: structuredClone(f.body.stamp), state: f.body.state,
      base: f.body.stamp.revision, changes: [{ index: 0, cell: { ...f.body.cells[0], c: 'x' } }] } };
    d.body.stamp.revision = '9007199254740994';
    if (fault === 'base') d.body.base = '0';
    if (fault === 'binding') d.body.stamp.binding = '66'.repeat(32);
    if (fault === 'generation') d.body.stamp.target.generation = '2';
    if (fault === 'repeat') assert.equal(r.accept(d), 'Delta');
    assert.equal(r.accept(d), 'Gap'); assert.equal(r.synchronized, false);
  }
});
test('bad geometry role pairing style and scalar never replace a visible frame', () => {
  for (const fault of ['style', 'control', 'surrogate', 'wide', 'size', 'count']) {
    const f = fixture(), r = receiver(f);
    if (fault === 'style') f.body.cells[0].s = 65535;
    if (fault === 'control') f.body.cells[0].c = '\x1b';
    if (fault === 'surrogate') f.body.cells[0].c = '\ud800';
    if (fault === 'wide') f.body.cells[0].r = 'WideSpacer';
    if (fault === 'size') f.body.state.columns = 1;
    if (fault === 'count') f.body.cells = Array(129).fill(f.body.cells[0]);
    assert.equal(r.accept(f), 'Gap'); assert.equal(r.frame, undefined); assert.equal(r.ack(), undefined);
  }
});
test('disconnect and new announced barrier require a completely installed replacement', () => {
  const f = fixture(), r = receiver(f); r.accept(f); r.disconnect();
  assert.equal(r.synchronized, false); assert.equal(r.ack(), undefined);
  assert.equal(r.accept(f), 'Gap');
  f.body.stamp.barrier = '77'.repeat(16); f.body.stamp.syncEpoch = '2';
  r.begin(f.body.stamp); assert.equal(r.accept(f), 'Installed'); assert.equal(r.ack()?.syncEpoch, '2');
});
test('caller mutation cannot rewrite retained state or its exact ACK', () => {
  const f = fixture(), r = receiver(f); r.accept(f); f.body.stamp.revision = '1'; f.body.cells[0].c = 'x';
  assert.equal(r.frame?.cells[0].c, '<'); assert.equal(r.ack()?.revision, '9007199254740993');
  assert.throws(() => { r.frame!.cells[0].c = 'x'; });
});
test('replacement snapshot cannot bypass state epoch or same-revision content checks',()=>{
  for(const fault of ['focus','dimensions','buffer','cells','cursor','epochs']) {
    const f=fixture(),r=receiver(f);assert.equal(r.accept(f),'Installed');
    const next=structuredClone(f);next.body.stamp.syncEpoch='2';next.body.stamp.barrier='88'.repeat(16);
    if(['focus','dimensions','buffer'].includes(fault))next.body.stamp.revision='9007199254740994';
    if(fault==='focus')next.body.state.focusReporting=true;
    if(fault==='dimensions'){next.body.state.columns=3;next.body.cells.push({...next.body.cells[1]});}
    if(fault==='buffer')next.body.state.buffer='Alternate';
    if(fault==='cells')next.body.cells[0].c='x';
    if(fault==='cursor')next.body.state.cursor.column=1;
    if(fault==='epochs')next.body.stamp.modeEpoch='1';
    r.begin(next.body.stamp);assert.equal(r.accept(next),'Gap',fault);
    assert.equal(r.frame?.cells[0].c,'<');assert.equal(r.ack(),undefined);
  }
  const f=fixture(),r=receiver(f);r.accept(f);
  f.body.stamp.barrier='99'.repeat(16);f.body.stamp.syncEpoch='2';r.begin(f.body.stamp);
  assert.equal(r.accept(f),'Installed'); // a fresh barrier for the same exact prefix
  f.body.stamp.barrier='aa'.repeat(16);f.body.stamp.syncEpoch='3';f.body.stamp.modeEpoch='2';
  f.body.stamp.revision='9007199254740994';r.begin(f.body.stamp);
  assert.equal(r.accept(f),'Installed'); // two actual mode toggles may end unchanged
});
class Element {
  children: Element[] = []; textContent = ''; className = ''; style: Record<string,string> = {};
  attributes:Record<string,string>={};
  set innerHTML(_: string) { throw Error('HTML sink forbidden'); }
  setAttribute(name: string, value: string) { assert.ok(['role','aria-label','aria-hidden'].includes(name));this.attributes[name]=value; }
  appendChild(child: Element) { this.children.push(child); return child; }
}
test('renderer uses inert text and Host roles rather than parsing terminal commands or markup', () => {
  const f = fixture(), r = receiver(f); r.accept(f);
  const created: Element[] = []; const doc = { createElement(tag: string) {
    assert.ok(['div','span'].includes(tag)); const e = new Element(); created.push(e); return e;
  } };
  const node = render(doc, r.frame!);
  assert.equal(node.children[0].textContent, '<'); assert.equal(created.length, 4);
  assert.equal(node.style.gridTemplateColumns, 'repeat(2, 1ch)');
  assert.equal(node.children[2].className,'cursor cursor-Block');
});
test('renderer applies bounded SGR colors styles and wide cells without client width inference',()=>{
  const f=fixture(),r=receiver(f);f.body.cells[0]={...f.body.cells[0],c:'中',r:'WideLead',f:{Rgb:[1,2,3]},s:1|4|32|64};
  f.body.cells[1].r='WideSpacer';assert.equal(r.accept(f),'Installed');
  const doc={createElement(){return new Element();}};const node=render(doc,r.frame!);
  assert.equal(node.children[0].style.color,'rgb(1,2,3)');assert.equal(node.children[0].style.fontWeight,'bold');
  assert.equal(node.children[0].style.fontStyle,'italic');assert.equal(node.children[0].style.textDecorationLine,'underline line-through');
  assert.equal(node.children[0].style.gridColumn,'1 / span 2');assert.equal(node.children[1].style.visibility,'hidden');
});
test('hidden cells are suppressed from both the accessible label and child accessibility tree',()=>{
  const f=fixture(),r=receiver(f);f.body.cells[0].c='Z';f.body.cells[0].s=16;r.accept(f);
  const node=render({createElement(){return new Element();}},r.frame!);
  assert.equal(node.attributes['aria-label'],'  ');assert.equal(node.children[0].attributes['aria-hidden'],'true');
  assert.throws(()=>render({createElement(){throw Error('must not reach DOM');}},{...r.frame!}));
});
