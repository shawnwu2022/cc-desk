// 先清理按次传输，避免官方包装或用户后续子进程继承 Desk 预加载。
const original = process.env.CC_DESK_CODEX_NODE_OPTIONS;
const raw = process.env.CC_DESK_CODEX_ARGV;
if (original === 'empty') process.env.NODE_OPTIONS = '';
else delete process.env.NODE_OPTIONS;
delete process.env.CC_DESK_CODEX_ARGV;
delete process.env.CC_DESK_CODEX_NODE_OPTIONS;

let applied = false;
function exactFileEntry(data) {
  try {
    if (process.execArgv.length !== 0 || !data || typeof data.entry !== 'string' || !data.fileId) return false;
    if (data.entry !== target.entry || data.fileId.volume !== target.fileId.volume || data.fileId.index !== target.fileId.index) return false;
    const fs = require('node:fs');
    const canonical = value => fs.realpathSync.native(value).replace(/^\\\\\?\\UNC\\/, '\\\\').replace(/^\\\\\?\\/, '').toLowerCase();
    const selected = fs.statSync(data.entry, { bigint: true });
    return selected.isFile() && selected.dev.toString() === data.fileId.volume &&
      selected.ino.toString() === data.fileId.index && canonical(process.argv[1]) === canonical(data.entry);
  } catch { return false; }
}
try {
  const data = JSON.parse(raw);
  const prefixes = ['hooks.SessionStart=', 'hooks.UserPromptSubmit=', 'hooks.Stop=', 'hooks.state='];
  const validArgs = Array.isArray(data.args) && data.args.every(value => typeof value === 'string' && value.length <= 65536 && !value.includes('\0')) &&
      ((data.args.length === 2 && data.args[0] === '-c' && data.args[1].startsWith('hooks=')) ||
       (data.args.length === 8 && data.args.every((value, index) => index % 2 === 0 ? value === '-c' : value.startsWith(prefixes[(index - 1) / 2]))));
  const generation = Number(data.generation);
  const port = Number(process.env.CC_DESK_CODEX_PORT);
  const bound = typeof data.run === 'string' && data.run.length > 0 && Buffer.byteLength(data.run) <= 128 &&
      !/[\u0000-\u001f\u007f-\u009f]/u.test(data.run) &&
      typeof data.generation === 'string' && String(generation) === data.generation &&
      Number.isInteger(generation) && generation > 0 && generation <= 0xffffffff &&
      Number.isInteger(port) && port > 0 && port <= 65535 &&
      /^[A-Za-z0-9_-]{32,128}$/.test(process.env.CC_DESK_CODEX_CAPABILITY ?? '');
  if (bound && process.execArgv.length === 0 && typeof data.entry === 'string' &&
      data.run === target.run && data.generation === target.generation &&
      (process.env.CC_DESK_CODEX_RUN ?? null) === data.run &&
      (process.env.CC_DESK_CODEX_GENERATION ?? null) === data.generation &&
      data.fileId && typeof data.fileId.volume === 'string' && typeof data.fileId.index === 'string' &&
      validArgs) {
    if (exactFileEntry(data)) {
      process.argv.splice(2, 0, ...data.args);
      applied = true;
    }
  }
} catch {
  // 可选观察失败只保留原 argv，不探测、重启或替换启动目标。
}
if (!applied) {
  for (const name of ['PORT', 'RUN', 'GENERATION', 'CAPABILITY']) delete process.env['CC_DESK_CODEX_' + name];
  if (rollback) {
    // This data is baked by the backend into the held private loader, never
    // recovered from an untrusted/malformed environment envelope.
    if (!exactFileEntry(rollback)) {
      process.stderr.write('CC Desk: original CLI arguments could not be safely restored.\n');
      process.exit(1);
    }
    try { process.argv.splice(2, process.argv.length - 2, ...rollback.args); }
    catch { process.exit(1); }
  }
}
