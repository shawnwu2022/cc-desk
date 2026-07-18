/**
 * PTY → tab 反查索引：PTY 输出/退出事件按 ptyId O(1) 定位 tab，
 * 替代遍历 terminalInstances 的 O(n) 线性扫描（多并行会话高频输出时随 N 线性放大）。
 *
 * 与 terminalInstances 同生命周期：spawn 成功赋 ptyId 时 link，实例销毁时 unlink。
 * 空 ptyId 不入索引（spawn 失败/未赋值），避免无效映射。
 *
 * 抽取为独立类便于单元测试（XTermTerminal.vue 组件级难直接测）。
 */
export class PtyIndex {
  private readonly map = new Map<string, string>()

  /** 注册 ptyId → tabId；空 ptyId 忽略（spawn 未成功赋值时不建立映射） */
  link(ptyId: string, tabId: string): void {
    if (ptyId) this.map.set(ptyId, tabId)
  }

  /** 注销 ptyId；空 ptyId 忽略（内部判空，调用方可省略外层 if） */
  unlink(ptyId: string): void {
    if (ptyId) this.map.delete(ptyId)
  }

  /** 查 ptyId 对应 tabId；未知返回 undefined */
  get(ptyId: string): string | undefined {
    return this.map.get(ptyId)
  }

  /** 清空（组件 cleanup） */
  clear(): void {
    this.map.clear()
  }

  /** 当前映射数（测试/调试用） */
  get size(): number {
    return this.map.size
  }
}
