import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import HistoricalVersionsPanel from '@/components/settings/HistoricalVersionsPanel.vue'
import { useVersionHistoryStore } from '@/stores/versionHistory'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import preparation from '../fixtures/version-history-preparation-wire.json'
import switchWire from '../fixtures/version-switch-wire.json'
import installWire from '../fixtures/version-historical-install-wire.json'

const io = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }))
const wrappers: VueWrapper[] = []
const row = { releaseId: 'a'.repeat(32), assetId: '576637999', version: '0.17.7', publishedAt: '2026-09-20T10:45:00Z',
  platform: 'windows-x86_64', availablePlatforms: ['windows-x86_64'], packageFormat: 'nsis', verification: 'awaiting-verification',
  selectAllowed: true, installReady: false, dataModes: { freshSettings: 'available', keepCurrentData: 'unavailable' }, blockedReason: null }
const selection = { selectionToken: 'b'.repeat(32), releaseId: row.releaseId, assetId: row.assetId, version: row.version,
  expiresAt: '2026-10-02T20:00:00Z', verification: 'awaiting-verification', installReady: false }
let ordinary: typeof installWire.reviews[number]
beforeEach(() => {
  setActivePinia(createPinia()); vi.resetAllMocks(); ordinary = installWire.reviews[0]
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', invoke: io.invoke } })
  io.invoke.mockImplementation(async command => {
    if (command === 'list_history') return { rows: [row], nextCursor: null, truncated: false }
    if (command === 'select_history') return selection
    if (command === 'begin_prepare_history') return preparation.ticket
    if (command === 'prepare_history') return preparation.prepared
    if (command === 'inspect_switch') return { ...switchWire.reviews[1], preparationId: preparation.ticket.transactionId }
    if (command === 'inspect_historical_install') return { ...ordinary, preparationId: preparation.ticket.transactionId, version: row.version }
    if (command === 'begin_historical_install') return installWire.ticket
    if (command === 'cancel_prepare_history') return preparation.cancelled
    throw new Error('Unexpected command')
  })
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); delete (window as any).__CC_DESK_DOCUMENT__; document.body.innerHTML = '' })
async function ready(locale = 'en') {
  const w = mount(HistoricalVersionsPanel, { props: { active: true }, attachTo: document.body,
    global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })] } })
  wrappers.push(w)
  await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
  await w.get('[data-history-select]').trigger('click'); await flushPromises()
  await w.get('[data-history-prepare]').trigger('click'); await flushPromises()
  return w
}
function button(selector: string) { return document.querySelector<HTMLButtonElement>(selector)! }
async function acknowledge() {
  const input = document.querySelector<HTMLInputElement>('[data-history-ordinary-ack]')!
  input.checked = true; input.dispatchEvent(new Event('change', { bubbles: true })); await flushPromises()
}

describe('ordinary historical installer confirmation', () => {
  // 未审核 payload 的发布者验证包可独立检查普通安装，但仍需明确风险确认。
  it.each(['en', 'zh'])('HistoryInstall_ConfirmRisks_001: %s', async locale => {
    const w = await ready(locale)
    expect(io.invoke.mock.calls.some(([command]) => command === 'inspect_historical_install')).toBe(false)
    await w.get('[data-history-ordinary-install]').trigger('click'); await flushPromises()
    expect(io.invoke).toHaveBeenCalledWith('inspect_historical_install', { transactionId: preparation.ticket.transactionId })
    const dialog = document.querySelector('[data-history-ordinary-dialog]')!
    const messages = locale === 'en' ? en : zh
    expect(dialog.textContent).toContain(messages.historyOrdinaryCompatibility)
    expect(dialog.textContent).toContain(messages.historyOrdinaryBackup)
    expect(dialog.textContent).toContain(messages.historyOrdinaryInstaller)
    expect(dialog.textContent).toContain(messages.historyOrdinarySharedCli)
    expect(button('[data-history-ordinary-begin]').disabled).toBe(true)
    await acknowledge()
    const submit = button('[data-history-ordinary-begin]'); submit.click(); submit.click(); await flushPromises()
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_historical_install')).toEqual([
      ['begin_historical_install', { transactionId: preparation.ticket.transactionId }],
    ])
    expect(io.invoke.mock.calls.some(([command]) => command === 'begin_switch')).toBe(false)
    expect(useVersionHistoryStore().transactionId).toBe(installWire.ticket.transactionId)
    expect(w.get('[data-history-status]').text()).toBe(messages.historyHandoffIssued)
    expect(w.find('[data-history-ordinary-started]').exists()).toBe(false)
  })

  // 普通安装回执未知后，不得重放或从旧审核路径取消；重新打开仍保留原归属。
  it('HistoryInstall_UnknownOwner_002', async () => {
    const w = await ready()
    await w.get('[data-history-ordinary-install]').trigger('click'); await flushPromises(); await acknowledge()
    io.invoke.mockRejectedValueOnce({ code: 'HISTORY_TASK_FAILED', details: '/private SECRET' })
    button('[data-history-ordinary-begin]').click(); await flushPromises()
    expect(w.get('[data-history-ordinary-install]').attributes('disabled')).toBeDefined()
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(w.get('[data-history-refresh]').attributes('disabled')).toBeDefined()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    await w.setProps({ active: false }); await w.setProps({ active: true }); await flushPromises()
    ordinary = installWire.reviews[1]
    await w.get('[data-history-inspect]').trigger('click'); await flushPromises()
    expect(useVersionHistoryStore().transactionId).toBe(installWire.ticket.transactionId)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_historical_install')).toHaveLength(1)
    expect(io.invoke.mock.calls.some(([command]) => ['begin_switch', 'cancel_prepare_history'].includes(command))).toBe(false)
    expect(w.text()).not.toMatch(/private|SECRET/)
  })

  // 对话框打开后导航使确认失效；没有开始安装时仍按原文档清理准备。
  it('HistoryInstall_StaleConfirmation_003', async () => {
    const w = await ready()
    await w.get('[data-history-ordinary-install]').trigger('click'); await flushPromises(); await acknowledge()
    const submit = button('[data-history-ordinary-begin]')
    await w.setProps({ active: false }); submit.click(); await flushPromises()
    expect(io.invoke.mock.calls.some(([command]) => command === 'begin_historical_install')).toBe(false)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cancel_prepare_history')).toHaveLength(1)
  })

  // 开始普通安装后，旧审核完成不能复活审核安装或取消动作。
  it('HistoryInstall_OldReviewCannotReplay_004', async () => {
    const original = io.invoke.getMockImplementation()!
    let reads = 0, finish!: (value: unknown) => void
    io.invoke.mockImplementation((command, payload) => command === 'inspect_switch' && ++reads === 1
      ? new Promise(resolve => { finish = resolve }) : original(command, payload))
    const w = await ready()
    await w.get('[data-history-ordinary-install]').trigger('click'); await flushPromises(); await acknowledge()
    io.invoke.mockRejectedValueOnce({ code: 'HISTORY_TASK_FAILED' })
    button('[data-history-ordinary-begin]').click(); await flushPromises()
    finish({ ...switchWire.reviews[3], preparationId: preparation.ticket.transactionId }); await flushPromises()
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(io.invoke.mock.calls.some(([command]) => command === 'begin_switch')).toBe(false)
  })

  // 管理器启动安装程序的证明只能展示启动与备份位置，不是安装成功。
  it('HistoryInstall_ShowBackupReceipt_005', async () => {
    const w = await ready()
    await w.get('[data-history-ordinary-install]').trigger('click'); await flushPromises(); await acknowledge()
    button('[data-history-ordinary-begin]').click(); await flushPromises()
    ordinary = installWire.reviews[2]
    await w.get('[data-history-inspect]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-ordinary-backup]').text()).toContain(ordinary.backupLocation)
    expect(w.get('[data-history-ordinary-started]').text()).toBe(en.historyOrdinaryInstallerStarted)
    expect(w.get('[data-history-ordinary-install]').attributes('disabled')).toBeDefined()
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
  })

  // 面板关闭期间到达的签发回执仍由原操作保留，不能取消或走另一条安装路径。
  it('HistoryInstall_LateIssuedOwner_006', async () => {
    const w = await ready()
    await w.get('[data-history-ordinary-install]').trigger('click'); await flushPromises(); await acknowledge()
    let finish!: (value: unknown) => void
    io.invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    button('[data-history-ordinary-begin]').click(); await flushPromises()
    await w.setProps({ active: false }); await w.setProps({ active: true }); await flushPromises()
    finish(installWire.ticket); await flushPromises()
    expect(useVersionHistoryStore().transactionId).toBe(installWire.ticket.transactionId)
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(io.invoke.mock.calls.some(([command]) => command === 'cancel_prepare_history')).toBe(false)
  })

  // 未知变更之后即使普通检查返回旧已验证状态，也不能重放该准备的安装请求。
  it('HistoryInstall_VerifiedAfterUnknown_007', async () => {
    const w = await ready()
    await w.get('[data-history-ordinary-install]').trigger('click'); await flushPromises(); await acknowledge()
    io.invoke.mockRejectedValueOnce({ code: 'HISTORY_TASK_FAILED' })
    button('[data-history-ordinary-begin]').click(); await flushPromises()
    await w.get('[data-history-inspect]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-ordinary-install]').attributes('disabled')).toBeDefined()
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_historical_install')).toHaveLength(1)
  })

  // 普通后端撤回安装动作时，发布者验证与勾选风险声明不能补出安装授权。
  it('HistoryInstall_WithheldAdmission_008', async () => {
    ordinary = { ...ordinary, allowedActions: ['refresh', 'cancel-preparation'] }
    const w = await ready()
    await w.get('[data-history-ordinary-install]').trigger('click'); await flushPromises(); await acknowledge()
    expect(button('[data-history-ordinary-begin]').disabled).toBe(true)
    expect(io.invoke.mock.calls.some(([command]) => command === 'begin_historical_install')).toBe(false)
  })

  // 现有恢复证据阻止普通安装时，固定安全提示必须区分原因并保留未知归属。
  it('HistoryInstall_ExistingRecovery_009', async () => {
    const w = await ready()
    await w.get('[data-history-ordinary-install]').trigger('click'); await flushPromises(); await acknowledge()
    io.invoke.mockRejectedValueOnce({ code: 'HISTORY_ORDINARY_EXISTING_RECOVERY', details: '/private SECRET' })
    button('[data-history-ordinary-begin]').click(); await flushPromises()
    expect(w.get('[data-history-error] p').text()).toMatch(/existing recovery evidence/i)
    expect(w.get('[data-history-inspect]').attributes('disabled')).toBeUndefined()
    expect(w.get('[data-history-ordinary-install]').attributes('disabled')).toBeDefined()
    expect(w.text()).not.toMatch(/private|SECRET/)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_historical_install')).toHaveLength(1)
  })
})
