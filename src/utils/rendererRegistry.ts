import { toRaw } from 'vue'

/**
 * Terminal 渲染生命周期注册表：per-terminal 单飞初始化、dispose 标记、reload timer 三状态。
 *
 * key 一律 toRaw 归一：terminalInstances 是深度 reactive Map，setTerminalEl 取出的
 * instance.term 是 Vue proxy，与创建路径的 raw terminal 身份不同；不归一会让 WeakMap
 * 建立两个 key——单飞失效（重复初始化）、timer 句柄被覆盖（dispose 只能清最后一个，
 * 首个 timer 永久持有 terminal）。
 *
 * timer 以 stop 函数保存（而非句柄本身）：调用方闭包持有具体句柄类型，
 * registry 不需要感知 Node Timeout / jsdom number 的环境差异。
 */
export class TerminalRendererRegistry {
  private readonly inFlight = new WeakMap<object, Promise<void>>()
  private readonly disposed = new WeakMap<object, boolean>()
  private readonly timers = new WeakMap<object, () => void>()

  private static key(term: object): object {
    return toRaw(term)
  }

  /** 单飞执行：同一 terminal（含 raw/proxy 两种身份）并发调用只执行一次，其余复用同一 Promise。 */
  runOnce(term: object, run: () => Promise<void>): Promise<void> {
    const key = TerminalRendererRegistry.key(term)
    const existing = this.inFlight.get(key)
    if (existing) return existing
    // 失败也终结并缓存（terminal 不重用），避免 unhandled rejection 且后续调用命中单飞。
    const promise = run().then(
      () => {},
      () => {},
    )
    this.inFlight.set(key, promise)
    return promise
  }

  /** 标记 terminal 已 dispose：后续初始化/reload 全部跳过。 */
  markDisposed(term: object): void {
    this.disposed.set(TerminalRendererRegistry.key(term), true)
  }

  /** terminal 是否已 dispose（raw/proxy 身份查询结果一致）。 */
  isDisposed(term: object): boolean {
    return this.disposed.get(TerminalRendererRegistry.key(term)) === true
  }

  /** 记录 per-terminal reload timer 的 stop 函数（同 key 覆盖前先执行旧 stop，防句柄泄漏）。 */
  setTimer(term: object, stop: () => void): void {
    const key = TerminalRendererRegistry.key(term)
    this.timers.get(key)?.()
    this.timers.set(key, stop)
  }

  /** 停止并移除 per-terminal reload timer（terminal 未注册过则无操作）。 */
  clearTimer(term: object): void {
    const key = TerminalRendererRegistry.key(term)
    this.timers.get(key)?.()
    this.timers.delete(key)
  }
}
