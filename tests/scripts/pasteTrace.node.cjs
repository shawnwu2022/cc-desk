const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');
const ts = require('typescript');
const Module = require('node:module');
const file = path.resolve(__dirname, '../../src/utils/pasteTrace.ts');
const loaded = new Module(file, module);
loaded.filename = file;
loaded.paths = module.paths;
loaded._compile(ts.transpileModule(fs.readFileSync(file, 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText, file);
const { PasteTrace } = loaded.exports;

function clocked(enabled = true) {
  let time = 0;
  let n = 0;
  const trace = new PasteTrace(enabled, () => time, () => `00000000-0000-4000-8000-${String(++n).padStart(12, '0')}`);
  return { trace, advance: ms => { time += ms; } };
}

test('PasteTrace_DisabledIsNoOp_001', () => {
  const { trace } = clocked(false);
  const ticket = trace.begin('pty-a');
  assert.equal(trace.observe(ticket, 'pty-a', 'secret', () => trace.input('pty-a', 'secret')), undefined);
});

test('PasteTrace_ClipboardReferenceAtActualSend_002', () => {
  const { trace } = clocked();
  const ticket = trace.begin('pty-a');
  const wire = '\x1b[200~head\n尾\x1b[201~';
  const meta = trace.observe(ticket, 'pty-a', 'head\n尾', () => trace.input('pty-a', wire));
  assert.ok(meta, 'diagnostic input needs a transaction');
  assert.equal(meta.expected, 'head\n尾');
  assert.equal(meta.bytes, Buffer.byteLength(wire));
  assert.equal(meta.clipboard, true);
});

test('PasteTrace_InterleavedInputPrecedesClipboard_003', () => {
  const { trace, advance } = clocked();
  const ticket = trace.begin('pty-a');
  advance(20);
  const control = trace.input('pty-a', '\r');
  const paste = trace.observe(ticket, 'pty-a', 'head', () => trace.input('pty-a', '\x1b[200~head\x1b[201~'));
  assert.ok(control && paste);
  assert.ok(control.seq < paste.seq);
  assert.equal(control.pasteId, paste.pasteId);
  assert.equal(control.expected, undefined);
  assert.equal(paste.ageMs, 20);
});

test('PasteTrace_ReferenceDoesNotLeakIntoNextInput_004', () => {
  const { trace } = clocked();
  const ticket = trace.begin('pty-a');
  trace.observe(ticket, 'pty-a', 'private content', () => trace.input('pty-a', 'private content'));
  const next = trace.input('pty-a', 'keystroke');
  assert.ok(next);
  assert.equal(next.expected, undefined);
  assert.equal(JSON.stringify(trace).includes('private content'), false);
  assert.equal(JSON.stringify(next).includes('keystroke'), false);
});

test('PasteTrace_ThrowClearsScopedReference_005', () => {
  const { trace } = clocked();
  const ticket = trace.begin('pty-a');
  const error = new Error('transport failure');
  assert.throws(() => trace.observe(ticket, 'pty-a', 'secret', () => { throw error; }), e => e === error);
  assert.equal(trace.input('pty-a', 'next')?.expected, undefined);
});

test('PasteTrace_SameTabOverlapsKeepOwnIds_006', () => {
  const { trace } = clocked();
  const first = trace.begin('pty-a');
  const second = trace.begin('pty-a');
  const a = trace.observe(first, 'pty-a', 'one', () => trace.input('pty-a', 'one'));
  const b = trace.observe(second, 'pty-a', 'two', () => trace.input('pty-a', 'two'));
  assert.ok(a && b);
  assert.notEqual(a.pasteId, b.pasteId);
  assert.equal(a.expected, 'one');
  assert.equal(b.expected, 'two');
});

test('PasteTrace_DifferentPtyNeverInheritsClipboard_007', () => {
  const { trace } = clocked();
  const ticket = trace.begin('pty-a');
  assert.equal(trace.observe(ticket, 'pty-a', 'secret', () => trace.input('pty-b', 'x')), undefined);
});

test('PasteTrace_ExpiresAndCannotRearm_008', () => {
  const { trace, advance } = clocked();
  const ticket = trace.begin('pty-a');
  advance(60_001);
  assert.equal(trace.observe(ticket, 'pty-a', 'secret', () => trace.input('pty-a', 'secret')), undefined);
  assert.equal(trace.begin('pty-a'), undefined);
  assert.equal(trace.input('pty-a', 'x'), undefined);
});

test('PasteTrace_GlobalEventBudget_009', () => {
  const { trace } = clocked();
  trace.begin('pty-a');
  let count = 0;
  for (let i = 0; i < 1024; i++) if (trace.input('pty-a', '\r')) count++;
  assert.equal(count, 256);
  trace.begin('pty-b');
  assert.equal(trace.input('pty-b', 'next'), undefined);
});

test('PasteTrace_NoExtraAwaitOrResend_010', async () => {
  const { trace } = clocked();
  const ticket = trace.begin('pty-a');
  let sends = 0;
  const promise = Promise.resolve(false);
  const returned = trace.observe(ticket, 'pty-a', 'text', () => { sends++; return promise; });
  assert.equal(returned, promise);
  assert.equal(sends, 1);
  assert.equal(await returned, false);
});

function loadPaste() {
  const filename = path.resolve(__dirname, '../../src/utils/pasteText.ts');
  const m = new Module(filename, module);
  m.filename = filename;
  m.paths = module.paths;
  m._compile(ts.transpileModule(fs.readFileSync(filename, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText, filename);
  return m.exports;
}

function attach(paste, trace) {
  assert.equal(typeof paste.setPasteObserver, 'function', 'production paste path needs an opt-in observation hook');
  paste.setPasteObserver(id => {
    const ticket = trace.begin(id);
    return ticket ? (target, expected, send) => trace.observe(ticket, target, expected, send) : undefined;
  });
}

test('PasteTrace_ProductionCommitMatchesNormalizedClipboard_011', async () => {
  const paste = loadPaste();
  const { trace } = clocked();
  attach(paste, trace);
  const text = 'Error: value\r\n\tat x\r(匿名)\\Item.ts';
  const sent = [];
  await paste.commitPaste(async () => text, () => ({ ptyId: 'a' }),
    value => paste.buildPastePayload(value, true, false),
    (id, data) => { sent.push({ data, meta: trace.input(id, data) }); return Promise.resolve(true); });
  assert.equal(sent.length, 1);
  assert.equal(sent[0].data, '\x1b[200~' + text.replace(/\r\n?/g, '\n') + '\x1b[201~');
  assert.equal(sent[0].meta.expected, text.replace(/\r\n?/g, '\n'));
});

test('PasteTrace_ReadRaceAndStaleTarget_012', async () => {
  const paste = loadPaste();
  const { trace } = clocked();
  attach(paste, trace);
  let resolve;
  let id = 'old';
  let sends = 0;
  const pending = paste.commitPaste(() => new Promise(r => { resolve = r; }), () => ({ ptyId: id }),
    value => paste.buildPastePayload(value, true, false), async () => { sends++; });
  assert.ok(trace.input('old', '\r'), 'observation must start before asynchronous clipboard read');
  id = 'new';
  resolve('full clipboard');
  await pending;
  assert.equal(sends, 0);
  assert.equal(trace.input('new', 'x'), undefined);
});

test('PasteTrace_ImageFallbackIsNotText_013', async () => {
  const paste = loadPaste();
  const { trace } = clocked();
  attach(paste, trace);
  const sent = [];
  await paste.commitPaste(async () => '', () => ({ ptyId: 'a' }), value => value,
    (id, data) => { sent.push({ data, meta: trace.input(id, data) }); return Promise.resolve(true); },
    () => '\x1bv');
  assert.equal(sent.length, 1);
  assert.equal(sent[0].data, '\x1bv');
  assert.equal(sent[0].meta.clipboard, false);
  assert.equal(sent[0].meta.expected, undefined);
});

test('PasteTrace_ProductionFailureRemainsFailure_014', async () => {
  const paste = loadPaste();
  const { trace } = clocked();
  attach(paste, trace);
  const error = new Error('write failed');
  let sends = 0;
  await assert.rejects(paste.commitPaste(async () => 'full', () => ({ ptyId: 'a' }), value => value,
    async () => { sends++; throw error; }), e => e === error);
  assert.equal(sends, 1);
  assert.equal(trace.input('a', 'x').expected, undefined);
});

function productionApi(enabled, observed) {
  const paste = loadPaste();
  const trace = new PasteTrace();
  const filename = process.env.CC_TRACE_API_BASELINE || path.resolve(__dirname, '../../src/api/tauri.ts');
  const m = new Module(filename, module);
  m.filename = filename;
  m.require = name => {
    if (name === '@/utils/pasteTrace') return { pasteTrace: trace };
    if (name === '@/utils/pasteText') return paste;
    if (name === '@tauri-apps/api/core') return { invoke: (cmd, args) => { observed.push({ cmd, args }); return Promise.resolve(true); } };
    if (name.startsWith('@tauri-apps/')) return {};
    throw new Error(`Unexpected import: ${name}`);
  };
  const source = fs.readFileSync(filename, 'utf8').replace('import.meta.env.VITE_CC_DESK_PASTE_TRACE', JSON.stringify(enabled ? '1' : ''));
  m._compile(ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText, filename);
  return { api: m.exports, paste };
}

test('PasteTrace_RealIpcWrapperCarriesReference_015', async () => {
  const observed = [];
  const { api, paste } = productionApi(true, observed);
  const expected = 'first\n(匿名)\tlast';
  await paste.commitPaste(async () => expected, () => ({ ptyId: 'a' }),
    value => paste.buildPastePayload(value, true, false),
    (id, data) => api.ptyInput(id, data, 'clipboard-keyboard'));
  assert.equal(observed.length, 1, 'diagnostics must not insert an extra IPC round trip');
  assert.equal(observed[0].cmd, 'pty_input');
  assert.equal(observed[0].args.data, '\x1b[200~' + expected + '\x1b[201~');
  assert.ok(observed[0].args.trace, 'actual API wrapper must carry correlation metadata');
  assert.equal(observed[0].args.trace.expected, expected);
});

test('PasteTrace_DefaultIpcUnchanged_016', async () => {
  const observed = [];
  const { api, paste } = productionApi(false, observed);
  await paste.commitPaste(async () => 'unchanged', () => ({ ptyId: 'a' }), value => value,
    (id, data) => api.ptyInput(id, data, 'clipboard-keyboard'));
  assert.deepEqual(observed, [{ cmd: 'pty_input', args: { id: 'a', data: 'unchanged', source: 'clipboard-keyboard' } }]);
});
