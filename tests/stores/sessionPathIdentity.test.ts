import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
// @ts-expect-error - node:crypto polyfill（与 sessionTree.test.ts 一致）
import { randomUUID } from 'crypto'

if (typeof globalThis.crypto === 'undefined' || !globalThis.crypto.randomUUID) {
  Object.defineProperty(globalThis, 'crypto', {
    value: { ...globalThis.crypto, randomUUID: () => randomUUID() },
    writable: true, configurable: true,
  })
}

// 平台身份矩阵（codex 重要#3）：doMock + 动态 import 注入平台，测 store 级路径身份行为
describe('session store 路径身份（平台感知）', () => {
  beforeEach(() => {
    vi.resetModules()
    setActivePinia(createPinia())
  })

  // Windows：大小写不敏感 -> E:\Repo 与 e:/repo 同一 key，set 删旧覆盖
  it('SetDisplayName_WindowsCaseInsensitive_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'windows', platform: 'windows',
      isMac: false, isWindows: true,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'win32-x64',
    }))
    vi.doMock('@/api/tauri', () => ({
      getProjectsState: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {} }),
      updateProjectsState: vi.fn().mockResolvedValue(undefined),
      getSessions: vi.fn().mockResolvedValue([]),
      ptyKill: vi.fn().mockResolvedValue(true),
      getSessionCount: vi.fn().mockResolvedValue(0),
      searchSessionMessages: vi.fn().mockResolvedValue([]),
    }))
    const { useSessionStore } = await import('@/stores/session')
    const store = useSessionStore()
    store.displayNames.set('e:/repo', '旧')
    await store.setDisplayName('E:\\Repo', '新')
    expect([...store.displayNames.keys()].length).toBe(1)
    expect(store.getDisplayName('e:/repo')).toBe('新')
    expect(store.getDisplayName('E:\\REPO')).toBe('新')
  })

  // Linux：大小写敏感 -> /work/Foo 与 /work/foo 不同 key，各自保留
  it('SetDisplayName_LinuxCaseSensitive_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'linux', platform: 'linux',
      isMac: false, isWindows: false,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'linux-x64',
    }))
    vi.doMock('@/api/tauri', () => ({
      getProjectsState: vi.fn().mockResolvedValue({ pinnedProjects: [], archivedSessions: {} }),
      updateProjectsState: vi.fn().mockResolvedValue(undefined),
      getSessions: vi.fn().mockResolvedValue([]),
      ptyKill: vi.fn().mockResolvedValue(true),
      getSessionCount: vi.fn().mockResolvedValue(0),
      searchSessionMessages: vi.fn().mockResolvedValue([]),
    }))
    const { useSessionStore } = await import('@/stores/session')
    const store = useSessionStore()
    await store.setDisplayName('/work/Foo', 'A')
    await store.setDisplayName('/work/foo', 'B')
    // Linux 不 lower -> 两个不同 key 各自保留
    expect([...store.displayNames.keys()].length).toBe(2)
    expect(store.getDisplayName('/work/Foo')).toBe('A')
    expect(store.getDisplayName('/work/foo')).toBe('B')
  })

  // Linux load：混合大小写 key 各自独立入 Map（不合并）
  it('Load_LinuxCaseSensitive_001', async () => {
    vi.doMock('@/utils/platform', () => ({
      detectPlatform: () => 'linux', platform: 'linux',
      isMac: false, isWindows: false,
      ctrl: 'Ctrl', alt: 'Alt', cmd: 'Ctrl', getClaudePlatformKey: () => 'linux-x64',
    }))
    vi.doMock('@/api/tauri', () => ({
      getProjectsState: vi.fn().mockResolvedValue({
        pinnedProjects: [], archivedSessions: {},
        displayNames: { '/work/Foo': 'A', '/work/foo': 'B' },
      }),
      updateProjectsState: vi.fn().mockResolvedValue(undefined),
      getSessions: vi.fn().mockResolvedValue([]),
      ptyKill: vi.fn().mockResolvedValue(true),
      getSessionCount: vi.fn().mockResolvedValue(0),
      searchSessionMessages: vi.fn().mockResolvedValue([]),
    }))
    const { useSessionStore } = await import('@/stores/session')
    const store = useSessionStore()
    await store.loadProjectsState()
    expect([...store.displayNames.keys()].length).toBe(2)  // Linux 不合并大小写
    expect(store.getDisplayName('/work/Foo')).toBe('A')
    expect(store.getDisplayName('/work/foo')).toBe('B')
  })
})
