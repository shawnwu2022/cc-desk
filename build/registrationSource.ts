import {existsSync} from 'node:fs'
import {resolve} from 'node:path'
import type {Plugin} from 'vite'
// CLI 与产品构建复用同一份捕获输入与独立模块校验。
// @ts-expect-error JavaScript 构建工具没有类型声明文件。
import {bundleRegistrationSource} from '../scripts/build-browser-registration-source.mjs'
export function registrationSourcePlugin(root:string):Plugin {
 return {name:'registration-source',apply:'build',async generateBundle(){
  if(existsSync(resolve(root,'public/browser-registration.js')))throw Error('Custom registration source requires an explicit standalone build, not silent product replacement')
  const {bytes,manifest}=await bundleRegistrationSource('remote/ui-terminal/src/productBrowserRegistrationSource.ts')
  this.emitFile({type:'asset',fileName:'browser-registration.js',source:bytes})
  this.emitFile({type:'asset',fileName:'browser-registration.manifest.json',source:JSON.stringify(manifest,null,2)+'\n'})
 }}
}
