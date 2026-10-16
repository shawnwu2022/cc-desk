import { build } from 'esbuild'
import { mkdir, readFile, writeFile, copyFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..')
const sdkRoot=resolve(root,'node_modules/@anthropic-ai/claude-agent-sdk')
const version=JSON.parse(await readFile(resolve(sdkRoot,'package.json'),'utf8')).version
if(version!=='0.3.301') throw new Error('UNEXPECTED_SESSION_SDK_VERSION')
const output=resolve(root,'src-tauri/resources/native-session-sdk')
await mkdir(output,{recursive:true})
await build({entryPoints:[resolve(root,'scripts/session-sdk-bridge.mjs')],outfile:resolve(output,'bridge.cjs'),bundle:true,platform:'node',target:'node22',format:'cjs',minify:true,legalComments:'eof',define:{'import.meta.url':'__ccDeskImportUrl'},banner:{js:'// Contains Anthropic Claude Agent SDK 0.3.301. See LICENSE.md and NOTICE.txt.\nvar __ccDeskImportUrl = require("node:url").pathToFileURL(__filename).href;'}})
await copyFile(resolve(sdkRoot,'LICENSE.md'),resolve(output,'LICENSE.md'))
await writeFile(resolve(output,'NOTICE.txt'),'Claude Agent SDK 0.3.301 by Anthropic PBC.\nOfficial source: https://www.npmjs.com/package/@anthropic-ai/claude-agent-sdk\nUse is subject to https://code.claude.com/docs/en/legal-and-compliance.\nOnly the local session metadata SDK API is called. No model query or bundled CLI runtime is used.\n')
console.log('Prepared pinned native session metadata SDK helper.')
