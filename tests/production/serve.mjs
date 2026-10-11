import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { build, preview } from 'vite'

const root = fileURLToPath(new URL('../../', import.meta.url))
const policy = JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json', import.meta.url), 'utf8')).app.security.csp
if (typeof policy !== 'string' || !policy.trim()) throw new Error('Production startup requires the actual nonempty production CSP')

// 编译正常生产入口，并在浏览器中执行真实 CSP；不得替换为视觉 fixture 或开发模块。
await build({ root })
const server = await preview({
  root,
  preview: { host: '127.0.0.1', port: 4177, strictPort: true },
  plugins: [{
    name: 'production-startup-csp',
    configurePreviewServer(server) {
      server.middlewares.use((_request, response, next) => {
        response.setHeader('Content-Security-Policy', policy)
        next()
      })
    },
  }],
})
server.printUrls()
