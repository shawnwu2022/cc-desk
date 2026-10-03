import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import UpdateSection from '@/components/settings/sections/UpdateSection.vue'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import wire from '../fixtures/version-history-preparation-wire.json'
import switchWire from '../fixtures/version-switch-wire.json'

const io = vi.hoisted(() => ({ invoke: vi.fn(), check: vi.fn(), relaunch: vi.fn() }))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({}) }))
vi.mock('@/api/tauri', async original => ({ ...await original<object>(), check: io.check, relaunch: io.relaunch }))
const wrappers: VueWrapper[] = []
const row = { releaseId: 'a'.repeat(32), assetId: '576637999', version: '0.17.7', publishedAt: '2026-09-20T10:45:00Z',
  platform: 'windows-x86_64', availablePlatforms: ['windows-x86_64'], packageFormat: 'nsis', verification: 'awaiting-verification',
  selectAllowed: true, installReady: false, dataModes: { freshSettings: 'available', keepCurrentData: 'unavailable' }, blockedReason: null }
const selection = { selectionToken: 'b'.repeat(32), releaseId: row.releaseId, assetId: row.assetId, version: row.version,
  expiresAt: '2026-10-02T20:00:00Z', verification: 'awaiting-verification', installReady: false }
beforeEach(() => {
  setActivePinia(createPinia()); vi.clearAllMocks()
  Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'instance', invoke: io.invoke } })
  io.invoke.mockImplementation(async command => {
    if (command === 'inspect_switch') return { ...switchWire.reviews[1], preparationId: wire.ticket.transactionId }
    if (command === 'list_history') return { rows: [row], nextCursor: null, truncated: false }
    if (command === 'select_history') return selection
    if (command === 'begin_prepare_history') return wire.ticket
    if (command === 'prepare_history') return wire.prepared
    if (command === 'cancel_prepare_history') return wire.cancelled
    throw new Error('Unexpected command')
  })
})
afterEach(() => { wrappers.splice(0).forEach(w => w.unmount()); delete (window as any).__CC_DESK_DOCUMENT__; document.body.innerHTML = '' })
function render() {
  const w = mount(UpdateSection, { props: { active: true }, attachTo: document.body,
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en, zh } })] } })
  wrappers.push(w); return w
}

describe('historical versions through the real settings and document client', () => {
  // 设置页必须通过真实文档桥获取列表，再明确选择和验证；不调用普通更新安装。
  it('HistoryUI_SelectPrepare_001', async () => {
    const w = render()
    expect(w.find('[data-history-panel]').exists()).toBe(true)
    await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    expect(io.invoke).toHaveBeenCalledWith('list_history', { cursor: null })
    await w.get('[data-history-select]').trigger('click'); await flushPromises()
    expect(io.invoke).toHaveBeenCalledWith('select_history', { releaseId: row.releaseId, assetId: row.assetId })
    await w.get('[data-history-prepare]').trigger('click'); await flushPromises()
    expect(io.invoke).toHaveBeenCalledWith('begin_prepare_history', { selectionToken: selection.selectionToken })
    expect(io.invoke).toHaveBeenCalledWith('prepare_history', { transactionId: wire.ticket.transactionId })
    expect(w.get('[data-history-status]').text()).toMatch(/publisher.*verified/i)
    expect(w.get('[data-history-install]').attributes('disabled')).toBeUndefined()
    expect(w.get('[data-update-install]').attributes('disabled')).toBeDefined()
    expect(io.check).not.toHaveBeenCalled(); expect(io.relaunch).not.toHaveBeenCalled()
  })
  // 验证失败后的自动清理不能把签名错误隐藏成普通取消。
  it('HistoryUI_VerificationFailure_002', async () => {
    io.invoke.mockImplementation(async command => {
      if (command === 'inspect_switch') return { ...switchWire.reviews[1], preparationId: wire.ticket.transactionId }
    if (command === 'list_history') return { rows: [row], nextCursor: null, truncated: false }
      if (command === 'select_history') return selection
      if (command === 'begin_prepare_history') return wire.ticket
      if (command === 'prepare_history') throw { code: 'HISTORY_SIGNATURE_INVALID', details: '/private TOKEN=secret' }
      if (command === 'cancel_prepare_history') return wire.cancelled
    })
    const w = render(); await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await w.get('[data-history-select]').trigger('click'); await flushPromises()
    await w.get('[data-history-prepare]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-error]').text()).toContain('failed verification')
    expect(w.text()).not.toMatch(/TOKEN|private|secret/)
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
  })
  // 分页、不可选行和筛选只展示后端提供的有界目录。
  it('HistoryUI_PaginationBlocked_003', async () => {
    const cursor = 'c'.repeat(32)
    const unavailable = { ...row, releaseId: 'd'.repeat(32), version: '0.17.6', selectAllowed: false,
      assetId: null, packageFormat: null, dataModes: { freshSettings: 'unavailable', keepCurrentData: 'unavailable' }, blockedReason: 'SIGNATURE_MISSING' }
    io.invoke.mockImplementation(async (command, payload) => {
      if (command === 'inspect_switch') return { ...switchWire.reviews[1], preparationId: wire.ticket.transactionId }
    if (command === 'list_history') return payload.cursor === null ? { rows: [row], nextCursor: cursor, truncated: false }
        : { rows: [unavailable], nextCursor: null, truncated: true }
    })
    const w = render(); await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await w.get('[data-history-more]').trigger('click'); await flushPromises()
    expect(io.invoke).toHaveBeenLastCalledWith('list_history', { cursor })
    expect(w.findAll('[data-history-row]')).toHaveLength(2)
    expect(w.findAll('[data-history-select]')[1].attributes('disabled')).toBeDefined()
    expect(w.get('[data-history-truncated]').text()).toContain('may not include')
    await w.get('[data-history-filter]').setValue('0.17.6')
    expect(w.findAll('[data-history-row]')).toHaveLength(1)
    expect(w.get('[data-history-row]').text()).toContain('signature is missing')
  })
  // 离开面板先于预约回执时，回执必须在原文档检查后取消，不得开始下载。
  it('HistoryUI_CancelBeforeTicket_004', async () => {
    let finish!: (value: unknown) => void
    io.invoke.mockImplementation(async command => {
      if (command === 'inspect_switch') return { ...switchWire.reviews[1], preparationId: wire.ticket.transactionId }
    if (command === 'list_history') return { rows: [row], nextCursor: null, truncated: false }
      if (command === 'select_history') return selection
      if (command === 'begin_prepare_history') return new Promise(resolve => { finish = resolve })
      if (command === 'cancel_prepare_history') return wire.cancelled
    })
    const w = render(); await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await w.get('[data-history-select]').trigger('click'); await flushPromises()
    await w.get('[data-history-prepare]').trigger('click'); await flushPromises()
    await w.setProps({ active: false }); finish(wire.ticket); await flushPromises()
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_prepare_history')).toHaveLength(1)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'prepare_history')).toHaveLength(0)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cancel_prepare_history')).toHaveLength(1)
    expect(w.get('[data-history-status]').text()).toContain('cancelled')
  })
  // 重复点击和下载中的迟到成功不能越过取消，也不能在重新挂载后重放预约。
  it('HistoryUI_RemountCancelRace_005', async () => {
    let finish!: (value: unknown) => void
    io.invoke.mockImplementation(async command => {
      if (command === 'inspect_switch') return { ...switchWire.reviews[1], preparationId: wire.ticket.transactionId }
    if (command === 'list_history') return { rows: [row], nextCursor: null, truncated: false }
      if (command === 'select_history') return selection
      if (command === 'begin_prepare_history') return wire.ticket
      if (command === 'prepare_history') return new Promise(resolve => { finish = resolve })
      if (command === 'cancel_prepare_history') return wire.cancelled
    })
    const w = render(); await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await w.get('[data-history-select]').trigger('click'); await flushPromises()
    await w.get('[data-history-prepare]').trigger('click'); await flushPromises()
    w.unmount(); const remounted = render(); await flushPromises()
    expect(remounted.get('[data-history-refresh]').attributes('disabled')).toBeDefined()
    finish(wire.prepared); await flushPromises()
    expect(remounted.text()).not.toContain('Publisher signature, SHA256 and size verified')
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_prepare_history')).toHaveLength(1)
    expect(io.invoke.mock.calls.filter(([command]) => command === 'cancel_prepare_history')).toHaveLength(1)
    expect(remounted.get('[data-history-refresh]').attributes('disabled')).toBeUndefined()
  })
  // 导航后较旧的列表和版本选择不能覆盖重新激活的界面。
  it('HistoryUI_StaleSelection_006', async () => {
    let finish!: (value: unknown) => void
    io.invoke.mockImplementation(async command => {
      if (command === 'inspect_switch') return { ...switchWire.reviews[1], preparationId: wire.ticket.transactionId }
    if (command === 'list_history') return { rows: [row], nextCursor: null, truncated: false }
      if (command === 'select_history') return new Promise(resolve => { finish = resolve })
    })
    const w = render(); await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await w.get('[data-history-select]').trigger('click'); await flushPromises()
    await w.setProps({ active: false }); await w.setProps({ active: true })
    finish(selection); await flushPromises()
    expect(w.find('[data-history-selected]').exists()).toBe(false)
    expect(w.find('[data-history-prepare]').exists()).toBe(false)
  })
  // 取消失败必须保留占用状态，新的检查允许重试后也不得创建第二个准备事务。
  it('HistoryUI_CancelFailure_007', async () => {
    const w = render(); await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await w.get('[data-history-select]').trigger('click'); await flushPromises()
    await w.get('[data-history-prepare]').trigger('click'); await flushPromises()
    io.invoke.mockRejectedValueOnce({ code: 'HISTORY_NETWORK_UNAVAILABLE', details: 'SECRET' })
    await w.get('[data-history-cancel]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-error]').text()).toContain('Could not confirm cancellation')
    expect(w.get('[data-history-refresh]').attributes('disabled')).toBeDefined()
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    await w.get('[data-history-inspect]').trigger('click'); await flushPromises()
    await w.get('[data-history-cancel]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-status]').text()).toContain('cancelled')
    expect(w.get('[data-history-refresh]').attributes('disabled')).toBeUndefined()
    expect(w.text()).not.toContain('SECRET')
  })
  // 文档桥替换必须拒绝旧选择的回执，并显示可恢复的窗口错误。
  it('HistoryUI_DocumentReplacement_008', async () => {
    let finish!: (value: unknown) => void
    io.invoke.mockImplementation(async command => {
      if (command === 'inspect_switch') return { ...switchWire.reviews[1], preparationId: wire.ticket.transactionId }
    if (command === 'list_history') return { rows: [row], nextCursor: null, truncated: false }
      if (command === 'select_history') return new Promise(resolve => { finish = resolve })
    })
    const w = render(); await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await w.get('[data-history-select]').trigger('click'); await flushPromises()
    Object.defineProperty(window, '__CC_DESK_DOCUMENT__', { configurable: true, value: { instanceId: 'replacement', invoke: vi.fn() } })
    finish(selection); await flushPromises()
    expect(w.find('[data-history-prepare]').exists()).toBe(false)
    expect(w.get('[data-history-error]').text()).toContain('Reopen CC Desk')
  })

  // 同一JS桥的后端文档被撤销时，取消失败不能建议必定无效的重试。
  it('HistoryUI_RevokedCancellation_009', async () => {
    const w = render(); await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await w.get('[data-history-select]').trigger('click'); await flushPromises()
    const prepare = w.get('[data-history-prepare]').element as HTMLButtonElement
    prepare.click(); prepare.click(); await flushPromises()
    expect(io.invoke.mock.calls.filter(([command]) => command === 'begin_prepare_history')).toHaveLength(1)
    io.invoke.mockRejectedValueOnce({ code: 'FORBIDDEN' })
    await w.get('[data-history-cancel]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-error]').text()).toContain('Reopen CC Desk')
    expect(w.find('[data-history-cancel]').exists()).toBe(false)
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
  })

  // 验证标签仅属于精确选择的发行版；同版本其他资产、取消中与后续选择仍待验证。
  it('HistoryUI_RowVerificationOwner_010', async () => {
    const other = { ...row, releaseId: 'c'.repeat(32), assetId: '576638000' }
    let finishCancel!: (value: unknown) => void
    io.invoke.mockImplementation(async (command, payload) => {
      if (command === 'inspect_switch') return { ...switchWire.reviews[1], preparationId: wire.ticket.transactionId }
    if (command === 'list_history') return { rows: [row, other], nextCursor: null, truncated: false }
      if (command === 'select_history') return { ...selection, releaseId: payload.releaseId, assetId: payload.assetId }
      if (command === 'begin_prepare_history') return wire.ticket
      if (command === 'prepare_history') return wire.prepared
      if (command === 'cancel_prepare_history') return new Promise(resolve => { finishCancel = resolve })
    })
    const w = render(); await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await w.findAll('[data-history-select]')[0].trigger('click'); await flushPromises()
    await w.get('[data-history-prepare]').trigger('click'); await flushPromises()
    expect(w.findAll('[data-history-row]')[0].get('.history-reason').text()).toBe('Publisher verified')
    expect(w.findAll('[data-history-row]')[1].get('.history-reason').text()).toBe('Publisher verification pending')
    expect(w.get('[data-history-install]').attributes('disabled')).toBeUndefined()
    await w.get('[data-history-cancel]').trigger('click'); await flushPromises()
    expect(w.findAll('[data-history-row]').map(row => row.get('.history-reason').text())).toEqual(['Publisher verification pending', 'Publisher verification pending'])
    finishCancel(wire.cancelled); await flushPromises()
    await w.findAll('[data-history-select]')[1].trigger('click'); await flushPromises()
    expect(w.findAll('[data-history-select]')[1].attributes('aria-pressed')).toBe('true')
    expect(w.findAll('[data-history-row]').map(row => row.get('.history-reason').text())).toEqual(['Publisher verification pending', 'Publisher verification pending'])
  })
  // 重新激活必须清除已验证标签；旧下载在新owner出现后完成也不能恢复该标签。
  it('HistoryUI_RowOwnerInvalidation_011', async () => {
    const w = render(); await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    await w.get('[data-history-select]').trigger('click'); await flushPromises()
    await w.get('[data-history-prepare]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-row] .history-reason').text()).toBe('Publisher verified')
    await w.setProps({ active: false }); await flushPromises(); await w.setProps({ active: true }); await flushPromises()
    await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-row] .history-reason').text()).toBe('Publisher verification pending')
    let finish!: (value: unknown) => void
    io.invoke.mockImplementationOnce(async () => selection)
    await w.get('[data-history-select]').trigger('click'); await flushPromises()
    io.invoke.mockImplementationOnce(async () => wire.ticket).mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    await w.get('[data-history-prepare]').trigger('click'); await flushPromises()
    await w.setProps({ active: false }); await w.setProps({ active: true }); await flushPromises()
    finish(wire.prepared); await flushPromises()
    await w.get('[data-history-refresh]').trigger('click'); await flushPromises()
    expect(w.get('[data-history-row] .history-reason').text()).toBe('Publisher verification pending')
    expect(w.find('[data-history-selected]').exists()).toBe(false)
    expect(w.get('[data-history-install]').attributes('disabled')).toBeDefined()
  })

})
