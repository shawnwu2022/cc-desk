import { spawn } from 'node:child_process'
import { mkdirSync, writeFileSync, createWriteStream, readFileSync, mkdtempSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { resolve } from 'node:path'
import { chromium } from '@playwright/test'

// 只在失败的隔离 fixture 上取证，不比较或改写已批准的基线，也不参与准入。
const base = resolve(process.argv[2])
const output = resolve('visual-evidence/control-diagnostics')
mkdirSync(output, { recursive: true })
const children = []
const services = new Map()
let browser
const pause = ms => new Promise(done => setTimeout(done, ms))
async function start(root, port, label, bin = resolve(root, 'node_modules/vite/bin/vite.js'), config) {
  const log = createWriteStream(resolve(output, `${label}-server.log`))
  let started = false
  let stdout = ''
  let failure
  const child = spawn(process.execPath, [bin, ...(config ? ['--config', config] : []), '--mode', 'visual', '--host', '127.0.0.1', '--port', String(port), '--strictPort'], {
    cwd: root, env: { ...process.env, CC_DESK_VISUAL_FIXTURE: '1' }, stdio: ['ignore', 'pipe', 'pipe'],
  })
  children.push(child)
  child.on('error', error => { failure = error })
  child.stdout.on('data', bytes => {
    log.write(bytes)
    stdout += bytes.toString().replace(/\x1b\[[0-9;]*m/g, '')
    started = stdout.includes(`http://127.0.0.1:${port}/`)
  })
  child.stderr.pipe(log, { end: false })
  child.on('close', () => log.end())
  for (let attempt = 0; attempt < 150; attempt++) {
    if (failure) throw failure
    if (child.exitCode !== null) throw new Error(`${label}: Vite exited ${child.exitCode}`)
    if (started) {
      try {
        if ((await fetch(`http://127.0.0.1:${port}/__visual__/`)).ok && child.exitCode === null) {
          services.set(label, { pid: child.pid, root, port, bin, config,
            lockSha256: createHash('sha256').update(readFileSync(resolve(root, 'package-lock.json'))).digest('hex') })
          return
        }
      } catch { /* 等待本任务创建的服务 */ }
    }
    await pause(100)
  }
  throw new Error(`${label}: Vite startup timeout`)
}
async function capture(port, label) {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1.5,
    locale: 'en-US', timezoneId: 'UTC', reducedMotion: 'reduce',
    userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36',
  })
  try {
    const page = await context.newPage()
    const origin = `http://127.0.0.1:${port}`
    const errors = []
    page.on('pageerror', error => errors.push(error.message))
    await page.route('**/*', route => new URL(route.request().url()).origin === origin ? route.continue() : route.abort())
    await page.clock.setSystemTime(new Date('2026-09-28T12:00:00Z'))
    await page.goto(`${origin}/__visual__/?scenario=projects&locale=zh&density=compact`)
    await page.locator('[data-visual-ready="true"]').waitFor()
    await page.evaluate(() => document.fonts.ready)
    const evidence = await page.evaluate(() => ({
      vueVersion: document.querySelector('#app')?.__vue_app__?.version,
      blockedHostCalls: document.querySelector('[data-blocked-host-calls]')?.getAttribute('data-blocked-host-calls'),
      controls: ['[data-project-search]', '[data-project-sort]'].map(selector => {
        const element = document.querySelector(selector)
        const styles = getComputedStyle(element)
        return { selector, html: element.outerHTML, rectangle: element.getBoundingClientRect().toJSON(),
          styles: Object.fromEntries([...styles].map(key => [key, styles.getPropertyValue(key)])) }
      }),
    }))
    if (errors.length || evidence.blockedHostCalls !== '0') throw new Error(`${label}: fixture isolation failed`)
    await page.screenshot({ path: resolve(output, `${label}.png`), animations: 'disabled', caret: 'hide', scale: 'device' })
    const cdp = await context.newCDPSession(page)
    await cdp.send('DOM.enable')
    const dom = await cdp.send('DOM.getDocument', { depth: -1, pierce: true })
    await cdp.send('CSS.enable')
    const platformFonts = []
    for (const selector of ['[data-project-search]', '[data-project-sort]']) {
      const { nodeId } = await cdp.send('DOM.querySelector', { nodeId: dom.root.nodeId, selector })
      platformFonts.push({ selector, ...await cdp.send('CSS.getPlatformFontsForNode', { nodeId }) })
    }
    writeFileSync(resolve(output, `${label}.json`), JSON.stringify({ label, service: services.get(label), browser: browser.version(), ...evidence, dom, platformFonts }, null, 2))
    console.log(`${label}: isolated controls captured`)
  } finally { await context.close() }
}
try {
  await start(base, 4319, 'stable024')
  await start(process.cwd(), 4318, 'candidate025')
  browser = await chromium.launch()
  await capture(4319, 'stable024')
  await capture(4318, 'candidate025')
  // 同一源码仅交叉一个依赖族，避免把当前控件漂移误归为宿主或布局变化。
  mkdirSync(resolve('.cache'), { recursive: true })
  const temporary = mkdtempSync(resolve('.cache/visual-control-diagnostic-'))
  const root = process.cwd().replaceAll(String.fromCharCode(92), '/')
  const old = base.replaceAll(String.fromCharCode(92), '/')
  const vite5Config = resolve(temporary, 'old-vite.config.mjs')
  writeFileSync(vite5Config, `import original from ${JSON.stringify(`${root}/vite.config.ts`)}
import oldVuePlugin from ${JSON.stringify(`${old}/node_modules/@vitejs/plugin-vue/dist/index.mjs`)}
import { createRequire } from 'node:module'
const candidateRequire = createRequire(${JSON.stringify(`${root}/package.json`)})
export default env => {
  const result = original(env)
  return {...result, cacheDir: ${JSON.stringify(resolve(temporary, 'vite5-cache'))},
    plugins: result.plugins.map(plugin => plugin.name === 'vite:vue' ? oldVuePlugin({compiler: candidateRequire('@vue/compiler-sfc')}) : plugin)}
}
`)
  await start(process.cwd(), 4320, 'candidate-vite5', resolve(base, 'node_modules/vite/bin/vite.js'), vite5Config)
  await capture(4320, 'candidate-vite5')
  const config = resolve(temporary, 'old-vue.config.mjs')
  writeFileSync(config, `import original from ${JSON.stringify(`${root}/vite.config.ts`)}
import vue from '@vitejs/plugin-vue'
import { createRequire } from 'node:module'
const oldRequire = createRequire(${JSON.stringify(`${old}/package.json`)})
export default env => {
  const result = original(env)
  return {...result, cacheDir: ${JSON.stringify(resolve(temporary, 'cache'))},
    plugins: result.plugins.map(plugin => plugin.name === 'vite:vue' ? vue({compiler: oldRequire('@vue/compiler-sfc')}) : plugin),
    resolve: {...result.resolve, alias: {...result.resolve.alias, vue: ${JSON.stringify(`${old}/node_modules/vue/dist/vue.runtime.esm-bundler.js`)}}}}
}
`)
  await start(process.cwd(), 4321, 'candidate-vue33', undefined, config)
  await capture(4321, 'candidate-vue33')
  const earlyConfig = resolve(temporary, 'early-styles.config.mjs')
  writeFileSync(earlyConfig, `import original from ${JSON.stringify(`${root}/vite.config.ts`)}
export default env => {
  const result = original(env)
  return {...result, cacheDir: ${JSON.stringify(resolve(temporary, 'early-cache'))}, plugins: [{
    name: 'diagnostic-early-fixture-styles', enforce: 'pre',
    transform(code, id) {
      if (!id.replaceAll(String.fromCharCode(92), '/').endsWith('/src/visual/entry.ts')) return
      const global = "import '@/styles/global.css'"
      const fixture = "import './fixture.css'"
      if (!code.includes(global) || !code.includes(fixture)) throw new Error('DIAGNOSTIC_ENTRY_MISMATCH')
      return global + '\\n' + fixture + '\\n' + code.replace(global, '').replace(fixture, '')
    }
  }, ...result.plugins]}
}
`)
  await start(process.cwd(), 4322, 'candidate-early-styles', undefined, earlyConfig)
  await capture(4322, 'candidate-early-styles')
} finally {
  try { if (browser) await browser.close() } finally {
    // 只终止本脚本直接启动的 Vite 子进程。
    for (const child of children) child.kill()
  }
}
