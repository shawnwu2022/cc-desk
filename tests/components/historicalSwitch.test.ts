import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import HistoricalVersionsPanel from '@/components/settings/HistoricalVersionsPanel.vue'
import { useVersionHistoryStore } from '@/stores/versionHistory'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import preparation from '../fixtures/version-history-preparation-wire.json'
import wire from '../fixtures/version-switch-wire.json'

const io = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }))
const wrappers: VueWrapper[] = []
const row = { releaseId: 'a'.repeat(32), assetId: '576637999', version: '0.17.7', publishedAt: '2026-09-20T10:45:00Z',
  platform: 'windows-x86_64', availablePlatforms: ['windows-x86_64'], packageFormat: 'nsis', verification: 'awaiting-verification',
  selectAllowed: true, installReady: false, dataModes: { freshSettings: 'available', keepCurrentData: 'unavailable' }, blockedReason: null }
const selection = { selectionToken: 'b'.repeat(32), releaseId: row.releaseId, assetId: row.assetId, version: row.version,
  expiresAt: '2026-10-02T20:00:00Z', verification: 'awaiting-verification', installReady: false }
let review: unknown
beforeEach(() => {
  setActivePinia(createPinia()); vi.resetAllMocks(); review = wire.reviews[3]
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', invoke: io.invoke } })
  io.invoke.mockImplementation(async command => {
    if (command === 'list_history') return { rows: [row], nextCursor: null, truncated: false }
    if (command === 'select_history') return selection
    if (command === 'begin_prepare_history') return preparation.ticket
    if (command === 'prepare_history') return preparation.prepared
    if (command === 'inspect_switch') return { ...(review as object), preparationId: preparation.ticket.transactionId }
    if (command === 'begin_switch') return wire.ticket
    if (command === 'cancel_prepare_history') return preparation.cancelled
    throw new Error('Unexpected command')
  })
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); delete (window as any).__CC_DESK_DOCUMENT__; document.body.innerHTML = '' })
function render() {
  const w = mount(HistoricalVersionsPanel, { props: { active: true }, attachTo: document.body,
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  wrappers.push(w); return w
}
async function preparedPanel() {
  const w = render()
  await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
  await w.get('[data-history-select]').trigger('click'); await flushPromises()
  await w.get('[data-history-prepare]').trigger('click'); await flushPromises()
  return w
}
function dialogButton(selector: string) { return document.querySelector<HTMLButtonElement>(selector)! }

describe('ordinary historical switch composition', () => {
  // 发布者验证不能自行启用安装；后端可仅提供检查说明。
  it('HistorySwitch_BackendAdmission_001', async () => {
    review = wire.reviews[1]
    const w = await preparedPanel()
    expect(io.invoke).toHaveBeenCalledWith('inspect_switch', { preparationId: preparation.ticket.transactionId })
    expect(useVersionHistoryStore().prepared?.installReady).toBe(false)
    await w.get('[data-history-install]').trigger('click'); await flushPromises()
    expect(dialogButton('[data-history-begin]').disabled).toBe(true)
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain('package contents')
    expect(io.invoke.mock.calls.some(([command]) => command === 'begin_switch')).toBe(false)
  })
  // 明确的全新设置确认只发送有界身份，双击不能重放切换。
  it('HistorySwitch_ExplicitConfirmation_002', async () => {
    const w = await preparedPanel()
    const opener = w.get('[data-history-install]').element as HTMLButtonElement
    opener.focus(); opener.click(); await flushPromises()
    const dialog = document.querySelector('[role="dialog"]')!
    expect(dialog.textContent).toContain('0.17.7')
    expect(dialog.textContent).toContain('complete current application')
    expect(dialog.textContent).toContain('not sandbox')
    expect(document.activeElement).toBe(dialogButton('[data-history-back]'))
    expect(dialogButton('[data-history-begin]').getAttribute('data-danger')).toBe('true')
    expect(dialogButton('[data-history-begin]').classList.contains('ui-button--danger')).toBe(true)
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); await flushPromises()
    expect(document.querySelector('[role="dialog"]')).toBeNull()
    expect(document.activeElement).toBe(opener)
    expect(io.invoke.mock.calls.some(([command]) => command === 'begin_switch')).toBe(false)
    opener.click(); await flushPromises()
    const submit = dialogButton('[data-history-begin]'); submit.click(); submit.click(); await flushPromises()
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_switch')).toEqual([
      ['begin_switch', { preparationId: preparation.ticket.transactionId, dataMode: 'fresh-settings' }],
    ])
    expect(w.get('[data-history-status]').text()).toContain('handed to the version manager')
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(useVersionHistoryStore().transactionId).toBe(wire.ticket.transactionId)
  })
  // 已签发身份在后端错误、关闭面板和重新激活后仍只能刷新。
  it('HistorySwitch_IssuedOwnerRetained_003', async () => {
    const w = await preparedPanel()
    await w.get('[data-history-install]').trigger('click'); await flushPromises()
    dialogButton('[data-history-begin]').click(); await flushPromises()
    io.invoke.mockRejectedValueOnce({ code: 'HISTORY_NETWORK_UNAVAILABLE', details: '/private SECRET' })
    await w.get('[data-history-inspect]').trigger('click'); await flushPromises()
    await w.setProps({ active: false }); await w.setProps({ active: true }); await flushPromises()
    expect(useVersionHistoryStore().transactionId).toBe(wire.ticket.transactionId)
    review = wire.reviews[3] // A stale/contradictory response must not remove issued ownership.
    await w.get('[data-history-inspect]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(w.text()).not.toMatch(/SECRET|private/)
    w.unmount()
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cancel_prepare_history')).toHaveLength(0)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_switch')).toHaveLength(1)
  })
  // 切换命令回执丢失后，必须先进行新的检查才能恢复任何操作。
  it('HistorySwitch_UnknownThenInspect_004', async () => {
    const w = await preparedPanel()
    await w.get('[data-history-install]').trigger('click'); await flushPromises()
    io.invoke.mockRejectedValueOnce({ code: 'HISTORY_NETWORK_UNAVAILABLE' })
    dialogButton('[data-history-begin]').click(); await flushPromises()
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(w.get('[data-history-status]').text()).toContain('unknown')
    review = wire.reviews[4]
    await w.get('[data-history-inspect]').trigger('click'); await flushPromises()
    expect(useVersionHistoryStore().transactionId).toBe(wire.ticket.transactionId)
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_switch')).toHaveLength(1)
  })
  // 后端移除准入后，旧对话框不能继续提交。
  it('HistorySwitch_StaleReview_005', async () => {
    const w = await preparedPanel()
    await w.get('[data-history-install]').trigger('click'); await flushPromises()
    const submit = dialogButton('[data-history-begin]')
    await w.setProps({ active: false }); submit.click(); await flushPromises()
    expect(document.querySelector('[role="dialog"]')).toBeNull()
    expect(io.invoke.mock.calls.some(([command]) => command === 'begin_switch')).toBe(false)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cancel_prepare_history')).toHaveLength(1)
  })
  // 取消错误不能触发自动重试，新的检查可发现管理器已接管。
  it('HistorySwitch_CancelUnknown_006', async () => {
    const w = await preparedPanel()
    io.invoke.mockResolvedValueOnce({ ...wire.reviews[3], preparationId: preparation.ticket.transactionId }).mockRejectedValueOnce({ code: 'HISTORY_NETWORK_UNAVAILABLE' })
    await w.get('[data-history-cancel]').trigger('click'); await flushPromises()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    review = wire.reviews[4]
    await w.get('[data-history-inspect]').trigger('click'); await flushPromises()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cancel_prepare_history')).toHaveLength(1)
  })
  // 完整验证状态可明确不提供任何动作，前端不能根据阶段补齐。
  it('HistorySwitch_WithheldActions_007', async () => {
    review = { ...wire.reviews[3], allowedActions: [] }
    const w = await preparedPanel()
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(w.get('[data-history-inspect]').attributes('disabled')).toBeDefined()
  })
  // 切换前发起的旧检查不得在切换结果未知后重新启用操作。
  it('HistorySwitch_OldInspectCannotUnlock_008', async () => {
    const original = io.invoke.getMockImplementation()!
    let reads = 0, finishOld!: (value: unknown) => void
    io.invoke.mockImplementation((command, payload) => {
      if (command === 'inspect_switch' && ++reads === 1) return new Promise(resolve => { finishOld = resolve })
      return original(command, payload)
    })
    const w = await preparedPanel()
    await w.get('[data-history-install]').trigger('click'); await flushPromises()
    io.invoke.mockRejectedValueOnce({ code: 'HISTORY_TASK_FAILED' })
    dialogButton('[data-history-begin]').click(); await flushPromises()
    finishOld({ ...wire.reviews[3], preparationId: preparation.ticket.transactionId }); await flushPromises()
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(w.get('[data-history-status]').text()).toContain('unknown')
  })
  // 切换在途时关闭面板，迟到签发回执仍保留，且不能触发准备清理。
  it('HistorySwitch_PendingUnmount_009', async () => {
    const w = await preparedPanel()
    await w.get('[data-history-install]').trigger('click'); await flushPromises()
    let finish!: (value: unknown) => void
    io.invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    dialogButton('[data-history-begin]').click(); await flushPromises()
    await w.setProps({ active: false }); await w.setProps({ active: true }); await flushPromises()
    finish(wire.ticket); await flushPromises()
    expect(useVersionHistoryStore().transactionId).toBe(wire.ticket.transactionId)
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(io.invoke.mock.calls.some(([command]) => command === 'cancel_prepare_history')).toBe(false)
  })
  // 只有带原事务UUID的明确中止回执允许用户重新选择准备。
  it('HistorySwitch_ExplicitAbort_010', async () => {
    const w = await preparedPanel()
    await w.get('[data-history-install]').trigger('click'); await flushPromises()
    dialogButton('[data-history-begin]').click(); await flushPromises()
    review = wire.reviews.find(value => value.phase === 'aborted')
    await w.get('[data-history-inspect]').trigger('click'); await flushPromises()
    expect(useVersionHistoryStore().transactionId).toBe(wire.ticket.transactionId)
    expect(w.find('[data-history-prepare-again]').exists()).toBe(true)
    await w.get('[data-history-prepare-again]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-refresh]').attributes('disabled')).toBeUndefined()
    expect(w.find('[data-history-prepare]').exists()).toBe(false)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_prepare_history')).toHaveLength(1)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_switch')).toHaveLength(1)
  })
  // 旧文档的清理检查若发现已签发事务，就必须保留且停止取消。
  it('HistorySwitch_CleanupFindsHandoff_011', async () => {
    const w = await preparedPanel()
    review = wire.reviews[4]
    await w.setProps({ active: false }); await flushPromises()
    await w.setProps({ active: true }); await flushPromises()
    expect(useVersionHistoryStore().transactionId).toBe(wire.ticket.transactionId)
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(io.invoke.mock.calls.some(([command]) => command === 'cancel_prepare_history')).toBe(false)
  })
  // 不匹配的准备身份不能把验证结果转化为安装准入。
  it('HistorySwitch_WrongOwner_012', async () => {
    const original = io.invoke.getMockImplementation()!
    io.invoke.mockImplementation((command, payload) => command === 'inspect_switch'
      ? { ...wire.reviews[3], preparationId: 'f'.repeat(32) } : original(command, payload))
    const w = await preparedPanel()
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(w.get('[data-history-status]').text()).toContain('unknown')
  })

  // 清理检查已确认准备取消时，不应发第二次取消或保留不可用占用。
  it('HistorySwitch_CleanupAlreadyCancelled_013', async () => {
    const w = await preparedPanel()
    review = wire.reviews.find(value => value.phase === 'cancelled')
    await w.setProps({ active: false }); await flushPromises()
    await w.setProps({ active: true }); await flushPromises()
    expect(w.get('[data-history-refresh]').attributes('disabled')).toBeUndefined()
    expect(io.invoke.mock.calls.some(([command]) => command === 'cancel_prepare_history')).toBe(false)
  })

  // 矛盾的交接或繁忙回执在导航清理中也不能发送取消命令。
  it.each(['HANDOFF_ISSUED', 'PREPARATION_BUSY'])('HistorySwitch_InvalidCleanup_014: %s', async blockReason => {
    const w = await preparedPanel()
    review = { ...wire.reviews[3], allowedActions: ['refresh', 'review', 'cancel-preparation'], blockReason }
    await w.setProps({ active: false }); await flushPromises()
    expect(io.invoke.mock.calls.some(([command]) => command === 'cancel_prepare_history')).toBe(false)
    await w.setProps({ active: true }); await flushPromises()
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
  })

})
