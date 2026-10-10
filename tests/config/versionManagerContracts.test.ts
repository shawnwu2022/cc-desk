import { readFileSync, existsSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { parseManagerStatus } from '@/manager/contracts'
import ordinaryWire from '../fixtures/version-manager-ordinary-wire.json'
import { createVersionManagerClient } from '@/manager/api'
import wire from '../fixtures/version-manager-wire.json'

afterEach(() => { delete window.__CC_DESK_VERSION_MANAGER__ })

describe('version manager authenticated client', () => {
  it('ManagerClient_OrdinaryFence_008', async () => {
    for (const next of [
      { ...wire.recoveryRequired, transactionId: ordinaryWire.installerHandedOff.transactionId, sourceVersion: '0.18.4', targetVersion: '0.18.3', generation: '3' },
      { ...ordinaryWire.recoveryRequired },
      { ...ordinaryWire.recoveryRequired, ordinaryInstall: { ...ordinaryWire.recoveryRequired.ordinaryInstall, backupLocation: null } },
      { ...ordinaryWire.installerHandedOff, generation: '3', ordinaryInstall: { ...ordinaryWire.installerHandedOff.ordinaryInstall, backupLocation: 'C:\\different' } },
    ]) {
      const invoke = vi.fn().mockResolvedValueOnce(ordinaryWire.installerHandedOff).mockResolvedValueOnce(next)
      window.__CC_DESK_VERSION_MANAGER__ = { invoke }
      const client = createVersionManagerClient()!
      await client.inspect()
      await expect(client.inspect()).rejects.toThrow('MANAGER_DOCUMENT_CHANGED')
      expect(client.isCurrent()).toBe(false)
    }
  })

  // 大整数 generation 原样发回，事务与路径只能由后端文档确定。
  it('ManagerClient_ExactPayload_001', async () => {
    const status = { ...wire.installedUnconfirmed, generation: '9007199254740993' }
    const invoke = vi.fn().mockResolvedValueOnce(status).mockResolvedValueOnce({ ...wire.historicalActive, generation: '9007199254740994' })
    window.__CC_DESK_VERSION_MANAGER__ = { invoke }
    const client = createVersionManagerClient()!
    const snapshot = await client.inspect()
    await client.act('confirm-historical-version', snapshot)
    expect(invoke.mock.calls).toEqual([
      ['inspect_version_switch', {}], ['confirm_historical_version', { expectedGeneration: '9007199254740993' }],
    ])
  })

  // 文档换代期间的完成不能发布，且不能借新文档重发动作。
  it('ManagerClient_ReplacedBridge_002', async () => {
    let finish!: (value: unknown) => void
    const invoke = vi.fn().mockImplementation(() => new Promise(resolve => { finish = resolve }))
    window.__CC_DESK_VERSION_MANAGER__ = { invoke }
    const client = createVersionManagerClient()!
    const request = client.inspect()
    const replacement = vi.fn()
    window.__CC_DESK_VERSION_MANAGER__ = { invoke: replacement }
    finish(wire.installedUnconfirmed)
    await expect(request).rejects.toThrow('MANAGER_DOCUMENT_CHANGED')
    expect(client.isCurrent()).toBe(false)
    expect(replacement).not.toHaveBeenCalled()
  })

  // 后端快照倒退和事务/版本身份改变不能覆盖当前事务。
  it('ManagerClient_OwnerFence_003', async () => {
    for (const delta of [{ generation: '16' }, { transactionId: '22222222-2222-4222-8222-222222222222' },
      { sourceVersion: '0.19.0' }, { targetVersion: '0.17.6' }]) {
      const invoke = vi.fn().mockResolvedValueOnce(wire.installedUnconfirmed).mockResolvedValueOnce({ ...wire.installedUnconfirmed, ...delta })
      window.__CC_DESK_VERSION_MANAGER__ = { invoke }
      const client = createVersionManagerClient()!
      await client.inspect()
      await expect(client.inspect()).rejects.toThrow()
    }
  })

  // 未知提交禁止同一 generation 重放；重新挂载也只检查状态。
  it('ManagerClient_NoUnknownReplay_004', async () => {
    const invoke = vi.fn().mockResolvedValueOnce(wire.installedUnconfirmed).mockRejectedValueOnce({ code: 'UNKNOWN', details: 'SECRET' })
    window.__CC_DESK_VERSION_MANAGER__ = { invoke }
    const client = createVersionManagerClient()!
    const original = await client.inspect()
    await expect(client.act('return-to-previous', original)).rejects.toBeDefined()
    invoke.mockResolvedValueOnce(wire.installedUnconfirmed)
    const remounted = createVersionManagerClient()!
    const unchanged = await remounted.inspect()
    expect(remounted.canAct('return-to-previous', unchanged)).toBe(false)
    await expect(remounted.act('return-to-previous', unchanged)).rejects.toThrow('MANAGER_ACTION_UNAVAILABLE')
    expect(invoke.mock.calls.filter(([command]) => command === 'restore_previous_version')).toHaveLength(1)
    invoke.mockResolvedValueOnce({ ...wire.recoveryRequired, generation: '18' })
    expect(remounted.canAct('return-to-previous', await remounted.inspect())).toBe(true)
  })

  // 只有后端当前快照明确提供的动作才可以提交。
  it('ManagerClient_OfferedActions_005', async () => {
    const invoke = vi.fn().mockResolvedValue({ ...wire.installedUnconfirmed, allowedActions: ['refresh'] })
    window.__CC_DESK_VERSION_MANAGER__ = { invoke }
    const client = createVersionManagerClient()!
    const status = await client.inspect()
    await expect(client.act('confirm-historical-version', status)).rejects.toThrow('MANAGER_ACTION_UNAVAILABLE')
    expect(invoke).toHaveBeenCalledTimes(1)
  })

  // 首次文档就绪前允许只读重查；成功准入后的 FORBIDDEN 永久撤销该桥。
  it('ManagerClient_AdmissionRevoked_006', async () => {
    const invoke = vi.fn().mockRejectedValueOnce({ code: 'FORBIDDEN' }).mockResolvedValueOnce(wire.installedUnconfirmed)
      .mockRejectedValueOnce({ code: 'FORBIDDEN' })
    window.__CC_DESK_VERSION_MANAGER__ = { invoke }
    const client = createVersionManagerClient()!
    await expect(client.inspect()).rejects.toEqual({ code: 'FORBIDDEN' })
    expect(client.isCurrent()).toBe(true)
    await client.inspect()
    await expect(client.inspect()).rejects.toThrow('MANAGER_DOCUMENT_CHANGED')
    expect(createVersionManagerClient()!.isCurrent()).toBe(false)
  })

  // 同代数的迟到只读快照不能覆盖后发检查撤销的动作。
  it('ManagerClient_ReadOrder_007', async () => {
    let finish!: (value: unknown) => void
    const invoke = vi.fn().mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
      .mockResolvedValueOnce({ ...wire.installedUnconfirmed, allowedActions: ['refresh'] })
    window.__CC_DESK_VERSION_MANAGER__ = { invoke }
    const client = createVersionManagerClient()!
    const first = client.inspect()
    await client.inspect()
    finish(wire.installedUnconfirmed)
    await expect(first).rejects.toThrow('MANAGER_STALE_RESPONSE')
  })
})

describe('version manager Rust wire contract', () => {
  // 读取与 Rust 序列化测试共用的样本，保证安装未检查与历史启用保持不同状态。
  it('ManagerWire_RustSnapshots_001', () => {
    for (const value of Object.values(wire)) expect(parseManagerStatus(value)).toEqual(value)
    expect(parseManagerStatus(wire.installedUnconfirmed).phase).not.toBe(parseManagerStatus(wire.historicalActive).phase)
  })

  it('ManagerWire_OrdinarySnapshots_005', () => {
    for (const value of Object.values(ordinaryWire)) {
      const parsed = parseManagerStatus(value)
      expect(parsed).toEqual(value)
      expect(Object.isFrozen(parsed.ordinaryInstall)).toBe(true)
    }
    expect(parseManagerStatus(wire.installing)).not.toHaveProperty('ordinaryInstall')
  })

  it('ManagerWire_OrdinaryProofAndActions_006', () => {
    const base = ordinaryWire.installerHandedOff
    for (const ordinaryInstall of [null, {}, { ...base.ordinaryInstall, extra: true },
      { ...base.ordinaryInstall, contextPolicy: 'fresh-settings' },
      { ...base.ordinaryInstall, backupLocation: null },
      { ...base.ordinaryInstall, backupLocation: '' },
      { ...base.ordinaryInstall, backupLocation: '/tmp/backup' },
      { ...base.ordinaryInstall, backupLocation: 'C:\\backup\nsecret' },
      { ...base.ordinaryInstall, installerHandedOff: 'true' },
    ]) expect(() => parseManagerStatus({ ...base, ordinaryInstall })).toThrow('MANAGER_INVALID_RESPONSE')
    for (const phase of ['installed-unconfirmed', 'historical-active', 'restored', 'returning', 'pre-context-aborted', 'preparing']) {
      expect(() => parseManagerStatus({ ...base, phase })).toThrow('MANAGER_INVALID_RESPONSE')
    }
    expect(() => parseManagerStatus({ ...ordinaryWire.recoveryRequired, allowedActions: ['refresh', 'return-to-previous'] })).toThrow('MANAGER_INVALID_RESPONSE')
    expect(() => parseManagerStatus({ ...base, phase: 'installed-unconfirmed', allowedActions: ['refresh', 'confirm-historical-version'] })).toThrow('MANAGER_INVALID_RESPONSE')
  })

  // 大于 JS 安全整数的 generation 必须保留精确字符串。
  it('ManagerWire_ExactGeneration_002', () => {
    expect(parseManagerStatus({ ...wire.installing, generation: '18446744073709551615' }).generation).toBe('18446744073709551615')
    for (const generation of [17, '', '017', '-1', '1e2', '18446744073709551616']) {
      expect(() => parseManagerStatus({ ...wire.installing, generation })).toThrow('MANAGER_INVALID_RESPONSE')
    }
  })

  // 未知状态、权限、路径字段和非规范身份不能进入界面。
  it('ManagerWire_RejectUnknownData_003', () => {
    for (const delta of [
      { phase: 'completed' }, { blockedReason: '/secret/config' }, { path: 'C:\\secret' },
      { transactionId: '00000000-0000-0000-0000-000000000000' }, { transactionId: '11111111111141118111111111111111' },
      { sourceVersion: '0.18.0 <script>' }, { targetVersion: '4294967296.0.0' },
      { allowedActions: ['refresh', 'force-kill'] }, { allowedActions: ['return-to-previous'] },
      { allowedActions: ['refresh', 'refresh'] }, { allowedActions: ['refresh', 'confirm-historical-version'] },
    ]) expect(() => parseManagerStatus({ ...wire.installing, ...delta })).toThrow('MANAGER_INVALID_RESPONSE')
  })

  // 状态只限定允许动作的上界，不能自动补上后端没有提供的操作。
  it('ManagerWire_KeepActionSubset_004', () => {
    const value = { ...wire.installedUnconfirmed, allowedActions: ['refresh'] }
    expect(parseManagerStatus(value).allowedActions).toEqual(['refresh'])
    const parsed = parseManagerStatus(value)
    value.allowedActions.push('confirm-historical-version')
    expect(parsed.allowedActions).toEqual(['refresh'])
  })
})

describe('dedicated version manager entry', () => {
  // 管理器必须由独立 HTML 入口加载，不能启动普通 App。
  it('ManagerEntry_DedicatedDocument_001', () => {
    expect(existsSync('version-manager.html')).toBe(true)
    const html = readFileSync('version-manager.html', 'utf8')
    expect(html).toContain('/src/manager/main.ts')
    expect(html).not.toContain('/src/main.ts')
    expect(readFileSync('vite.config.ts', 'utf8')).toContain('version-manager.html')
  })

  // 所有传递导入只允许独立管理器和无状态展示控件。
  it('ManagerEntry_NoRuntimeImports_002', () => {
    const pending = [resolve('src/manager/main.ts')]
    const seen = new Set<string>()
    while (pending.length) {
      const file = pending.pop()!
      if (seen.has(file)) continue
      seen.add(file)
      expect(file).not.toMatch(/[/\\](?:stores|terminal|api)[/\\]|[/\\]App\.vue$/)
      const source = readFileSync(file, 'utf8')
      expect(source).not.toMatch(/@tauri-apps|createPinia|__CC_DESK_DOCUMENT__|__TAURI_INTERNALS__/)
      for (const match of source.matchAll(/(?:from\s+|import\s*)['"]([^'"]+)['"]/g)) {
        const name = match[1]
        if (!name.startsWith('.') && !name.startsWith('@/')) continue
        const base = name.startsWith('@/') ? resolve('src', name.slice(2)) : resolve(dirname(file), name)
        const target = [base, `${base}.ts`, `${base}.vue`].find(existsSync)
        expect(target, `Missing manager dependency: ${name}`).toBeDefined()
        if (target && !target.endsWith('.css')) pending.push(target)
      }
    }
  })
})
