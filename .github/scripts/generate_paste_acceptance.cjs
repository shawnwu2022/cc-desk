const fs = require('node:fs');
const path = require('node:path');

const root = process.cwd();
const ts = require(path.join(root, 'node_modules/typescript'));
const compiled = ts.transpileModule(fs.readFileSync(path.join(root, 'src/utils/pasteText.ts'), 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
fs.mkdirSync(path.join(root, '.ci-claude'), { recursive: true });
const compiledPath = path.join(root, '.ci-claude/pasteText.cjs');
fs.writeFileSync(compiledPath, compiled);
const { buildPastePayload } = require(compiledPath);
const fixtures = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tests/fixtures/devtools-paste-framing.json'), 'utf8'));
const cases = [];
function add(name, source, isJson = false) {
  if (cases.some(c => c.name === name)) throw new Error(`Duplicate acceptance case: ${name}`);
  if (isJson) JSON.parse(source); // Never parse/stringify the text being transported.
  cases.push({ name, wire: buildPastePayload(source, true, false), expected: source.replace(/\r\n?/g, '\n'), isJson });
}
for (const fixture of fixtures) {
  if (fixture.cliAcceptance === false) {
    if (!fixture.cliSkipReason) throw new Error(`Missing explicit CLI skip reason: ${fixture.name}`);
    console.log('Transport-only fixture:', fixture.name, fixture.cliSkipReason);
    continue;
  }
  add(fixture.name, (fixture.prefix || '') + Array(fixture.repeat).fill(fixture.source).join(fixture.separator || '') + (fixture.suffix || ''), fixture.isJson === true);
}

// Existing large JSON regression. Metrics only; no user's business data.
const lines = ['{', '"items":['];
for (let index = 0; index < 3574; index += 1) lines.push('0,');
lines.push('0', '],', '"tail":""}');
const filler = 106002 - Buffer.byteLength(lines.join('\n'));
if (filler < 0) throw new Error('reported-shape base is too large');
lines[lines.length - 1] = `"tail":"${'x'.repeat(filler)}"}`;
const reported = lines.join('\n');
if (Buffer.byteLength(reported) !== 106002 || reported.split('\n').length - 1 !== 3578) throw new Error('JSON shape metrics changed');
add('win10-19045-user-reported-shape', reported, true);

// Synthetic DevTools-shaped text, not a byte-exact reconstruction of the clipboard.
// 30 three-byte code points explain the reported UTF-8/code-point difference of 60.
const stack = ["Debug.js:216 Uncaught TypeError: Cannot read properties of undefined (reading 'value') " + '测'.repeat(15)];
for (const method of ['Widget.update', 'List.refresh', 'Widget.list', 'Widget.render', 'Widget.tick', 'Component.update', 'invoke', 'Class._invoke', 'Class.invoke', 'Class.update']) {
  stack.push(`    at ${method} (eval at <anonymous> (compile.js:238:32), <anonymous>:219:43)`);
}
stack.push('');
for (let index = 0; index < 5; index += 1) stack.push(`(匿名)\t@\tassets\\script\\c…nt\\Item.ts:${128 + index}`);
stack.push('eval\t@\tVM980:3');
const padding = 1329 - Buffer.byteLength(stack.join('\n'));
if (padding < 0) throw new Error('DevTools shape exceeds target size');
stack[0] += 'x'.repeat(padding);
const consoleText = stack.join('\n');
if (Buffer.byteLength(consoleText) !== 1329 || [...consoleText].length !== 1269 || consoleText.split('\n').length - 1 !== 17) throw new Error('DevTools metrics changed');
add('devtools-text-shape-1341', consoleText.replace(/\n/g, '\r\n'));
// Remove the blank line and 14 ASCII padding bytes: frame=1326, chars=1266, LF=16.
if (padding < 14) throw new Error('Insufficient padding for the 1326-byte shape');
const shortStack = [...stack];
shortStack[0] = shortStack[0].slice(0, -14);
shortStack.splice(11, 1);
add('devtools-text-shape-1326', shortStack.join('\r\n'));
add('devtools-text-prefix-shift-17', '0123456789ABCDEFG' + consoleText);
for (const boundary of [255, 256, 257, 4095, 4096, 4097]) {
  add(`devtools-text-boundary-${boundary}`, 'x'.repeat(boundary - 6) + '__BOUNDARY__\n' + consoleText);
}
if (!cases.some(c => !c.isJson) || !cases.some(c => c.isJson)) throw new Error('Both text and JSON acceptance are required');
const payloadFile = path.join(root, '.ci-claude/payloads.json');
fs.writeFileSync(payloadFile, JSON.stringify(cases));

const packageDir = path.join(root, '.ci-claude/node_modules/@anthropic-ai/claude-code');
const pkg = JSON.parse(fs.readFileSync(path.join(packageDir, 'package.json'), 'utf8'));
const bin = typeof pkg.bin === 'string' ? pkg.bin : pkg.bin.claude;
const cli = process.env.CC_PASTE_NATIVE_CLI || path.resolve(packageDir, bin);
if (!fs.existsSync(cli)) throw new Error(`Claude CLI entry missing: ${cli}`);
fs.appendFileSync(process.env.GITHUB_ENV, `CC_E2E_CLAUDE_PATH=${cli}\nCC_PASTE_PAYLOAD_FILE=${payloadFile}\nCC_TESTED_CLAUDE_VERSION=${pkg.version}\n`);
console.log('Installed package:', pkg.version, 'entry:', cli);
for (const c of cases) console.log(c.name, 'body_bytes=' + Buffer.byteLength(c.expected), 'lf=' + (c.expected.split('\n').length - 1), 'isJson=' + c.isJson);
if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY,
  '## Paste acceptance inputs\n\nPackage: `' + pkg.version + '`\n\n| Case | Body bytes | JSON |\n|---|---:|---|\n' +
  cases.map(c => `| ${c.name} | ${Buffer.byteLength(c.expected)} | ${c.isJson} |`).join('\n') +
  '\n\nShapes are synthetic. This job does not reproduce Windows 10 build 19045 or read any user clipboard.\n');
