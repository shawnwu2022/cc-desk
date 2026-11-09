// Explicit read-only --version probes; no inherited credentials or user CLI roots.
import fs from 'node:fs'
import path from 'node:path'
import { createHash } from 'node:crypto'
import { spawn, spawnSync } from 'node:child_process'
const root = fs.mkdtempSync(path.resolve('.audit/v027-stage3-isolated-cli-'))
const programs = [
  ['claude', 'C:\\Users\\wusha\\.local\\bin\\claude.exe'],
  ['codex', 'D:\\nodejs\\node_global\\node_modules\\@openai\\codex\\node_modules\\@openai\\codex-win32-x64\\vendor\\x86_64-pc-windows-msvc\\bin\\codex.exe'],
  ['opencode', 'D:\\nodejs\\node_global\\node_modules\\@opencode\\cli\\bin\\opencode.exe'],
]
const digest = filename => createHash('sha256').update(fs.readFileSync(filename)).digest('hex')
const results = []
for (const [cli, program] of programs) {
  const profileRoot = path.join(root, cli)
  const directory = name => { const target = path.join(profileRoot, name); fs.mkdirSync(target, { recursive: true }); return target }
  const home = directory('home'), cwd = directory('project'), temp = directory('temp')
  const env = { SystemRoot: process.env.SystemRoot, WINDIR: process.env.SystemRoot,
    PATH: `${process.env.SystemRoot}\\System32;${process.env.SystemRoot}`, COMSPEC: `${process.env.SystemRoot}\\System32\\cmd.exe`, PATHEXT: '.EXE;.CMD',
    HOME: home, USERPROFILE: home, HOMEDRIVE: path.parse(home).root.slice(0, 2), HOMEPATH: home.slice(2),
    APPDATA: directory('roaming'), LOCALAPPDATA: directory('local'), TEMP: temp, TMP: temp,
    XDG_CONFIG_HOME: directory('xdg-config'), XDG_DATA_HOME: directory('xdg-data'), XDG_STATE_HOME: directory('xdg-state'), XDG_CACHE_HOME: directory('xdg-cache'),
    CODEX_HOME: directory('codex-home'), CLAUDE_CONFIG_DIR: directory('claude-config'), OPENCODE_CONFIG_DIR: directory('opencode-config'),
    DISABLE_AUTOUPDATER: '1', DISABLE_UPDATES: '1', DISABLE_TELEMETRY: '1', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1', OPENCODE_DISABLE_AUTOUPDATE: '1',
    HTTP_PROXY: 'http://127.0.0.1:9', HTTPS_PROXY: 'http://127.0.0.1:9', ALL_PROXY: 'http://127.0.0.1:9', NO_PROXY: '127.0.0.1,localhost' }
  const before = digest(program)
  const result = await new Promise(resolve => {
    const child = spawn(program, ['--version'], { cwd, env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] })
    let bytes = 0, chunks = [], stderr = false, failure = null
    const stop = reason => {
      failure ??= reason
      if (child.pid) spawnSync('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, timeout: 5000, stdio: 'ignore' })
      child.kill('SIGKILL')
    }
    const timer = setTimeout(() => stop('timeout'), 5000)
    for (const [stream, errorStream] of [[child.stdout, false], [child.stderr, true]]) stream.on('data', chunk => {
      bytes += chunk.length; stderr ||= errorStream
      if (bytes > 8192) { chunks = []; stop('output-limit') } else if (!failure) chunks.push(chunk)
    })
    child.on('error', () => { clearTimeout(timer); resolve({ state: 'unknown', version: null, reason: 'command-failed' }) })
    child.on('close', code => {
      clearTimeout(timer)
      const line = Buffer.concat(chunks).toString('utf8').replace(/\r?\n$/, '')
      const version = cli === 'claude' ? /^(\d+\.\d+\.\d+) \(Claude Code\)$/.exec(line)?.[1]
        : cli === 'codex' ? /^codex-cli (\d+\.\d+\.\d+)$/.exec(line)?.[1]
        : /^(?:opencode v?)?(\d+\.\d+\.\d+)$/.exec(line)?.[1]
      resolve(!failure && code === 0 && !stderr && version ? { state: 'known', version, reason: null }
        : { state: 'unknown', version: null, reason: failure ?? (code === 0 ? 'invalid-output' : 'command-failed') })
    })
  })
  results.push({ cli, profile: `isolated-${cli}`, argv: ['--version'], ...result, executableUnchanged: before === digest(program) })
}
const report = { localOnly: true, isolatedDataRelativePath: path.relative(process.cwd(), root), inheritedCredentials: false,
  sessionCommands: false, productionBackendInvoked: false, results }
fs.writeFileSync('.audit/v027-stage3-cli-versions.json', JSON.stringify(report, null, 2) + '\n')
console.log(JSON.stringify(report))
if (results.some(result => result.state !== 'known' || !result.executableUnchanged)) process.exitCode = 1
