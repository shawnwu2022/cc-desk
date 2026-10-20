// 先清理按次传输，避免官方包装或用户后续子进程继承 Desk 预加载。
const original = process.env.CC_DESK_CODEX_NODE_OPTIONS;
const raw = process.env.CC_DESK_CODEX_ARGV;
if (original === 'empty') process.env.NODE_OPTIONS = '';
else delete process.env.NODE_OPTIONS;
delete process.env.CC_DESK_CODEX_ARGV;
delete process.env.CC_DESK_CODEX_NODE_OPTIONS;

let applied = false;
try {
  const data = JSON.parse(raw);
  const prefixes = ['hooks.SessionStart=', 'hooks.UserPromptSubmit=', 'hooks.Stop=', 'hooks.state='];
  if (typeof data.entry === 'string' && Array.isArray(data.args) && data.args.length === 8 &&
      data.args.every((value, index) => typeof value === 'string' && !value.includes('\0') &&
        (index % 2 === 0 ? value === '-c' : value.startsWith(prefixes[(index - 1) / 2])))) {
    const fs = require('node:fs');
    const canonical = value => fs.realpathSync.native(value).replace(/^\\\\\?\\UNC\\/, '\\\\').replace(/^\\\\\?\\/, '').toLowerCase();
    if (canonical(process.argv[1]) === canonical(data.entry)) {
      process.argv.splice(2, 0, ...data.args);
      applied = true;
    }
  }
} catch {
  // 可选观察失败只保留原 argv，不探测、重启或替换启动目标。
}
if (!applied) {
  for (const name of ['PORT', 'RUN', 'GENERATION', 'CAPABILITY']) delete process.env['CC_DESK_CODEX_' + name];
}
