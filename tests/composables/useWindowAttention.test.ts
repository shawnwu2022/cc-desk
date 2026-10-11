import { setActivePinia, createPinia } from 'pinia'
import { beforeEach, describe, it, expect, vi } from 'vitest'
import { createApp, defineComponent } from 'vue'
import { useWindowAttention } from '@/composables/useWindowAttention'

// mock 状态须经 vi.hoisted 提升，工厂在 import 阶段执行时才可访问
const attentionApi = vi.hoisted(() => ({
  focusHandlers: [] as ((event: { payload: boolean }) => void)[],
  focusedResult: true,
  // 非 null 时 isFocused() 挂起直到手动 resolve，用于构造初始快照迟到场景
  pendingInitial: null as Promise<boolean> | null,
  attentionCalls: [] as (string | null)[],
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    onFocusChanged: (handler: (event: { payload: boolean }) => void) => {
      attentionApi.focusHandlers.push(handler)
      return Promise.resolve(() => {})
    },
    isFocused: () =>
      attentionApi.pendingInitial ?? Promise.resolve(attentionApi.focusedResult),
    requestUserAttention: (kind: string | null) => {
      attentionApi.attentionCalls.push(kind)
      return Promise.resolve()
    },
  }),
}))

let resolvePendingInitial: ((value: boolean) => void) | null = null

// 用 Vue 自带 createApp 挂载最小宿主组件，触发 onMounted/onUnmounted 生命周期
function mountAttention() {
  let api!: ReturnType<typeof useWindowAttention>
  const container = document.createElement('div')
  document.body.appendChild(container)
  const app = createApp(defineComponent({
    setup() {
      api = useWindowAttention()
      return () => null
    },
  }))
  app.mount(container)
  const unmount = () => { app.unmount(); container.remove() }
  return { unmount, api }
}

// 排空微任务队列（onMounted 的 async 初始化链全部落在 microtask 上）
const flush = () => new Promise<void>(resolve => setTimeout(resolve, 0))

beforeEach(() => {
  setActivePinia(createPinia())
  attentionApi.focusHandlers = []
  attentionApi.attentionCalls = []
  attentionApi.focusedResult = true
  attentionApi.pendingInitial = null
  resolvePendingInitial = null
})

describe('useWindowAttention', () => {
  // 失焦事件更新 isFocused/focusReady；重新聚焦时请求一次用户关注（任务栏跳动）
  it('WindowAttention_FocusEvent_UpdatesState_001', async () => {
    attentionApi.focusedResult = false
    const { api } = mountAttention()
    await flush()
    expect(api.isFocused.value).toBe(false)
    expect(api.focusReady.value).toBe(true)
    expect(attentionApi.attentionCalls).toEqual([])

    attentionApi.focusHandlers[0]({ payload: true })
    expect(api.isFocused.value).toBe(true)
    expect(attentionApi.attentionCalls).toEqual([null])
  })

  // focusEpoch 竞态：初始 isFocused() 快照迟到，且期间已发生焦点事件 —— 过期快照被丢弃
  it('WindowAttention_StaleInitialSnapshotDiscarded_001', async () => {
    attentionApi.pendingInitial = new Promise<boolean>(resolve => { resolvePendingInitial = resolve })
    const { api } = mountAttention()
    await flush()
    expect(api.focusReady.value).toBe(false)

    // 初始快照尚未返回时，先发生一次失焦事件（epoch 推进）
    attentionApi.focusHandlers[0]({ payload: false })
    expect(api.isFocused.value).toBe(false)

    // 迟到的初始快照声称"已聚焦"，不得覆盖事件给出的状态
    resolvePendingInitial!(true)
    await flush()
    expect(api.isFocused.value).toBe(false)
    expect(api.focusReady.value).toBe(true)
  })

  // 卸载时调用 requestUserAttention(null) 取消任务栏跳动
  it('WindowAttention_UnmountCancelsUserAttention_001', async () => {
    const { unmount } = mountAttention()
    await flush()
    attentionApi.focusHandlers[0]({ payload: true })
    expect(attentionApi.attentionCalls).toEqual([null])

    unmount()
    expect(attentionApi.attentionCalls).toEqual([null, null])
  })

  // 卸载后残留的焦点事件与迟到的初始快照都不生效：状态保持挂载前的初始值
  it('WindowAttention_EventAfterUnmountIgnored_001', async () => {
    // 挂起初始快照，保证挂载后 focusReady 仍为 false
    attentionApi.pendingInitial = new Promise<boolean>(resolve => { resolvePendingInitial = resolve })
    const { unmount, api } = mountAttention()
    await flush()
    expect(api.focusReady.value).toBe(false)

    unmount()
    attentionApi.focusHandlers[0]({ payload: true })
    resolvePendingInitial!(true)
    await flush()
    expect(api.isFocused.value).toBe(true)
    expect(api.focusReady.value).toBe(false)
    // 唯一一次调用来自卸载时的取消；被忽略的焦点事件未追加新的请求
    expect(attentionApi.attentionCalls).toEqual([null])
  })
})
