import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
const adapter = path.resolve('scripts/opencode-archive-sqlite.mjs')
const code = `import assert from 'node:assert/strict';import {DatabaseSync,backup} from ${JSON.stringify(pathToFileURL(adapter).href)};const db=new DatabaseSync(':memory:',{allowExtension:true});assert.throws(()=>db.enableLoadExtension(true),/SQLITE_EXTENSION_FORBIDDEN/);assert.throws(()=>db.loadExtension('untrusted'),/SQLITE_EXTENSION_FORBIDDEN/);assert.throws(()=>backup(db,'untrusted'),/SQLITE_BACKUP_FORBIDDEN/);db.enableLoadExtension(false);db.exec('CREATE TABLE marker(x)');assert.equal(db.prepare('SELECT count(*) n FROM marker').get().n,0);db.close();console.log('OpenCodeArchiveSqlite_ExtensionsAlwaysDenied_001 passed');`
const result = spawnSync(process.execPath, ['--permission', '--allow-fs-read=' + adapter, '--input-type=module', '-e', code], { encoding: 'utf8', windowsHide: true, timeout: 10000 })
assert.equal(result.status, 0, result.stderr); process.stdout.write(result.stdout)