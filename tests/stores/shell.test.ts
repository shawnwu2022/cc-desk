import { setActivePinia, createPinia } from 'pinia'
import { beforeEach, describe, it, expect } from 'vitest'
import { useShellStore } from '@/stores/shell'

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('shell store clearWorkspaceRequest', () => {
  // 持有当前 sequence 的消费者清除请求后 pendingRequest 置空
  it('Shell_ClearWorkspaceRequest_MatchingSequenceClears_001', () => {
    const store = useShellStore()
    store.requestWorkspaceAction({ kind: 'refresh' })
    expect(store.pendingRequest).toEqual({ kind: 'refresh' })
    store.clearWorkspaceRequest(store.requestSequence)
    expect(store.pendingRequest).toBeNull()
  })

  // 过期 sequence 不清除：请求被新请求覆盖后，旧消费者持旧 sequence 清不掉新请求
  it('Shell_ClearWorkspaceRequest_StaleSequenceIgnored_001', () => {
    const store = useShellStore()
    store.requestWorkspaceAction({ kind: 'refresh' })
    const staleSequence = store.requestSequence
    store.requestWorkspaceAction({ kind: 'add-project' })
    store.clearWorkspaceRequest(staleSequence)
    expect(store.pendingRequest).toEqual({ kind: 'add-project' })
  })
})

describe('shell store responsiveMode', () => {
  // 阈值边界：899 compact / 900 overlay / 1179 overlay / 1180 wide
  it('Shell_ResponsiveMode_ThresholdBoundary_001', () => {
    const store = useShellStore()
    store.setViewportWidth(899)
    expect(store.responsiveMode).toBe('compact')
    store.setViewportWidth(900)
    expect(store.responsiveMode).toBe('overlay')
    store.setViewportWidth(1179)
    expect(store.responsiveMode).toBe('overlay')
    store.setViewportWidth(1180)
    expect(store.responsiveMode).toBe('wide')
  })
})

describe('shell store setSidebarWidth', () => {
  // 钳制范围 [240, 360]：过小取 240，过大取 360，范围内原样保留
  it('Shell_SidebarWidth_ClampedToRange_001', () => {
    const store = useShellStore()
    store.setSidebarWidth(100)
    expect(store.sidebarWidth).toBe(240)
    store.setSidebarWidth(1000)
    expect(store.sidebarWidth).toBe(360)
    store.setSidebarWidth(300)
    expect(store.sidebarWidth).toBe(300)
  })
})
