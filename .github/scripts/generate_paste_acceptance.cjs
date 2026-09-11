const fs = require('node:fs');
const path = require('node:path');

const root = process.cwd();
const ts = require(path.join(root, 'node_modules/typescript'));
const sourceTs = fs.readFileSync(path.join(root, 'src/utils/pasteText.ts'), 'utf8');
const compiled = ts.transpileModule(sourceTs, {
  compilerOptions: {
    module: ts.ModuleKind.CommonJS,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;

fs.mkdirSync(path.join(root, '.ci-claude'), { recursive: true });
const compiledPath = path.join(root, '.ci-claude/pasteText.cjs');
fs.writeFileSync(compiledPath, compiled);
const { buildPastePayload } = require(compiledPath);

const fixtures = JSON.parse(
  fs.readFileSync(path.join(root, 'src-tauri/tests/fixtures/devtools-paste-framing.json'), 'utf8'),
);
const cases = fixtures.filter(fixture => fixture.isJson).map(fixture => {
  const source = (fixture.prefix || '')
    + Array(fixture.repeat).fill(fixture.source).join(fixture.separator || '')
    + (fixture.suffix || '');
  JSON.parse(source);
  return {
    name: fixture.name,
    wire: buildPastePayload(source, true, false),
    expected: source.replace(/\r\n?/g, '\n'),
  };
});

// Privacy-safe shape matching the first field report exactly after newline normalization.
const lines = ['{', '"items":['];
for (let index = 0; index < 3574; index += 1) lines.push('0,');
lines.push('0', '],', '"tail":""}');
const base = lines.join('\n');
const filler = 106002 - Buffer.byteLength(base);
if (filler < 0) throw new Error('reported-shape base is too large');
lines[lines.length - 1] = `"tail":"${'x'.repeat(filler)}"}`;
const reported = lines.join('\n');
if (Buffer.byteLength(reported) !== 106002) throw new Error('reported-shape byte count mismatch');
if ((reported.match(/\n/g) || []).length !== 3578) throw new Error('reported-shape line count mismatch');
JSON.parse(reported);
cases.push({
  name: 'win10-19045-user-reported-shape',
  wire: buildPastePayload(reported, true, false),
  expected: reported,
});

if (cases.length === 0) throw new Error('No real Claude acceptance cases');
const payloadFile = path.resolve(root, '.ci-claude/payloads.json');
fs.writeFileSync(payloadFile, JSON.stringify(cases));

const packageDir = path.resolve(root, '.ci-claude/node_modules/@anthropic-ai/claude-code');
const packageJson = JSON.parse(fs.readFileSync(path.join(packageDir, 'package.json'), 'utf8'));
const bin = typeof packageJson.bin === 'string' ? packageJson.bin : packageJson.bin.claude;
const cli = path.resolve(packageDir, bin);
if (!fs.existsSync(cli)) throw new Error(`Claude CLI entry missing: ${cli}`);

fs.appendFileSync(
  process.env.GITHUB_ENV,
  `CC_E2E_CLAUDE_PATH=${cli}\nCC_PASTE_PAYLOAD_FILE=${payloadFile}\nCC_TESTED_CLAUDE_VERSION=${packageJson.version}\n`,
);
console.log('Claude version:', packageJson.version);
for (const testCase of cases) {
  console.log(testCase.name, Buffer.byteLength(testCase.expected));
}
