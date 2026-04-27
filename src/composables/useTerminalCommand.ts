type SendCommandFn = (text: string) => void

const sendCommandFn: { current: SendCommandFn | null } = { current: null }

/** 由 XTermTerminal 注册，提供发送文字+聚焦终端的能力 */
export function registerTerminalCommand(fn: SendCommandFn) {
  sendCommandFn.current = fn
}

/** 向活跃终端发送文字并聚焦 */
export function sendTerminalCommand(text: string) {
  sendCommandFn.current?.(text)
}
