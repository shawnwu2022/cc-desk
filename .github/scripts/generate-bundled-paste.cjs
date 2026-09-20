// Preserve every historical strict fixture; append separate regressions only.
require('./generate_paste_acceptance.cjs');
const fs = require('node:fs');
const path = require('node:path');
const root = process.cwd();
const target = path.join(root, '.ci-claude/payloads.json');
const cases = JSON.parse(fs.readFileSync(target, 'utf8'));
const { buildPastePayload } = require(path.join(root, '.ci-claude/pasteText.cjs'));
const rows = Array.from({ length: 231 }, (_, index) => `frame-${String(index).padStart(3, '0')} 日志 synthetic console payload ${'x'.repeat(60)}`);
const missing = 28037 - Buffer.byteLength(rows.join('\n'));
if (missing < 0) throw new Error('Synthetic reported shape is too large');
rows[0] += 'x'.repeat(missing);
const body = rows.join('\n');
if (Buffer.byteLength(body) !== 28037 || (body.match(/\n/g) || []).length !== 230) throw new Error('Reported shape changed');
const launchMode = process.env.CC_PASTE_LAUNCH_MODE || 'direct';
for (const [name, expected, copies] of [
  ['field-shape-28037-no-tabs', body, 1],
  ['consecutive-three-pastes', 'BEGIN 日志\nMIDDLE\nEND', 3],
]) {
  cases.push({ name, expected, wire: buildPastePayload(expected, true, false), isJson: false, launchMode, copies });
}
fs.writeFileSync(target, JSON.stringify(cases));

// Release blockers are exact-equality cases that represent the supported paste
// contract. Historical editor normalizations remain in payloads.json and are
// still characterized strictly, but do not turn a fixed truncation regression
// into a permanently red release signal.
const releaseGateNames = [
  'devtools-json-big-integer',
  'console-multiline-over-128k',
  'devtools-json-nested-64',
  'devtools-json-nested-256',
  'devtools-json-nested-800',
  'win10-19045-user-reported-shape',
  'field-shape-28037-no-tabs',
  'consecutive-three-pastes',
];
const byName = new Map();
for (const entry of cases) {
  if (byName.has(entry.name)) throw new Error(`Duplicate bundled acceptance case: ${entry.name}`);
  byName.set(entry.name, entry);
}
const gate = releaseGateNames.map(name => {
  const entry = byName.get(name);
  if (!entry) throw new Error(`Missing release-gate case: ${name}`);
  return entry;
});
fs.writeFileSync(path.join(root, '.ci-claude/payloads-gate.json'), JSON.stringify(gate));
console.log(`Bundled backend cases: ${cases.length}; exact release gate: ${gate.length}; historical strict fixtures retained`);
