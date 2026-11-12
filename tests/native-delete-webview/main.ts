import { createApp, computed, h, nextTick, ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import ArchivedSessionsDrawer from '@/components/sessions/ArchivedSessionsDrawer.vue'
import SessionList from '@/components/sessions/SessionList.vue'
import NativeSessionDeletePanel from '@/components/sessions/NativeSessionDeletePanel.vue'
import { useNativeHistoryStore, type NativeHistoryContext } from '@/stores/nativeHistory'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useNativeSessionDeletesStore } from '@/stores/nativeSessionDeletes'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useProjectsStateStore } from '@/stores/projectsState'
import { useSessionStore } from '@/stores/session'
import { useCliSupportStore } from '@/stores/cliSupport'
import { createNativeCliAdapter } from '@/session/adapters/nativeCliAdapter'
import { createNativeSessionDeleteClient } from '@/api/tauri'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'
import '@/styles/global.css'

const host = window as any

const frame = () => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())))
async function until(test: () => boolean, code: string, limit = 10_000) { const end = performance.now() + limit; while (!test()) { if (performance.now() > end) throw Error(code); await new Promise(r => setTimeout(r, 20)) } await frame() }
function element(selector: string): HTMLElement { const value = document.querySelector<HTMLElement>(selector); if (!value) throw Error('ELEMENT_MISSING'); return value }
const hasText = (selector: string, text: string) => document.querySelector(selector)?.textContent?.includes(text) === true
async function click(selector: string) { element(selector).click(); await nextTick(); await frame() }
async function ack(selector: string) { const input = element(selector) as HTMLInputElement; input.checked = true; input.dispatchEvent(new Event('change', { bubbles: true })); await nextTick(); await frame() }
async function expectCode(task: Promise<unknown>, codes: string[]) { const code = await task.then(() => null, (e: any) => e?.code); if (!codes.includes(code)) throw Error('UNEXPECTED_REJECTION') }
const failureCodes = new Set([
  'SOURCE_UNSUPPORTED', 'SOURCE_BUSY', 'SOURCE_CHANGED', 'SOURCE_UNAVAILABLE', 'SCOPE_UNKNOWN', 'SCOPE_STALE', 'SESSION_ORIGIN_AMBIGUOUS', 'PROFILE_CLI_MISMATCH', 'ENV_VALUE_UNAVAILABLE',
  'OPENCODE_UI_TIMEOUT', 'OPENCODE_ACK_ENABLED', 'OPENCODE_FIXTURE_MISSING', 'OPENCODE_UI_ERROR',
  'DESK_ARCHIVE_TIMEOUT', 'DESK_RESTORE_TIMEOUT', 'DESK_PERSISTENCE_INVALID',
  'RECYCLE_FIXTURE_MISSING', 'RECYCLE_PANEL_TIMEOUT', 'RECYCLE_REMOVE_TIMEOUT', 'RECYCLE_RESTORE_TIMEOUT', 'RECYCLE_ACK_ENABLED',
  'ELEMENT_MISSING', 'UNEXPECTED_REJECTION', 'REAL_HISTORY_MISSING', 'PREVIEW_TIMEOUT',
  'RISK_COPY_MISSING', 'SECOND_CONFIRMATION_MERGED', 'MISSING_ACK_ENABLED',
  'SUBTREE_ACK_OPTIONAL', 'NATIVE_DELETE_DISABLED', 'NATIVE_RESULT_TIMEOUT',
  'DELETED_ROW_RETAINED', 'CONTROL_ROW_LOST', 'RECEIPT_MISSING', 'DUPLICATE_RECEIPT_CHANGED',
  'INSPECT_TIMEOUT', 'EXPIRY_FIXTURE_MISSING', 'EXPIRED_TICKET_NOT_BLOCKED',
])
function failureCode(failure: unknown): string {
  const candidate = failure && typeof failure === 'object' && 'code' in failure ? failure.code : undefined
  const code = typeof candidate === 'string' ? candidate : failure instanceof Error ? failure.message : ''
  return failureCodes.has(code) ? code : 'FRONTEND_ACCEPTANCE_FAILED'
}
let started = false
host.__STAGE11__ = { async start() {
  if (started) return; started = true
  const bridge = host.__CC_DESK_DOCUMENT__
  if (!bridge) throw Error('DOCUMENT_BRIDGE_MISSING')
  // 只有原生宿主核实实际UDF和父Job后才调用start；模块加载不能提前写偏好或mount。
  const pinia = createPinia(); setActivePinia(pinia)
  const history = useNativeHistoryStore(), catalog = useUnifiedSessionsStore(), support = useCliSupportStore()
  support.setCliEnabled('codex', true); support.setCliEnabled('claude', true); support.setCliEnabled('opencode', true)
  const projects = useProjectsStateStore(); projects.loaded = true
  const unused = () => { throw new Error('UNRELATED_SESSION_ACTION') }
  catalog.configureAdapters([createNativeCliAdapter({
    tabs: useNativeTabsStore(), history, metadata: projects, historyEnabled: cli => support.isEnabled(cli),
    archive: { getArchivedSessions: useSessionStore().getArchivedSessions, archiveSession: projects.archiveSession, restoreSession: projects.restoreSession },
    runtime: { createTab: unused, restartTab: unused, stopTab: async () => unused(), renameTab: async () => unused() },
  })])
  const selectedId = ref(''), stage = ref('Waiting for the native document'), drawerOpen = ref(false)
  const target = computed(() => catalog.sessions.find(row => row.nativeSessionId === selectedId.value) ?? null)
  createApp({ render: () => h('main', { style: 'max-width:1000px;margin:30px auto;padding:24px' }, [
    h('h1', 'Task-isolated permanent deletion acceptance'), h('p', stage.value),
    h('p', 'Synthetic fixtures only. This host does not start the installed CC Desk application.'),
    h(NativeSessionDeletePanel, { target: target.value, active: true }),
    h('section', { 'data-desk-live': '' }, [h(SessionList, { sessions: catalog.sessions.filter(row => row.cli === 'claude' && !row.archived), onMenuAction: (id: string, action: string) => { if (action !== 'archive') unused(); void catalog.archiveSession(id).catch(() => {}) } })]),
    h(ArchivedSessionsDrawer, { open: drawerOpen.value, sessions: catalog.sessions, 'onUpdate:open': (value: boolean) => { drawerOpen.value = value }, onRestoreRequest: (id: string) => { void catalog.restoreArchivedSession(id).catch(() => {}) } }),
    h('h2', 'Observed synthetic history'), h('ul', { 'data-fixture-history': '' }, catalog.sessions.map(row => h('li', { key: row.id }, `${row.title} · ${row.nativeSessionId}`))),
  ]) }).use(pinia).use(createI18n({ legacy: false, locale: 'en', messages: { en, zh } })).mount('#app')
  const call = (command: string, payload: unknown = {}) => bridge.invoke(command, payload)
  const checkpoint = (name: string) => call('s11_checkpoint', { name })
  try {
    const fixture = await call('s11_fixture')
    let context: NativeHistoryContext = fixture.context
    async function reload() { await history.load({ ...context, force: true, retainOnFailure: true }); await catalog.refresh(context.projectPath, { cli: 'codex' }); selectedId.value = fixture.targetId; await nextTick() }
    await reload()
    if (!target.value || !history.get(context)?.source) throw Error('REAL_HISTORY_MISSING')
    await checkpoint('real-scope-history')
    await click('[data-native-cleanup-delete]'); await until(() => !!document.querySelector('[data-delete-next-confirm]'), 'PREVIEW_TIMEOUT')
    if (!hasText('[data-delete-dialog]', 'not a frozen list')) throw Error('RISK_COPY_MISSING')
    await call('s11_capture', { name: 'review' }); await checkpoint('review-only')
    await click('[data-delete-cancel]'); await checkpoint('cancelled-no-confirm')
    await click('[data-native-cleanup-delete]'); await until(() => !!document.querySelector('[data-delete-next-confirm]'), 'PREVIEW_TIMEOUT')
    if (document.querySelector('[data-delete-final]')) throw Error('SECOND_CONFIRMATION_MERGED')
    await click('[data-delete-next-confirm]')
    if (!(element('[data-delete-final]') as HTMLButtonElement).disabled) throw Error('MISSING_ACK_ENABLED')
    await ack('[data-delete-ack-permanent]')
    if (!(element('[data-delete-final]') as HTMLButtonElement).disabled) throw Error('SUBTREE_ACK_OPTIONAL')
    await checkpoint('independent-second-confirmation')
    await click('[data-delete-cancel]')
    const source = history.get(context)!.source!, sessionKey = target.value!.nativeHistorySessionKey!
    const client = createNativeSessionDeleteClient(), operationId = crypto.randomUUID()
    const preview = await client.preview({ operationId, source, sessionKey })
    const confirmation = { operationId, previewId: preview.previewId!, acknowledgePermanentDelete: true as const, acknowledgeNativeSubtree: true as const }
    for (const partial of [{ acknowledgePermanentDelete: false }, { acknowledgeNativeSubtree: false }]) await expectCode(call('cli_confirm_session_delete', { ...confirmation, ...partial }), ['DELETE_CONFIRMATION_REQUIRED'])
    await checkpoint('backend-missing-ack')
    const proof = await call('s11_proof')
    const bytes = new TextEncoder().encode(JSON.stringify(confirmation))
    await expectCode(host.__TAURI_INTERNALS__.invoke('cli_confirm_session_delete', bytes), ['FORBIDDEN'])
    await expectCode(host.__TAURI_INTERNALS__.invoke('cli_confirm_session_delete', bytes, { headers: { 'x-cc-desk-document': 'invalid-proof' } }), ['FORBIDDEN'])
    await expectCode(call('cli_preview_session_delete', { operationId: crypto.randomUUID(), source: { ...source, sourceRootKey: 'forged-root' }, sessionKey }), ['SCOPE_STALE', 'FORBIDDEN'])
    await call('s11_peer', { confirmation, proof })
    await checkpoint('wrong-document-source-peer')
    const previousContext = context
    context = await call('s11_change_source')
    await expectCode(call('cli_confirm_session_delete', confirmation), ['SCOPE_REVOKED', 'SCOPE_STALE', 'DELETE_PREVIEW_CHANGED'])
    await checkpoint('changed-source-ticket')
    // 与正式 profile watcher 一致：旧 revision 不再拥有历史行。
    history.invalidate(previousContext)
    await reload()
    if ((element('[data-native-cleanup-delete]') as HTMLButtonElement).disabled) throw Error('NATIVE_DELETE_DISABLED')
    await click('[data-native-cleanup-delete]'); await until(() => !!document.querySelector('[data-delete-next-confirm]'), 'PREVIEW_TIMEOUT')
    await click('[data-delete-cancel]')
    await history.load({ ...fixture.recycleContext, force: true })
    await catalog.refresh(fixture.recycleContext.projectPath, { cli: 'claude' })
    const recycleRow = catalog.sessions.find(row => row.nativeSessionId === fixture.recycleId && row.cli === 'claude')
    if (!recycleRow) throw Error('RECYCLE_FIXTURE_MISSING')
    await projects.reload()
    async function reloadDesk() {
      await projects.reload(); await catalog.refresh(fixture.recycleContext.projectPath, { cli: 'claude' }); await nextTick(); await frame()
      if (projects.lastErrorCode) throw Error('DESK_PERSISTENCE_INVALID')
    }
    function rowAt(selector: string): HTMLElement {
      const row = [...document.querySelectorAll<HTMLElement>(selector)].find(row => row.dataset.sessionRow === recycleRow!.id)
      if (!row) throw Error('RECYCLE_FIXTURE_MISSING')
      return row
    }
    async function archiveFromRow() {
      const archive = rowAt('[data-desk-live] [data-session-row]').querySelector<HTMLButtonElement>('[data-session-archive] button')
      if (!archive || archive.disabled) throw Error('ELEMENT_MISSING')
      archive.click(); await nextTick(); await frame()
      await until(() => catalog.sessions.some(row => row.id === recycleRow!.id && row.archived), 'DESK_ARCHIVE_TIMEOUT')
      await reloadDesk()
      if (!catalog.sessions.some(row => row.id === recycleRow!.id && row.archived)) throw Error('DESK_PERSISTENCE_INVALID')
    }
    await archiveFromRow()
    drawerOpen.value = true; await nextTick(); await frame()
    await call('s11_desk_checkpoint', { name: 'archived' }); await call('s11_capture', { name: 'desk-archived' })
    const restoreArchive = rowAt('.archived-sessions-content [data-session-row]').querySelector<HTMLElement>('.session-primary-action button')
    if (!restoreArchive) throw Error('ELEMENT_MISSING')
    restoreArchive.click(); restoreArchive.click()
    await until(() => catalog.sessions.some(row => row.id === recycleRow!.id && !row.archived) && [...projects.sessionRecords.values()].some(record => record.nativeSessionId === fixture.recycleId && (record.lastOpenedAt ?? 0) > 0), 'DESK_RESTORE_TIMEOUT')
    await reloadDesk()
    if (catalog.sessions.some(row => row.id === recycleRow!.id && row.archived)) throw Error('DESK_PERSISTENCE_INVALID')
    await call('s11_desk_checkpoint', { name: 'restored' }); await call('s11_capture', { name: 'desk-restored' })
    drawerOpen.value = false; await nextTick(); await frame()
    await archiveFromRow()
    drawerOpen.value = true; await nextTick(); await frame()
    await call('s11_desk_checkpoint', { name: 'rearchived' }); await call('s11_capture', { name: 'desk-rearchived' })
    if (document.querySelector('[data-recycle-open], [data-recycle-remove], [data-opencode-archive-open]')) throw Error('RECYCLE_PANEL_TIMEOUT')
    await call('s11_recycle_checkpoint', { name: 'review' }); await call('s11_capture', { name: 'recycle-review' })
    // 仅用隔离合成来源准备旧版本数据；产品 UI 不再提供移除入口。
    drawerOpen.value = false; await nextTick(); await frame()
    await call('cli_recycle_session', { source: history.get(fixture.recycleContext)!.source!, action: 'remove', operationId: crypto.randomUUID(), sessionKey: recycleRow.nativeHistorySessionKey, acknowledgeRecoverable: true })
    await history.load({ ...fixture.recycleContext, force: true, retainOnFailure: true })
    await reloadDesk()
    drawerOpen.value = true; await nextTick(); await frame()
    await until(() => !catalog.sessions.some(row => row.nativeSessionId === fixture.recycleId) && !!document.querySelector('[data-recycle-restore]') && !(element('[data-recycle-restore]') as HTMLButtonElement).disabled, 'RECYCLE_REMOVE_TIMEOUT')
    await call('s11_recycle_checkpoint', { name: 'stored' }); await call('s11_capture', { name: 'recycle-stored' })
    const restore = element('[data-recycle-restore]'); restore.click(); restore.click()
    await until(() => catalog.sessions.some(row => row.nativeSessionId === fixture.recycleId && row.archived) && !document.querySelector('[data-recycle-restore]'), 'RECYCLE_RESTORE_TIMEOUT')
    await call('s11_recycle_checkpoint', { name: 'restored' }); await call('s11_capture', { name: 'recycle-restored' })
    drawerOpen.value = false; await nextTick(); await frame()
    await history.load({ ...fixture.opencodeContext, force: true })
    await catalog.refresh(fixture.opencodeContext.projectPath, { cli: 'opencode' })
    const opencodeRow = () => catalog.sessions.find(row => row.cli === 'opencode' && row.nativeSessionId === fixture.opencodeId)
    if (!opencodeRow() || opencodeRow()!.nativeArchived) throw Error('OPENCODE_FIXTURE_MISSING')
    // Preserve a real Desk tag to verify native restoration does not clear it.
    const opencodeId = opencodeRow()!.id
    await projects.archiveSession(fixture.opencodeContext.projectPath, opencodeId)
    await catalog.refresh(fixture.opencodeContext.projectPath, { cli: 'opencode' })
    await call('s11_opencode_checkpoint', { name: 'review' }); await call('s11_capture', { name: 'opencode-review' })
    // 先通过认证后端准备旧归档，再验证普通抽屉只保留单条恢复。
    await call('cli_set_opencode_archive', { source: history.get(fixture.opencodeContext)!.source!, operationId: crypto.randomUUID(), sessionKey: opencodeRow()!.nativeHistorySessionKey, archived: true, acknowledgeRecoverable: true })
    await history.load({ ...fixture.opencodeContext, force: true, retainOnFailure: true })
    await catalog.refresh(fixture.opencodeContext.projectPath, { cli: 'opencode' })
    await call('s11_opencode_checkpoint', { name: 'archived' }); await call('s11_capture', { name: 'opencode-archived' })
    drawerOpen.value = true; await nextTick(); await frame()
    if (document.querySelector('[data-opencode-archive-open]')) throw Error('OPENCODE_UI_ERROR')
    await click('[data-session-opencode-archive]')
    await until(() => !!document.querySelector('[data-opencode-archive-submit]'), 'OPENCODE_UI_TIMEOUT')
    if (!(element('[data-opencode-archive-submit]') as HTMLButtonElement).disabled) throw Error('OPENCODE_ACK_ENABLED')
    await ack('[data-opencode-archive-ack]')
    const submit = element('[data-opencode-archive-submit]'); submit.click(); submit.click()
    await until(() => { if (document.querySelector('[data-opencode-archive-error]')) throw Error('OPENCODE_UI_ERROR'); return opencodeRow()?.nativeArchived === false && !(element('[data-opencode-archive-refresh]') as HTMLButtonElement).disabled && !document.querySelector('[data-opencode-archive-pending]') }, 'OPENCODE_UI_TIMEOUT', 40_000)
    await call('s11_opencode_checkpoint', { name: 'restored' }); await call('s11_capture', { name: 'opencode-restored' })
    await projects.restoreSession(fixture.opencodeContext.projectPath, opencodeId)
    drawerOpen.value = false; await nextTick(); await frame()
    if (!fixture.native) { stage.value = 'NoNative document, deletion review, legacy Claude recovery and OpenCode restore checks passed'; await call('s11_finish', { success: true }); return }
    await click('[data-native-cleanup-delete]'); await until(() => !!document.querySelector('[data-delete-next-confirm]'), 'PREVIEW_TIMEOUT')
    await click('[data-delete-next-confirm]'); await ack('[data-delete-ack-permanent]'); await ack('[data-delete-ack-subtree]')
    await call('s11_capture', { name: 'confirmation' })
    const button = element('[data-delete-final]'); button.click(); button.click()
    await until(() => hasText('[data-delete-result]', 'acknowledged'), 'NATIVE_RESULT_TIMEOUT', 45_000)
    await until(() => !catalog.sessions.some(row => row.nativeSessionId === fixture.targetId), 'DELETED_ROW_RETAINED')
    if (!catalog.sessions.some(row => row.nativeSessionId === fixture.controlId)) throw Error('CONTROL_ROW_LOST')
    await call('s11_capture', { name: 'result' }); await checkpoint('native-delete-result')
    const operation = [...useNativeSessionDeletesStore().records.values()].find(record => record.receipt?.state === 'acknowledged')
    if (!operation?.preview?.previewId || !operation.receipt) throw Error('RECEIPT_MISSING')
    const repeated = { operationId: operation.request.operationId, previewId: operation.preview.previewId, acknowledgePermanentDelete: true, acknowledgeNativeSubtree: true }
    for (let i = 0; i < 2; i++) {
      const duplicate = await call('cli_confirm_session_delete', repeated), receipt = operation.receipt
      if (Object.keys(duplicate).sort().join(',') !== Object.keys(receipt).sort().join(',') || Object.keys(receipt).some(key => (duplicate as any)[key] !== (receipt as any)[key])) throw Error('DUPLICATE_RECEIPT_CHANGED')
    }
    await checkpoint('duplicate-confirm-receipt')
    await click('[data-delete-inspect]'); await until(() => operation.inspected === true && !operation.error && hasText('[data-delete-result]', 'acknowledged'), 'INSPECT_TIMEOUT')
    await checkpoint('inspect-without-replay')
    const freshSource = history.get(context)!.source!, expiryOperation = crypto.randomUUID()
    const expiry = history.get(context)!.sessions.find(row => row.nativeSessionId === fixture.expiryId)
    if (!expiry) throw Error('EXPIRY_FIXTURE_MISSING')
    const expiredPreview = await call('cli_preview_session_delete', { operationId: expiryOperation, source: freshSource, sessionKey: expiry.sessionKey })
    stage.value = 'Waiting for the production 120-second preview expiry; no CLI is active'
    await checkpoint('expiry-wait-started'); await new Promise(resolve => setTimeout(resolve, 121_000))
    const expiredReceipt = await call('cli_confirm_session_delete', { operationId: expiryOperation, previewId: expiredPreview.previewId, acknowledgePermanentDelete: true, acknowledgeNativeSubtree: true })
    if (expiredReceipt.operationId !== expiryOperation || expiredReceipt.cli !== 'codex' || expiredReceipt.state !== 'blocked'
      || expiredReceipt.reason !== 'DELETE_PREVIEW_CHANGED' || expiredReceipt.rootPresent !== null || expiredReceipt.descendants !== 'unverified') throw Error('EXPIRED_TICKET_NOT_BLOCKED')
    await checkpoint('expired-ticket')
    stage.value = 'Real document IPC, double confirmation and isolated CLI deletion passed'
    await call('s11_finish', { success: true })
  } catch (failure) { stage.value = 'Acceptance failed; no request is replayed'; await call('s11_finish', { success: false, failureCode: failureCode(failure) }) }
} }
