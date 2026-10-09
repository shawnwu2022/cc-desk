type SendCommandFn = (text: string) => boolean | void

const sendCommandFn: { current: SendCommandFn | null } = { current: null }

/** 由 XTermTerminal 注册，提供发送文字+聚焦终端的能力 */
export function registerTerminalCommand(fn: SendCommandFn) {
  sendCommandFn.current = fn
  return () => { if (sendCommandFn.current === fn) sendCommandFn.current = null }
}

/** 向活跃终端发送文字并聚焦 */
export function sendTerminalCommand(text: string) {
  return sendCommandFn.current?.(text) ?? false
}
