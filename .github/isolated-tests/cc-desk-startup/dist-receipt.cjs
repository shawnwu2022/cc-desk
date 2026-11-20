'use strict';
const fs = require('node:fs');
const path = require('node:path');
const {createHash} = require('node:crypto');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const root = path.resolve(process.argv[2] || '');
const source = process.argv[3] || '';
const out = {stage:'dist-html', distIndexSha256:null, distModuleGraphSha256:null, distInventorySha256:null,
  distReferencesResolved:false, frontendIdentityMatches:false, moduleScripts:0, localReferences:0};
function check(ok) { if (!ok) throw new Error('REJECTED'); }
function safeDir(dir) {
  const st = fs.lstatSync(dir);
  check(st.isDirectory() && !st.isSymbolicLink());
}
function read(file, max) {
  const st = fs.lstatSync(file);
  check(st.isFile() && !st.isSymbolicLink() && st.size > 0 && st.size <= max);
  const data = fs.readFileSync(file);
  check(data.length === st.size);
  return data;
}
function attr(attrs, name) {
  const values = [...attrs.matchAll(new RegExp('(?:^|\\s)' + name + '\\s*=\\s*(?:"([^"]*)"|\'([^\']*)\')', 'gi'))];
  const assignments = [...attrs.matchAll(new RegExp('(?:^|\\s)' + name + '(?=\\s*=|\\s|$)', 'gi'))];
  check(values.length <= 1 && assignments.length === values.length);
  return values.length ? values[0][1] ?? values[0][2] : undefined;
}
function key(ref) {
  check(typeof ref === 'string' && /^(?:\/|\.\/)?assets\/[A-Za-z0-9][A-Za-z0-9._-]{0,180}$/.test(ref));
  return ref.replace(/^(?:\/|\.\/)/, '');
}
try {
  check(/^[0-9a-f]{40}$/.test(source));
  safeDir(root);
  const htmlBytes = read(path.join(root,'index.html'), 256 * 1024);
  const html = htmlBytes.toString('utf8');
  out.distIndexSha256 = hash(htmlBytes);
  out.stage = 'dist-local-references';
  safeDir(path.join(root,'assets'));
  const tags = [...html.matchAll(/<(script|link)\b([^<>]*)>/gi)];
  check(tags.length > 0 && tags.length <= 48);
  const modules = [];
  for (const match of tags) {
    const [,rawTag,attrs] = match;
    const tag = rawTag.toLowerCase();
    const file = key(attr(attrs, tag === 'script' ? 'src' : 'href'));
    const data = read(path.join(root,file), 20 * 1024 * 1024);
    out.localReferences++;
    if (tag === 'script') {
      check(attr(attrs,'type') === 'module' && file.endsWith('.js'));
      modules.push([file,data.length,hash(data)]);
    }
  }
  check(modules.length > 0 && new Set(modules.map(x => x[0])).size === modules.length);
  out.moduleScripts = modules.length;
  out.distModuleGraphSha256 = hash(JSON.stringify(modules));
  out.distReferencesResolved = true;
  out.stage = 'dist-inventory';
  const inventory = [];
  function visit(dir, depth) {
    check(depth <= 8);
    safeDir(dir);
    for (const name of fs.readdirSync(dir).sort()) {
      check(/^[A-Za-z0-9][A-Za-z0-9._-]{0,180}$/.test(name));
      const file = path.join(dir,name);
      const stat=fs.lstatSync(file);
      check(!stat.isSymbolicLink());
      if (stat.isDirectory()) visit(file, depth + 1);
      else {
        const data = read(file,20*1024*1024);
        inventory.push([path.relative(root,file).split(path.sep).join('/'),data.length,hash(data)]);
      }
      check(inventory.length <= 160);
    }
  }
  visit(root,0);
  check(inventory.length >= 2);
  out.distInventorySha256 = hash(JSON.stringify(inventory));
  out.stage = 'dist-vite-identity';
  const settings = inventory.map(row => row[0]).filter(file => /^assets\/SettingsView-[A-Za-z0-9_-]+\.js$/.test(file));
  check(settings.length === 1);
  out.frontendIdentityMatches = read(path.join(root,settings[0]),2*1024*1024).includes(Buffer.from(source));
  check(out.frontendIdentityMatches);
  out.stage = 'dist-complete';
} catch {
  process.exitCode = 1;
}
process.stdout.write(JSON.stringify(out));
