import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useNotificationsStore, type ToastInput } from '@/stores/notifications'

beforeEach(() => { setActivePinia(createPinia()); vi.useFakeTimers() })
afterEach(() => { useNotificationsStore().clearToasts(); vi.useRealTimers() })

describe('Notifications_BoundedSafeFeedback', () => {
  // 相同去重键返回同一条消息，不重复插入。
  it('Toast_DedupeKey_001', () => {
    const store = useNotificationsStore()
    const first = store.pushToast({ kind: 'success', messageKey: 'rename', dedupeKey: 'rename:session-1' })
    const second = store.pushToast({ kind: 'success', messageKey: 'rename', dedupeKey: 'rename:session-1' })
    expect(second).toBe(first)
    expect(store.toasts).toHaveLength(1)
  })

  // 没有去重键的独立动作也最多保留最近三条。
  it('Toast_MaxThree_002', () => {
    const store = useNotificationsStore()
    const ids = Array.from({ length: 4 }, () => store.pushToast({ kind: 'success', messageKey: 'rename' }))
    expect(store.toasts.map((toast) => toast.id)).toEqual(ids.slice(1))
  })

  // 错误对象和原始错误文本不进入 store。
  it('Toast_RejectRawErrors_003', () => {
    const store = useNotificationsStore()
    expect(store.pushToast(new Error('Authorization: Bearer secret') as unknown as ToastInput)).toBeNull()
    expect(store.pushToast({ kind: 'error', messageKey: '/home/private Authorization: Bearer secret' } as unknown as ToastInput)).toBeNull()
    expect(store.toasts).toEqual([])
  })

  // 调用对象附带原始异常字段也不被复制到持久状态。
  it('Toast_CopySafeFields_004', () => {
    const store = useNotificationsStore()
    store.pushToast({ kind: 'info', messageKey: 'rename', message: 'secret', error: new Error('secret') } as ToastInput)
    expect(JSON.stringify(store.$state)).not.toContain('secret')
    expect(store.toasts[0].messageKey).toBe('rename')
  })

  // 非法 kind 不生成可视消息。
  it('Toast_RejectKind_005', () => {
    const store = useNotificationsStore()
    expect(store.pushToast({ kind: 'fatal', messageKey: 'rename' } as unknown as ToastInput)).toBeNull()
    expect(store.toasts).toHaveLength(0)
  })

  // 短反馈五秒后结束，显式关闭消息同样移除定时器。
  it('Toast_ExpireDismiss_006', () => {
    const store = useNotificationsStore()
    const first = store.pushToast({ kind: 'success', messageKey: 'rename' })!
    store.dismissToast(first)
    store.pushToast({ kind: 'info', messageKey: 'archive' })
    vi.advanceTimersByTime(4999)
    expect(store.toasts).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(store.toasts).toHaveLength(0)
    expect(vi.getTimerCount()).toBe(0)
  })

  // 指针/键盘阅读期间暂停计时，离开后只继续剩余时长。
  it('Toast_PauseResume_007', () => {
    const store = useNotificationsStore()
    const id = store.pushToast({ kind: 'success', messageKey: 'rename' })!
    vi.advanceTimersByTime(2000); store.pauseToast(id)
    vi.advanceTimersByTime(10000)
    expect(store.toasts).toHaveLength(1)
    store.resumeToast(id); vi.advanceTimersByTime(2999)
    expect(store.toasts).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(store.toasts).toHaveLength(0)
  })

  // 清空及销毁不遗留定时器，之后可再次产生反馈。
  it('Toast_ClearTimers_008', () => {
    const store = useNotificationsStore()
    store.pushToast({ kind: 'success', messageKey: 'rename' })
    store.clearToasts()
    expect(store.toasts).toHaveLength(0)
    expect(vi.getTimerCount()).toBe(0)
    expect(store.pushToast({ kind: 'info', messageKey: 'archive' })).toBeTypeOf('string')
    store.$dispose()
    expect(vi.getTimerCount()).toBe(0)
  })
})
