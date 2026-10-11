import { setActivePinia, createPinia } from 'pinia'
import { mockIPC } from '@tauri-apps/api/mocks'
import { beforeEach, describe, it, expect } from 'vitest'
import { useConfigStore } from '@/stores/config'
import type { ProjectConfigResult } from '@/types'

// 只用 basic 列表区分来源，足以断言"加载到的是哪一次响应"
function configResult(tag: string): ProjectConfigResult {
  return {
    basic: [{ source: { type: 'project', label: tag } }],
    mcp: [],
    skills: [],
    agents: [],
    hooks: [],
  }
}

function currentSource(): string | undefined {
  return useConfigStore().projectConfig?.basic[0]?.source.label
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockIPC((cmd) => {
    if (cmd === 'get_project_config') return configResult('default')
    return null
  })
})

describe('config store loadProjectConfig', () => {
  // 正常路径：加载成功后 projectConfig 保存后端返回值，error/isLoading 归零
  it('Config_LoadProjectConfig_StoresResult_001', async () => {
    const store = useConfigStore()
    await store.loadProjectConfig('/p-a')
    expect(store.projectConfig).toEqual(configResult('default'))
    expect(store.error).toBeNull()
    expect(store.isLoading).toBe(false)
  })

  // 缓存命中：同一 projectPath 第二次加载不再发起 get_project_config 调用
  it('Config_LoadProjectConfig_CacheHitSkipsSecondRead_001', async () => {
    let callCount = 0
    mockIPC((cmd) => {
      if (cmd === 'get_project_config') { callCount++; return configResult('default') }
      return null
    })
    const store = useConfigStore()
    await store.loadProjectConfig('/p-a')
    await store.loadProjectConfig('/p-a')
    expect(callCount).toBe(1)
    expect(store.projectConfig).toEqual(configResult('default'))
    expect(store.isLoading).toBe(false)
    expect(store.error).toBeNull()
  })

  // requestOwner 竞态：A 慢 B 快，B 完成后 A 才返回 —— 过期响应被丢弃，保留 B 的结果
  it('Config_RequestOwner_LateFirstResponseDiscarded_001', async () => {
    let resolveA!: (value: ProjectConfigResult) => void
    mockIPC((cmd, args) => {
      if (cmd !== 'get_project_config') return null
      const path = (args as { projectPath: string }).projectPath
      if (path === '/p-a') return new Promise<ProjectConfigResult>(resolve => { resolveA = resolve })
      return configResult('b')
    })
    const store = useConfigStore()
    const a = store.loadProjectConfig('/p-a')
    await store.loadProjectConfig('/p-b')
    expect(currentSource()).toBe('b')
    resolveA(configResult('a'))
    await a
    expect(currentSource()).toBe('b')
    expect(store.error).toBeNull()
    expect(store.isLoading).toBe(false)
  })

  // requestOwner 竞态：A 慢且失败，B 成功在先 —— A 的过期错误不污染状态
  it('Config_RequestOwner_LateFirstFailureDiscarded_001', async () => {
    let rejectA!: (reason: unknown) => void
    mockIPC((cmd, args) => {
      if (cmd !== 'get_project_config') return null
      const path = (args as { projectPath: string }).projectPath
      if (path === '/p-a') return new Promise<ProjectConfigResult>((_resolve, reject) => { rejectA = reject })
      return configResult('b')
    })
    const store = useConfigStore()
    const a = store.loadProjectConfig('/p-a')
    await store.loadProjectConfig('/p-b')
    rejectA(new Error('stale failure'))
    await a
    expect(currentSource()).toBe('b')
    expect(store.error).toBeNull()
  })

  // 加载失败回退：invoke 抛错时 error 置为回退文案，projectConfig 保持为空
  it('Config_LoadProjectConfig_FailureSetsError_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_project_config') throw new Error('backend read failed')
      return null
    })
    const store = useConfigStore()
    await store.loadProjectConfig('/p-a')
    expect(store.projectConfig).toBeNull()
    expect(store.error).toBe('Failed to load project config')
    expect(store.isLoading).toBe(false)
  })

  // 空 projectPath：走 clearConfig 回退，已有配置被清空
  it('Config_EmptyProjectPath_ClearsConfig_001', async () => {
    const store = useConfigStore()
    await store.loadProjectConfig('/p-a')
    expect(store.projectConfig).not.toBeNull()
    await store.loadProjectConfig('')
    expect(store.projectConfig).toBeNull()
    expect(store.error).toBeNull()
    expect(store.isLoading).toBe(false)
  })
})
