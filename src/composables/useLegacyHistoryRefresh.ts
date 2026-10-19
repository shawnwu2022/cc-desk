import { onScopeDispose, watch } from 'vue'
import { useHookStore } from '@/stores/hook'
import { useSessionStore, type TerminalTab } from '@/stores/session'
import { normalizePath } from '@/utils/path'

/**
 * 将旧 PTY 的认证交互合并为项目历史补读；只更新显示，不获得运行控制权。
 * @param options 工作区就绪状态、冻结根验证与项目发布回调。
 * @returns 无；作用域释放时撤销订阅、排队读取和迟到标题定时器。
 */
export function useLegacyHistoryRefresh(options: {
  ready(): boolean
  validateRoot(): Promise<string | undefined>
  publish(projectPath: string): void
}): void {
  const sessions = useSessionStore()
  type Owner = { tab: TerminalTab; identity: string; titleRevision: number; timer?: ReturnType<typeof setTimeout>; eventId?: string }
  const owners = new Map<string, Owner>(), pending = new Map<string, Owner>()
  let timer: ReturnType<typeof setTimeout> | undefined
  let disposed = false, reading = false
  const identity = (tab: TerminalTab) => JSON.stringify([tab.ptyId, tab.ptyGeneration, tab.sessionId, normalizePath(tab.projectPath)])
  const eligible = (tab: TerminalTab) => (tab.cli ?? 'claude') === 'claude' && tab.status === 'running'
    && !!tab.ptyId && !!tab.sessionId && tab.usesDefaultClaudeRoot === true && tab.observerEnabled !== false
  const current = (owner: Owner) => !disposed && sessions.tabs.get(owner.tab.tabId) === owner.tab
    && eligible(owner.tab) && identity(owner.tab) === owner.identity && owners.get(owner.tab.tabId) === owner
  function schedule() {
    if (disposed || reading || timer || !options.ready() || !pending.size) return
    timer = setTimeout(() => { timer = undefined; void read() }, 250)
  }
  async function read() {
    if (disposed || reading || !options.ready()) return
    reading = true
    const groups = new Map<string, Owner[]>()
    for (const owner of pending.values()) {
      const key = normalizePath(owner.tab.projectPath)
      const group = groups.get(key) ?? []
      group.push(owner); groups.set(key, group)
    }
    pending.clear()
    try {
      const batches = [...groups.values()]
      for (let index = 0; index < batches.length; index += 2) {
        await Promise.all(batches.slice(index, index + 2).map(async batch => {
          if (!options.ready() || !await options.validateRoot()) return
          const live = batch.filter(current)
          if (!live.length) return
          const projectPath = live[0].tab.projectPath
          const result = await sessions.loadHistoryFor(projectPath, true)
          if (!result.ok || !options.ready() || !await options.validateRoot()) return
          const remaining = live.filter(current)
          // 留下最后一次可信标题，后续读失败也不会退回启动时的首问。
          for (const owner of remaining) {
            const matches = result.sessions.filter(row => row.sessionId === owner.tab.sessionId)
            if (matches.length === 1) sessions.updateTabName(owner.tab.tabId, matches[0].name)
          }
          if (remaining.length) options.publish(projectPath)
        }))
      }
    } finally { reading = false; schedule() }
  }
  function queue(owner: Owner) { pending.set(owner.tab.tabId, owner); schedule() }
  watch(() => [...sessions.tabs.values()].map(tab => [tab, identity(tab), eligible(tab), tab.titleChangeRevision] as const), () => {
    for (const [id, owner] of owners) if (!current(owner)) {
      clearTimeout(owner.timer); owners.delete(id); pending.delete(id)
    }
    for (const tab of sessions.tabs.values()) if (eligible(tab)) {
      const titleRevision = tab.titleChangeRevision ?? 0
      let owner = owners.get(tab.tabId)
      if (!owner) {
        owner = { tab, identity: identity(tab), titleRevision }
        owners.set(tab.tabId, owner); queue(owner)
      } else if (owner.titleRevision !== titleRevision) {
        owner.titleRevision = titleRevision; queue(owner)
      }
    }
  }, { immediate: true })
  watch(options.ready, schedule)
  const unsubscribe = useHookStore().subscribe(['sessionStart', 'userPromptSubmit', 'stop', 'stopFailure'], event => {
    const tab = event.ptyId ? sessions.getTabByPtyId(event.ptyId) : undefined
    const owner = tab && owners.get(tab.tabId)
    if (!owner || !current(owner) || event.observerSource !== 'claude-hook' || event.runId !== tab!.ptyId
      || event.generation !== 1 || event.sessionId !== tab!.sessionId || !event.eventId || owner.eventId === event.eventId) return
    owner.eventId = event.eventId
    clearTimeout(owner.timer); owner.timer = undefined
    queue(owner)
    if (event.detail.type !== 'stop' && event.detail.type !== 'stopFailure') return
    // CLI 标题可能晚于 Stop 写盘；仅在 15/30/60 秒补读，不引入持续轮询。
    const delays = [15_000, 15_000, 30_000]
    const retry = (index: number) => {
      owner.timer = setTimeout(() => {
        owner.timer = undefined
        if (!current(owner)) return
        queue(owner)
        if (index + 1 < delays.length) retry(index + 1)
      }, delays[index])
    }
    retry(0)
  })
  onScopeDispose(() => {
    disposed = true; unsubscribe(); clearTimeout(timer)
    for (const owner of owners.values()) clearTimeout(owner.timer)
    owners.clear(); pending.clear()
  })
}
