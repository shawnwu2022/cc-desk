import type { Terminal } from '@xterm/xterm'

/**
 * 将 OSC 标题变化合并为只读失效信号；不将不可信终端文本用作会话名称。
 * @param terminal 当前解析器；调用方必须在切换运行或销毁解析器时释放订阅。
 * @param captureRefresh 收到通知时冻结当前 owner，返回仅作用于该 owner 的补读通知。
 * @returns 同时取消订阅和未提交通知的释放函数。
 */
export function observeTerminalTitleChanges(terminal: Pick<Terminal, 'onTitleChange'>, captureRefresh: () => (() => void) | undefined): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined
  let latest = '', published = ''
  let lastPublishedAt = Number.NEGATIVE_INFINITY
  const subscription = terminal.onTitleChange(value => {
    // OSC 是不可信输出；只保留有界比较值，不存入 store、日志或显示 DTO。
    if (!value || value.length > 1024 || value === latest) return
    latest = value
    clearTimeout(timer)
    const refresh = captureRefresh()
    if (!refresh || value === published) return
    // 吸收 CLI 忙闲图标的短时切换；持续变化最多每两秒通知一次，不设轮询。
    timer = setTimeout(() => {
      timer = undefined
      published = value
      lastPublishedAt = Date.now()
      refresh()
    }, Math.max(500, 2000 - (Date.now() - lastPublishedAt)))
  })
  return () => { clearTimeout(timer); subscription.dispose() }
}
