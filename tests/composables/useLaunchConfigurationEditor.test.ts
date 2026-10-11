import { setActivePinia, createPinia } from 'pinia'
import { mockIPC } from '@tauri-apps/api/mocks'
import { beforeEach, describe, it, expect } from 'vitest'
import { createApp, defineComponent, ref, type Ref } from 'vue'
import { useLaunchConfigurationEditor } from '@/composables/useLaunchConfigurationEditor'

// cli_list_profiles 返回空档案列表：create 不依赖已有档案；edit 查不到档案走错误路径
beforeEach(() => {
  setActivePinia(createPinia())
  mockIPC((cmd) => {
    if (cmd === 'cli_list_profiles') return { revision: '1', profiles: [] }
    return null
  })
})

// 用 Vue 自带 createApp 挂载最小宿主组件，触发 onBeforeUnmount 生命周期
function mountEditor(active: Ref<boolean>) {
  let api!: ReturnType<typeof useLaunchConfigurationEditor>
  const container = document.createElement('div')
  document.body.appendChild(container)
  const app = createApp(defineComponent({
    setup() {
      api = useLaunchConfigurationEditor(() => active.value)
      return () => null
    },
  }))
  app.mount(container)
  const unmount = () => { app.unmount(); container.remove() }
  return { unmount, api }
}

describe('useLaunchConfigurationEditor', () => {
  // create 请求：档案加载完成后挂载编辑器草稿，opening/error 归零
  it('Editor_OpenCreate_PublishesEditorDraft_001', async () => {
    const { api } = mountEditor(ref(true))
    const request = { kind: 'create', cli: 'claude' } as const
    await api.open(request)
    expect(api.editor.value).toEqual(request)
    expect(api.opening.value).toBe(false)
    expect(api.error.value).toBeNull()
  })

  // edit 请求引用不存在的档案：挂载被拒绝，error 报 PROFILE_SELECTION_CHANGED
  it('Editor_OpenEditMissingProfile_SetsSelectionChangedError_001', async () => {
    const { api } = mountEditor(ref(true))
    await api.open({ kind: 'edit', profileId: 'missing-id' })
    expect(api.editor.value).toBeNull()
    expect(api.opening.value).toBe(false)
    expect(api.error.value?.detailCode).toBe('PROFILE_SELECTION_CHANGED')
    expect(api.error.value?.context).toBe('settings')
  })

  // close() 清空编辑器、opening 与 error，可重新打开
  it('Editor_Close_ClearsEditorAndError_001', async () => {
    const { api } = mountEditor(ref(true))
    await api.open({ kind: 'create', cli: 'codex' })
    expect(api.editor.value).not.toBeNull()

    api.close()
    expect(api.editor.value).toBeNull()
    expect(api.opening.value).toBe(false)
    expect(api.error.value).toBeNull()
  })

  // active 变为 false（如切走设置页）时 watch 同步关闭编辑器
  it('Editor_Deactivate_ClosesEditor_001', async () => {
    const active = ref(true)
    const { api } = mountEditor(active)
    await api.open({ kind: 'create', cli: 'claude' })
    expect(api.editor.value).not.toBeNull()

    active.value = false
    expect(api.editor.value).toBeNull()
    expect(api.opening.value).toBe(false)
  })

  // epoch 竞态：打开中途调用 close()，迟到的档案加载结果不得再挂载编辑器
  it('Editor_OpenStaleAfterClose_DiscardedDraft_001', async () => {
    let resolveList!: (value: unknown) => void
    mockIPC((cmd) => {
      if (cmd === 'cli_list_profiles') {
        return new Promise(resolve => { resolveList = resolve })
      }
      return null
    })
    const { api } = mountEditor(ref(true))
    const request = { kind: 'create', cli: 'claude' } as const
    const opening = api.open(request)
    api.close()
    resolveList({ revision: '1', profiles: [] })
    await opening
    expect(api.editor.value).toBeNull()
    expect(api.opening.value).toBe(false)
    expect(api.error.value).toBeNull()
  })

  // 非激活态调用 open：直接拒绝，不发起 cli_list_profiles 请求
  it('Editor_OpenWhenInactive_NoRequest_001', async () => {
    let callCount = 0
    mockIPC((cmd) => {
      if (cmd === 'cli_list_profiles') { callCount++; return { revision: '1', profiles: [] } }
      return null
    })
    const { api } = mountEditor(ref(false))
    await api.open({ kind: 'create', cli: 'claude' })
    expect(callCount).toBe(0)
    expect(api.editor.value).toBeNull()
    expect(api.opening.value).toBe(false)
    expect(api.error.value).toBeNull()
  })
})
