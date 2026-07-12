import { describe, it, expect } from 'vitest'
import { decideStartupView } from '@/composables/useStartupDecision'
import type { ProjectStartupState } from '@/types/app'

const mkState = (over: Partial<ProjectStartupState> = {}): ProjectStartupState => ({
  hasAnyProject: true,
  hasVisibleProject: true,
  lastOpenedProjectInfo: null,
  ...over,
})
const noHidden = (_p: string) => false

describe('decideStartupView 启动决策 5 场景', () => {
  // 1. 无项目 -> welcome
  it('Decide_NoProject_Welcome_001', () => {
    const d = decideStartupView(mkState({ hasAnyProject: false, hasVisibleProject: false }), '', noHidden)
    expect(d.view).toBe('welcome')
    expect(d.openSessionsPanel).toBe(false)
    expect(d.restoreProject).toBeNull()
  })

  // 2. 全隐藏 -> projects（管理页）
  it('Decide_AllHidden_Projects_001', () => {
    const d = decideStartupView(mkState({ hasAnyProject: true, hasVisibleProject: false }), '', noHidden)
    expect(d.view).toBe('projects')
    expect(d.restoreProject).toBeNull()
  })

  // 3. 有可见 + lastOpened 有效未隐藏 -> terminal + restore
  it('Decide_LastOpenedValid_TerminalRestore_001', () => {
    const state = mkState({
      lastOpenedProjectInfo: { path: '/p-x', name: 'p-x', exists: true },
    })
    const d = decideStartupView(state, '/p-x', noHidden)
    expect(d.view).toBe('terminal')
    expect(d.restoreProject).toBe('/p-x')
    expect(d.openSessionsPanel).toBe(false)
  })

  // 4. 有可见 + 无 lastOpened（首次）-> terminal + 自动开 Sessions
  it('Decide_FirstRun_TerminalSessions_001', () => {
    const d = decideStartupView(mkState(), '', noHidden)
    expect(d.view).toBe('terminal')
    expect(d.openSessionsPanel).toBe(true)
    expect(d.restoreProject).toBeNull()
  })

  // 5. 有可见 + lastOpened 隐藏 -> terminal + 自动开 Sessions（不 setCwd）
  it('Decide_LastOpenedHidden_TerminalSessions_001', () => {
    const state = mkState({
      lastOpenedProjectInfo: { path: '/p-h', name: 'p-h', exists: true },
    })
    const d = decideStartupView(state, '/p-h', (p) => p === '/p-h')
    expect(d.view).toBe('terminal')
    expect(d.openSessionsPanel).toBe(true)
    expect(d.restoreProject).toBeNull()
  })

  // 5b. lastOpened 无效（exists=false）-> terminal + Sessions（不 restore）
  it('Decide_LastOpenedInvalid_TerminalSessions_001', () => {
    const state = mkState({
      lastOpenedProjectInfo: { path: '/p-gone', name: 'p-gone', exists: false },
    })
    const d = decideStartupView(state, '/p-gone', noHidden)
    expect(d.view).toBe('terminal')
    expect(d.openSessionsPanel).toBe(true)
    expect(d.restoreProject).toBeNull()
  })

  // 规范化比较：lastOpened 路径大小写差异仍判定未隐藏
  it('Decide_NormalizedNotHidden_001', () => {
    const state = mkState({
      lastOpenedProjectInfo: { path: 'E:\\Source\\Foo', name: 'Foo', exists: true },
    })
    const d = decideStartupView(state, 'E:\\Source\\Foo', (p) => p === 'e:/source/bar')
    expect(d.restoreProject).toBe('E:\\Source\\Foo')
  })

  // 8. lastOpened 有效但 info 为 null（后端未返回 info）-> terminal + Sessions（保守引导）
  it('Decide_LastOpenedNoInfo_TerminalSessions_001', () => {
    const d = decideStartupView(mkState({ lastOpenedProjectInfo: null }), '/p-x', noHidden)
    expect(d.view).toBe('terminal')
    expect(d.openSessionsPanel).toBe(true)
    expect(d.restoreProject).toBeNull()
  })
})
