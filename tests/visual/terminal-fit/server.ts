import { createServer } from 'vite'

// 同进程持有服务，Windows 沙箱无需枚举/终止子进程树也能可靠清理。
export default async function setup() {
  const server = await createServer({ configFile: 'tests/visual/terminal-fit/vite.config.ts', mode: 'terminal-fit' })
  await server.listen()
  return () => server.close()
}
