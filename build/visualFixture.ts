import type { Plugin } from 'vite'
import { resolve } from 'node:path'

/** Test entry only. No normal App/main import, production alias or public fixture route. */
export function visualFixturePlugin(root: string, enabled: boolean): Plugin {
  return {
    name: 'cc-desk-visual-fixture', enforce: 'pre',
    resolveId(source) {
      if (enabled && source.startsWith('@tauri-apps/')) return resolve(root, 'src/visual/tauriStub.ts')
    },
    load(id) {
      if (!enabled && id.replace(/\\/g, '/').includes('/src/visual/')) throw new Error('VISUAL_FIXTURE_DISABLED')
    },
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        const path = new URL(request.url ?? '/', 'http://localhost').pathname
        const fixture = path.startsWith('/__visual__') || path.includes('/src/visual/')
        if ((!enabled && fixture) || enabled && ['/', '/index.html', '/src/main.ts', '/src/App.vue'].includes(path)) {
          response.statusCode = 404; response.end('Not found'); return
        }
        if (enabled && path === '/__visual__/') {
          response.setHeader('Content-Type', 'text/html')
          response.end('<!doctype html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1.0"><title>CC Desk visual fixture</title></head><body><div id="app"></div><script type="module" src="/src/visual/entry.ts"></script></body></html>')
          return
        }
        next()
      })
    },
  }
}
