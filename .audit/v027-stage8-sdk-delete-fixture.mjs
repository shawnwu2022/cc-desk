// 仅验证任务自行创建的合成文件；不启动 CLI、查询模型或访问用户配置。
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const audit = path.join(repository, '.audit')
const sdkPath = path.join(repository, 'node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs')
const sdkBytes = fs.readFileSync(sdkPath)
assert.equal(createHash('sha256').update(sdkBytes).digest('hex'), '6b963b3eb871949a14392bb35ff970a55e91e2edbe218c61cd9eed60e46cbe45')
assert.equal(fs.realpathSync(audit), path.join(fs.realpathSync(repository), '.audit'))
const fixture = fs.mkdtempSync(path.join(audit, 'v027-stage8-sdk-delete-'))
assert.equal(path.dirname(fs.realpathSync(fixture)), fs.realpathSync(audit))
const configuration = path.join(fixture, 'config')
const projects = path.join(configuration, 'projects', 'authorized')
fs.mkdirSync(projects, { recursive: true })
const id = '10000000-0000-4000-8000-000000000001'
const aliasId = '10000000-0000-4000-8000-000000000002'
const writerId = '10000000-0000-4000-8000-000000000003'
const transcript = sessionId => JSON.stringify({ type: 'system', sessionId, cwd: fixture }) + '\n'
fs.writeFileSync(path.join(projects, `${id}.jsonl`), transcript(id))
fs.mkdirSync(path.join(projects, id, 'subagents'), { recursive: true })
fs.writeFileSync(path.join(projects, id, 'subagents', 'agent-synthetic.jsonl'), transcript(id))
fs.writeFileSync(path.join(fixture, 'project-file.keep'), 'synthetic project marker')
const originals = path.join(fixture, 'originals')
fs.mkdirSync(originals)
fs.writeFileSync(path.join(originals, `${aliasId}.jsonl`), transcript(aliasId))
fs.mkdirSync(path.join(originals, aliasId))
fs.writeFileSync(path.join(originals, aliasId, 'synthetic.keep'), 'synthetic child marker')
fs.linkSync(path.join(originals, `${aliasId}.jsonl`), path.join(projects, `${aliasId}.jsonl`))
fs.writeFileSync(path.join(projects, `${writerId}.jsonl`), transcript(writerId))
const child = `
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
const {deleteSession} = await import(${JSON.stringify(pathToFileURL(sdkPath).href)});
const root = process.env.CLAUDE_CONFIG_DIR;
assert(root && root === ${JSON.stringify(configuration)});
const projects = path.join(root,'projects','authorized');
await deleteSession(${JSON.stringify(id)});
assert(!fs.existsSync(path.join(projects,${JSON.stringify(id)}+'.jsonl')));
assert(!fs.existsSync(path.join(projects,${JSON.stringify(id)})));
assert(fs.existsSync(${JSON.stringify(path.join(fixture, 'project-file.keep'))}));
console.log(JSON.stringify({case:'native-disk-delete',transcriptRemoved:true,subagentDirectoryRemoved:true,syntheticProjectMarkerRetained:true}));
await deleteSession(${JSON.stringify(aliasId)});
assert(!fs.existsSync(path.join(projects,${JSON.stringify(aliasId)}+'.jsonl')));
assert(fs.existsSync(${JSON.stringify(path.join(originals, `${aliasId}.jsonl`))}));
assert(fs.existsSync(${JSON.stringify(path.join(originals, aliasId, 'synthetic.keep'))}));
console.log(JSON.stringify({case:'hardlink-adapter-counterexample',privateAliasRemoved:true,originalTranscriptRetained:true,originalChildRetained:true}));
const target = path.join(projects,${JSON.stringify(writerId)}+'.jsonl');
const fd = fs.openSync(target,'a');
try {
  await deleteSession(${JSON.stringify(writerId)});
  const removed = !fs.existsSync(target);
  fs.writeSync(fd,'synthetic open writer after native deletion\\n');
  console.log(JSON.stringify({case:'open-writer-counterexample',transcriptRemoved:removed,writeHandleStillUsable:true}));
  assert(removed);
} finally {fs.closeSync(fd);}
`
const env = Object.fromEntries(['SystemRoot', 'WINDIR', 'ComSpec', 'PATH', 'PATHEXT'].flatMap(key => process.env[key] ? [[key, process.env[key]]] : []))
for (const key of ['HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'TEMP', 'TMP']) {
  env[key] = path.join(fixture, key.toLowerCase())
  fs.mkdirSync(env[key], { recursive: true })
}
env.CLAUDE_CONFIG_DIR = configuration
const result = spawnSync(process.execPath, ['--input-type=module', '-e', child], { cwd: fixture, env, timeout: 20_000, maxBuffer: 65_536, windowsHide: true, encoding: 'utf8' })
if (result.status !== 0) {
  console.log(JSON.stringify({fixture: path.basename(fixture), state: 'failed', status: result.status, errorCode: result.error?.code ?? null}))
  process.stderr.write(result.stderr || '')
  process.stdout.write(result.stdout || '')
  process.exitCode = 1
} else {
  process.stdout.write(result.stdout)
  console.log(JSON.stringify({fixture: path.basename(fixture), state: 'passed', sdk: '0.3.301', platform: process.platform, retainedForInspection: true}))
}
