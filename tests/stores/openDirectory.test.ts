import { setActivePinia, createPinia } from 'pinia'
import { mockIPC } from '@tauri-apps/api/mocks'
import { beforeEach, describe, it, expect, vi } from 'vitest'
import { useAppStore } from '@/stores/app'

// Mock @/api/tauri
vi.mock('@/api/tauri', () => ({
  getAppConfig: vi.fn().mockResolvedValue({}),
  updateAppConfig: vi.fn().mockResolvedValue(undefined),
  saveLastProject: vi.fn().mockResolvedValue(undefined),
  saveDefaultClaudeOptions: vi.fn().mockResolvedValue(undefined),
  getHomeData: vi.fn().mockResolvedValue({ projects: [], recentSessions: [], hasMore: false }),
  getProjects: vi.fn().mockResolvedValue([]),
  getCheckResults: vi.fn().mockResolvedValue([]),
  runChecks: vi.fn().mockResolvedValue([]),
}))

// 平台确定性（T2 concern）：强制 Windows，使大小写不敏感测试在任意宿主全绿
vi.mock('@/utils/platform', () => ({
  detectPlatform: () => 'windows',
  platform: 'windows',
  isMac: false,
  isWindows: true,
  ctrl: 'Ctrl',
  alt: 'Alt',
  cmd: 'Ctrl',
  getClaudePlatformKey: () => 'win32-x64',
}))

beforeEach(() => {
  setActivePinia(createPinia())
  mockIPC(() => {})
})

// ==================== isKnownProject ====================

describe('isKnownProject', () => {
  // cachedProjects 中有完全匹配的路径时返回 true
  it('IsKnown_ExactMatch_001', () => {
    const store = useAppStore()
    store.cachedProjects = [{ path: 'C:/Users/dev/myproject', name: 'myproject' }]
    expect(store.isKnownProject('C:/Users/dev/myproject')).toBe(true)
  })

  // 路径大小写不同但仍匹配（归一化为小写比较）
  it('IsKnown_CaseInsensitive_001', () => {
    const store = useAppStore()
    store.cachedProjects = [{ path: 'C:/Users/Dev/MyProject', name: 'MyProject' }]
    expect(store.isKnownProject('c:/users/dev/myproject')).toBe(true)
  })

  // Windows 反斜杠与正斜杠混合时仍匹配
  it('IsKnown_SlashNormalize_001', () => {
    const store = useAppStore()
    store.cachedProjects = [{ path: 'C:\\Users\\dev\\project', name: 'project' }]
    expect(store.isKnownProject('C:/Users/dev/project')).toBe(true)
  })

  // cachedProjects 中无匹配路径时返回 false
  it('IsKnown_NoMatch_001', () => {
    const store = useAppStore()
    store.cachedProjects = [{ path: 'C:/Users/dev/project-a', name: 'project-a' }]
    expect(store.isKnownProject('C:/Users/dev/project-b')).toBe(false)
  })

  // cachedProjects 为空时返回 false
  it('IsKnown_EmptyList_001', () => {
    const store = useAppStore()
    store.cachedProjects = []
    expect(store.isKnownProject('C:/Users/dev/project')).toBe(false)
  })

  // 路径末尾有斜杠时仍匹配（normalizePath 去尾斜杠，trailing-slash 变体视为同一项目）
  it('IsKnown_TrailingSlash_001', () => {
    const store = useAppStore()
    store.cachedProjects = [{ path: 'C:/Users/dev/project', name: 'project' }]
    expect(store.isKnownProject('C:/Users/dev/project/')).toBe(true)
  })
})
