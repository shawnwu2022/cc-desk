import type { Terminal } from '@xterm/xterm'

export type CodexProgramStatus = 'idle' | 'working' | 'blocked' | 'unknown'

/** 原生 TUI 的有序视觉提示；不是认证 hook、回合完成或权限凭据。 */
export function observeCodexProgramStatus(terminal: Pick<Terminal, 'parser'>, publish: (status: CodexProgramStatus) => void): () => void {
  let disposed = false
  const subscription = terminal.parser.registerOscHandler(7501, payload => {
    if (disposed) return false
    const states: Record<string, CodexProgramStatus> = {
      'state=idle:app=codex': 'idle',
      'state=working:app=codex': 'working',
      'state=blocked:app=codex': 'blocked',
      'state=clear': 'unknown',
    }
    // 只认精确协议值，不保留终端正文或其它应用的数据。
    const status = Object.prototype.hasOwnProperty.call(states, payload) ? states[payload] : undefined
    if (status) publish(status)
    else if (/(?:^|:)app=codex(?::|$)/.test(payload)) publish('unknown')
    return false
  })
  return () => { disposed = true; subscription.dispose() }
}
