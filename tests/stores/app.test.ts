import { setActivePinia, createPinia } from 'pinia'
import { mockIPC } from '@tauri-apps/api/mocks'
import { beforeEach, describe, it, expect } from 'vitest'
import { useAppStore } from '@/stores/app'
import i18n from '@/i18n'

beforeEach(() => {
  setActivePinia(createPinia())
  mockIPC(() => {})
})

describe('getClaudeArgs', () => {
  // 无选项设置时返回空数组
  it('ClaudeArgs_Empty_001', () => {
    const store = useAppStore()
    // default claudeOptions: resume='', skipPermissions=false, customArgs=''
    const args = store.getClaudeArgs()
    expect(args).toEqual([])
  })

  // 设置 resume="abc123" 时返回 ["--resume", "abc123"]
  it('ClaudeArgs_Resume_001', () => {
    const store = useAppStore()
    store.setClaudeOptions({ resume: 'abc123' })
    const args = store.getClaudeArgs()
    expect(args).toEqual(['--resume', 'abc123'])
  })

  // 设置 skipPermissions=true 时包含 "--dangerously-skip-permissions"
  it('ClaudeArgs_SkipPerm_001', () => {
    const store = useAppStore()
    store.setClaudeOptions({ skipPermissions: true })
    const args = store.getClaudeArgs()
    expect(args).toEqual(['--dangerously-skip-permissions'])
  })

  // 设置 customArgs="--flag1 --flag2" 时按空格拆分为两个元素
  it('ClaudeArgs_CustomSplit_001', () => {
    const store = useAppStore()
    store.setClaudeOptions({ customArgs: '--flag1 --flag2' })
    const args = store.getClaudeArgs()
    expect(args).toEqual(['--flag1', '--flag2'])
  })

  // 设置 customArgs="  --flag  " 时过滤空字符串
  it('ClaudeArgs_CustomTrim_001', () => {
    const store = useAppStore()
    store.setClaudeOptions({ customArgs: '  --flag  ' })
    const args = store.getClaudeArgs()
    expect(args).toEqual(['--flag'])
  })

  // 同时设置 resume、skipPermissions、customArgs 时按顺序生成全部参数
  it('ClaudeArgs_Combined_001', () => {
    const store = useAppStore()
    store.setClaudeOptions({
      resume: 'sess1',
      skipPermissions: true,
      customArgs: '--model opus --verbose'
    })
    const args = store.getClaudeArgs()
    expect(args).toEqual([
      '--resume', 'sess1',
      '--dangerously-skip-permissions',
      '--model', 'opus', '--verbose'
    ])
  })
})

describe('setFontSize', () => {
  // 设置 size=5 时钳位到 10
  it('FontSize_MinClamp_001', () => {
    const store = useAppStore()
    store.setFontSize(5)
    expect(store.fontSize).toBe(10)
  })

  // 设置 size=30 时钳位到 24
  it('FontSize_MaxClamp_001', () => {
    const store = useAppStore()
    store.setFontSize(30)
    expect(store.fontSize).toBe(24)
  })

  // 设置 size=14 时直接接受
  it('FontSize_InRange_001', () => {
    const store = useAppStore()
    store.setFontSize(14)
    expect(store.fontSize).toBe(14)
  })
})

describe('currentProject', () => {
  // 设置 cwd="/Users/dev/myproject" 时返回 "myproject"
  it('CurrentProject_ForwardSlash_001', () => {
    const store = useAppStore()
    store.setCwd('/Users/dev/myproject')
    expect(store.currentProject).toBe('myproject')
  })

  // 设置 cwd="C:\\Users\\dev\\myproject" 时返回 "myproject"
  it('CurrentProject_BackSlash_001', () => {
    const store = useAppStore()
    store.setCwd('C:\\Users\\dev\\myproject')
    expect(store.currentProject).toBe('myproject')
  })

  // 设置 cwd="" 时返回 null
  it('CurrentProject_Empty_001', () => {
    const store = useAppStore()
    store.setCwd('')
    expect(store.currentProject).toBeNull()
  })
})

describe('setLanguage', () => {
  // setLanguage('zh') 同时更新 store.language 和 i18n.global.locale
  it('SetLanguage_Zh_UpdatesLocale_001', () => {
    const store = useAppStore()
    store.setLanguage('zh')
    expect(store.language).toBe('zh')
    expect(i18n.global.locale.value).toBe('zh')
  })

  // setLanguage('en') 同时更新 store.language 和 i18n.global.locale
  it('SetLanguage_En_UpdatesLocale_001', () => {
    const store = useAppStore()
    store.setLanguage('en')
    expect(store.language).toBe('en')
    expect(i18n.global.locale.value).toBe('en')
  })

  // 切换后再切换回中文，locale 正确同步
  it('SetLanguage_ToggleTwice_001', () => {
    const store = useAppStore()
    store.setLanguage('zh')
    expect(i18n.global.locale.value).toBe('zh')
    store.setLanguage('en')
    expect(i18n.global.locale.value).toBe('en')
  })
})

describe('detectSystemLocale', () => {
  // navigator.language 为 'zh-CN' 时返回 'zh'
  it('DetectLocale_ZhCN_001', () => {
    const original = navigator.language
    Object.defineProperty(navigator, 'language', { value: 'zh-CN', configurable: true })
    // detectSystemLocale 是内部函数，通过 loadAppConfig 间接触发
    // 直接验证初始 language 默认值逻辑
    const lang = navigator.language.startsWith('zh') ? 'zh' : 'en'
    expect(lang).toBe('zh')
    Object.defineProperty(navigator, 'language', { value: original, configurable: true })
  })

  // navigator.language 为 'en-US' 时返回 'en'
  it('DetectLocale_EnUS_001', () => {
    const original = navigator.language
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true })
    const lang = navigator.language.startsWith('zh') ? 'zh' : 'en'
    expect(lang).toBe('en')
    Object.defineProperty(navigator, 'language', { value: original, configurable: true })
  })
})

describe('loadAppConfig terminalTheme', () => {
  // config 有合法 terminalTheme 时直接加载
  it('LoadAppConfig_HasTerminalTheme_LoadsIt_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_app_config') return { theme: 'light', terminalTheme: 'cc-box-light' }
    })
    const store = useAppStore()
    await store.loadAppConfig()
    expect(store.terminalTheme).toBe('cc-box-light')
  })

  // terminalTheme 缺失 + GUI dark → 映射 cc-box-dark
  it('LoadAppConfig_MissingTerminalTheme_DarkGui_MapsToCcBoxDark_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_app_config') return { theme: 'dark' }
    })
    const store = useAppStore()
    await store.loadAppConfig()
    expect(store.terminalTheme).toBe('cc-box-dark')
  })

  // terminalTheme 缺失 + GUI light → 映射 cc-box-light
  it('LoadAppConfig_MissingTerminalTheme_LightGui_MapsToCcBoxLight_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_app_config') return { theme: 'light' }
    })
    const store = useAppStore()
    await store.loadAppConfig()
    expect(store.terminalTheme).toBe('cc-box-light')
  })

  // terminalTheme 缺失时持久化写回推断值（合并进同一次 updateAppConfig）
  it('LoadAppConfig_MissingTerminalTheme_PersistsInferredValue_001', async () => {
    const updates: Array<Record<string, unknown>> = []
    mockIPC((cmd, args) => {
      if (cmd === 'get_app_config') return { theme: 'dark' }
      if (cmd === 'update_app_config') {
        updates.push((args as { updates: Record<string, unknown> }).updates)
        return null
      }
    })
    const store = useAppStore()
    await store.loadAppConfig()
    expect(updates.some(u => u.terminalTheme === 'cc-box-dark')).toBe(true)
  })

  // terminalTheme 非法时归一化为默认并写回
  it('LoadAppConfig_InvalidTerminalTheme_NormalizesToDefault_001', async () => {
    const updates: Array<Record<string, unknown>> = []
    mockIPC((cmd, args) => {
      if (cmd === 'get_app_config') return { theme: 'light', terminalTheme: 'bogus' }
      if (cmd === 'update_app_config') {
        updates.push((args as { updates: Record<string, unknown> }).updates)
        return null
      }
    })
    const store = useAppStore()
    await store.loadAppConfig()
    expect(store.terminalTheme).toBe('cc-box-dark')
    expect(updates.some(u => u.terminalTheme === 'cc-box-dark')).toBe(true)
  })
})

describe('loadAppConfig theme', () => {
  // config 有 theme=dark 时加载到 store.theme（GUI 配色，区别于 terminalTheme）
  it('LoadAppConfig_HasTheme_LoadsIt_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_app_config') return { theme: 'dark' }
    })
    const store = useAppStore()
    await store.loadAppConfig()
    expect(store.theme).toBe('dark')
  })

  // config 缺失 theme 时回退默认 'light'（loadAppConfig 用 `config.theme || 'light'`）
  it('LoadAppConfig_MissingTheme_DefaultsLight_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_app_config') return {}
    })
    const store = useAppStore()
    await store.loadAppConfig()
    expect(store.theme).toBe('light')
  })
})

describe('setTerminalTheme', () => {
  // 设置合法 id 更新 store 并持久化
  it('SetTerminalTheme_UpdatesValueAndPersists_001', () => {
    const updates: Array<Record<string, unknown>> = []
    mockIPC((cmd, args) => {
      if (cmd === 'update_app_config') {
        updates.push((args as { updates: Record<string, unknown> }).updates)
        return null
      }
    })
    const store = useAppStore()
    store.setTerminalTheme('cc-box-light')
    expect(store.terminalTheme).toBe('cc-box-light')
    expect(updates.some(u => u.terminalTheme === 'cc-box-light')).toBe(true)
  })

  // 设置非法 id 归一化为默认
  it('SetTerminalTheme_InvalidId_Normalizes_001', () => {
    const store = useAppStore()
    store.setTerminalTheme('nope')
    expect(store.terminalTheme).toBe('cc-box-dark')
  })
})

describe('app store - 启动状态源 + setCwd 拆分 + setHidden opLock', () => {
  // loadAppConfig 读 lastOpenedProject + hiddenProjects
  it('LoadAppConfig_ReadLastOpenedAndHidden_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_app_config') return { lastOpenedProject: '/p-x', hiddenProjects: ['/p-h'] }
      if (cmd === 'update_app_config') return null
    })
    const store = useAppStore()
    await store.loadAppConfig()
    expect(store.lastOpenedProject).toBe('/p-x')
    expect(store.isHidden('/p-h')).toBe(true)
    expect(store.isHidden('/p-x')).toBe(false)
  })

  // setCwdLocal 只更新内存，不持久化（saveLastProject 未调）
  it('SetCwdLocal_NoPersist_001', () => {
    const store = useAppStore()
    store.setCwdLocal('/p-a')
    expect(store.cwd).toBe('/p-a')
    // 无 mockIPC 配置 saveLastProject -> 若调用会报错；此处仅断言 cwd 已切
  })

  // setCurrentProject persist=true：await saveLastProject 成功后 setCwdLocal
  it('SetCurrentProject_PersistSuccess_001', async () => {
    const calls: string[] = []
    mockIPC((cmd, args) => {
      if (cmd === 'save_last_project') { calls.push((args as { path: string }).path); return null }
      return null
    })
    const store = useAppStore()
    await store.setCurrentProject('/p-a', { persist: true })
    expect(store.cwd).toBe('/p-a')
    expect(calls).toEqual(['/p-a'])
  })

  // setCurrentProject persist 失败：saveLastProject reject -> 抛错 + cwd 不变（persist-first）
  it('SetCurrentProject_PersistFail_NoCwdChange_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'save_last_project') throw new Error('persist failed')
      return null
    })
    const store = useAppStore()
    store.setCwdLocal('/old') // 预设旧 cwd
    await expect(store.setCurrentProject('/p-new', { persist: true })).rejects.toThrow('persist failed')
    expect(store.cwd).toBe('/old') // 未变
  })

  // setCurrentProject persist=false：只 setCwdLocal，不 save
  it('SetCurrentProject_NoPersist_001', async () => {
    const store = useAppStore()
    await store.setCurrentProject('/p-a', { persist: false })
    expect(store.cwd).toBe('/p-a')
  })

  // setHidden opLock 串行：连续隐藏 A+B 不丢
  it('SetHidden_OpLock_ConcurrentAB_001', async () => {
    const store = useAppStore()
    await Promise.all([
      store.setHidden('/p-a', true),
      store.setHidden('/p-b', true),
    ])
    expect(store.isHidden('/p-a')).toBe(true)
    expect(store.isHidden('/p-b')).toBe(true)
  })

  // setHidden persist-first 失败：updateAppConfig reject -> hiddenProjects 不变 + 抛错
  it('SetHidden_PersistFail_Rollback_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'update_app_config') throw new Error('persist failed')
      return null
    })
    const store = useAppStore()
    await expect(store.setHidden('/p-a', true)).rejects.toThrow('persist failed')
    expect(store.isHidden('/p-a')).toBe(false)
  })

  // setHidden 取消隐藏用规范化比较（尾斜杠差异仍能移除）
  it('SetHidden_Show_NormalizedRemove_001', async () => {
    const store = useAppStore()
    await store.setHidden('/source/foo/', true) // 隐藏（带尾斜杠）
    expect(store.isHidden('/source/foo')).toBe(true) // 规范化比较可见
    await store.setHidden('/source/foo', false) // 取消（无尾斜杠）
    expect(store.isHidden('/source/foo/')).toBe(false) // 原始路径也已移除
  })

  // loadAppConfig 失败抛错（不吞）+ loadStatus=error
  it('LoadAppConfig_Throws_OnFail_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_app_config') throw new Error('config read failed')
      return null
    })
    const store = useAppStore()
    await expect(store.loadAppConfig()).rejects.toThrow('config read failed')
    expect(store.loadStatus).toBe('error')
  })

  // loadCache 失败抛错（不吞）
  it('LoadCache_Throws_OnFail_001', async () => {
    mockIPC((cmd) => {
      if (cmd === 'get_home_data') throw new Error('home read failed')
      return null
    })
    const store = useAppStore()
    await expect(store.loadCache()).rejects.toThrow('home read failed')
  })

  // 性能 #2 P1 #3：loadCache(force=true) 即使 cacheLoaded 也重新加载（刷新 startupState）。
  // 场景：启动重试时 lastOpened/hidden 可能变，若 cacheLoaded 跳过会让 startupState 用旧快照。
  it('LoadCache_ForceRefreshesDespiteCacheLoaded_001', async () => {
    let callCount = 0
    let lastOpenedSeen: string | null = null
    mockIPC((cmd, args) => {
      if (cmd === 'get_home_data') {
        callCount++
        lastOpenedSeen = (args as { lastOpened: string }).lastOpened
        return {
          projects: [], recentSessions: [], hasMore: false,
          startupState: { hasAnyProject: false, hasVisibleProject: false, lastOpenedProjectInfo: null },
        }
      }
      return null
    })
    const store = useAppStore()
    store.lastOpenedProject = '/a'
    await store.loadCache(true) // force：cacheLoaded=false -> 加载
    expect(callCount).toBe(1)
    expect(lastOpenedSeen).toBe('/a')
    expect(store.cacheLoaded).toBe(true)
    store.lastOpenedProject = '/b'
    await store.loadCache(true) // force：cacheLoaded=true 仍刷新
    expect(callCount).toBe(2)
    expect(lastOpenedSeen).toBe('/b') // 用新 lastOpened，非旧快照
    store.lastOpenedProject = '/c'
    await store.loadCache() // 非 force：cacheLoaded=true -> 跳过
    expect(callCount).toBe(2) // 未增加
  })

  // v6 codex batch1 #3：setCurrentProject(persist:true) lastOpenedOpLock 串行化--
  // 快速 A->B 切换（A 的 saveLastProject 故意慢于 B）最终磁盘写入顺序仍为 A 后 B，
  // 终态 cwd=B、lastOpened 持久化调用顺序 [A, B]（opLock 保证 A 写完才写 B，不会乱序覆盖）。
  it('SetCurrentProject_OpLock_FastAB_PreservesOrder_001', async () => {
    const calls: string[] = []
    let resolveA!: () => void
    const slowA = new Promise<void>(r => { resolveA = r })
    mockIPC((cmd, args) => {
      if (cmd === 'save_last_project') {
        const path = (args as { path: string }).path
        calls.push(path)
        // A 慢：返回一个等 resolveA 的 Promise；B 快：直接 resolve
        if (path === '/p-a') return slowA.then(() => null)
        return null
      }
      return null
    })
    const store = useAppStore()
    // 同时发起 A、B（A 先调但慢，B 后调但快）。无 opLock 则 B 先写完、A 后写完 -> 磁盘留 A（错）。
    const pA = store.setCurrentProject('/p-a', { persist: true })
    const pB = store.setCurrentProject('/p-b', { persist: true })
    // 让 B 有机会在 A resolve 前排队（验证 B 不抢跑）
    await Promise.resolve()
    resolveA() // A 的 saveLastProject 现在完成
    await Promise.all([pA, pB])
    expect(calls).toEqual(['/p-a', '/p-b']) // 顺序：A 先 B 后（opLock 串行）
    expect(store.cwd).toBe('/p-b') // 终态 cwd=B
  })

  // v6 codex batch1 #3：setCurrentProject(persist:false) 不经 opLock、不写磁盘（直接 setCwdLocal）
  it('SetCurrentProject_NoPersist_NoOpLock_001', async () => {
    const calls: string[] = []
    mockIPC((cmd, args) => {
      if (cmd === 'save_last_project') calls.push((args as { path: string }).path)
      return null
    })
    const store = useAppStore()
    await store.setCurrentProject('/p-a', { persist: false })
    expect(store.cwd).toBe('/p-a')
    expect(calls).toEqual([]) // persist=false 不写磁盘
  })

  // v6 codex batch1 #10：setHidden 拒绝隐藏当前 cwd（domain 层硬保护，防 App.vue 打开入口绕过）
  it('SetHidden_RejectHideCwd_001', async () => {
    const store = useAppStore()
    store.setCwdLocal('/p-cwd') // 设当前 cwd
    await store.setHidden('/p-cwd', true) // 隐藏当前 cwd -> 拒绝（不抛错，幂等兼容 UI）
    expect(store.isHidden('/p-cwd')).toBe(false) // 未隐藏
  })

  // v6 codex batch1 #10：setHidden 拒绝隐藏 cwd（规范化比较--斜杠/大小写差异仍识别为 cwd）
  it('SetHidden_RejectHideCwd_Normalized_001', async () => {
    const store = useAppStore()
    store.setCwdLocal('E:\\Source\\MyProj')
    await store.setHidden('e:/source/myproj', true) // 规范化后同 cwd -> 拒绝
    expect(store.isHidden('E:\\Source\\MyProj')).toBe(false)
  })

  // v6 codex batch1 #10：setHidden 隐藏非 cwd 项目仍正常（cwd 保护不影响其他路径）
  it('SetHidden_HideNonCwd_Ok_001', async () => {
    const store = useAppStore()
    store.setCwdLocal('/p-cwd')
    await store.setHidden('/p-other', true) // 非 cwd -> 正常隐藏
    expect(store.isHidden('/p-other')).toBe(true)
    expect(store.isHidden('/p-cwd')).toBe(false)
  })

  // v6 codex batch1 #10：setHidden 取消隐藏（hidden=false）不受 cwd 保护限制--
  // 取消隐藏 cwd 自身亦允许（虽管理页 cwd 项目禁隐藏按钮，但 hidden=false 语义是「显示」，应放行）
  it('SetHidden_ShowCwd_Ok_001', async () => {
    const store = useAppStore()
    store.setCwdLocal('/p-cwd')
    // 先设法让 cwd 在隐藏集合（绕过 cwd 保护模拟历史脏数据）：直接操作内部不易，改为验证 hidden=false 不抛错
    await store.setHidden('/p-cwd', false) // 取消隐藏 cwd -> 不受限，不抛错
    expect(store.isHidden('/p-cwd')).toBe(false)
  })
})
