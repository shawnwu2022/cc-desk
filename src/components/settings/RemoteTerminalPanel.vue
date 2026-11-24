<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { documentClientPort, documentClientStatus } from '../../../remote/ui-terminal/src/ipc'
import { nativeConnection, type RetirableNativeConnection } from '../../../remote/ui-terminal/src/native'
import { TerminalClient } from '../../../remote/ui-terminal/src/client'
import AppButton from '@/components/ui/AppButton.vue'
import type { TerminalConnectionSource } from '../../../remote/ui-terminal/src/connectionSource'
import type { browserReconnectSession, BrowserRetryState } from '../../../remote/ui-terminal/src/browserReconnect'
type RetryOwner = ReturnType<typeof browserReconnectSession>
const props = defineProps<{ source?: TerminalConnectionSource; reconnectSession?: (signal: AbortSignal) => Promise<RetryOwner | undefined> }>()
const { t } = useI18n()
const state = ref<'Locked' | 'Synced' | 'ViewOnly' | 'Unavailable' | 'Uncertain'>('Locked')
const busy = ref(false), output = ref<HTMLElement>()
const role = ref<'Controller' | 'Viewer'>('Viewer'), draft = ref('')
const inputStatus = ref<'' | 'HostWritten' | 'Rejected' | 'TooLarge'>('')
type Owned = { kind: 'single'; connection: RetirableNativeConnection } | { kind: 'reconnect'; owner: RetryOwner }
let owned: Owned | undefined, client: TerminalClient | undefined
const retryOptIn = ref(false), trialAttempted = ref(false), cancelling = ref(false)
const retryState = ref<BrowserRetryState>('New'), localDeadline = ref<number>(), replacements = ref(0)
const connected = computed(() => !retryOptIn.value || retryState.value === 'Connected')
let retirementUnknown = false
let observeTimer: ReturnType<typeof setTimeout> | undefined, openingWork: Promise<void> | undefined
function stopObserve() { if (observeTimer) clearTimeout(observeTimer); observeTimer = undefined }
function observeRetry() {
  stopObserve()
  if(disposed || owned?.kind !== 'reconnect') return
  const observation=owned.owner.observe()
  retryState.value=observation.state; localDeadline.value=observation.localDeadline; replacements.value=observation.replacements
  role.value=observation.role ?? 'Viewer'
  if(observation.state==='Unknown') state.value='Uncertain'
  else if(observation.state==='Closed') state.value='Locked'
  if(!['Closed','Unknown'].includes(observation.state)) observeTimer=setTimeout(observeRetry,100)
}
async function retire(value: Owned) {
  try {
    if(value.kind==='single'){value.connection.close();await value.connection.waitClosed()}
    else {await value.owner.close();await value.owner.waitClosed()}
  } catch(error) {retirementUnknown=true;throw error}
}
function currentlyConnected(){return owned?.kind !== 'reconnect' || owned.owner.observe().state === 'Connected'}
let disposed = false, epoch = 0
let opening: AbortController | undefined
async function close() {
  if(state.value==='ViewOnly')output.value?.replaceChildren()
  epoch++; opening?.abort(); stopObserve(); client?.disconnect()
  const value=owned, pending=openingWork
  // 先关闭父门，再等仍在取得或启动的原 owner；迟到结果也由 acquire 原路收口。
  const closing=value ? retire(value) : Promise.resolve()
  const results=await Promise.allSettled([closing, ...(pending ? [pending] : [])])
  if (owned === value) { owned=undefined; client=undefined; role.value='Viewer' }
  if(retirementUnknown || results[0]?.status==='rejected') throw Error('remote retirement uncertain')
}
async function disconnect() {
  if(cancelling.value) return
  cancelling.value=true; inputStatus.value=''
  try { await close(); if(!disposed){state.value='Locked';output.value?.replaceChildren()} }
  catch { if(!disposed) state.value='Uncertain' }
  finally { cancelling.value=false }
}
async function acquire(current: number) {
  opening=new AbortController()
  const signal=opening.signal
  if(retryOptIn.value && props.reconnectSession) {
    if(trialAttempted.value) throw Error('retry trial already used')
    trialAttempted.value=true
    const owner=await props.reconnectSession(signal)
    if(!owner) return
    const value:Owned={kind:'reconnect',owner}
    if(disposed || current!==epoch || signal.aborted){await retire(value);return}
    owned=value; observeRetry()
    try {
      const original=await owner.start()
      if(disposed || current!==epoch || signal.aborted){await retire(value);return}
      client=original;role.value=owner.observe().role ?? 'Viewer';observeRetry()
    } catch(error) {await retire(value);throw error}
  } else {
    const source=props.source
    const found=source ? await source.open(signal) : await (async()=>{
      const status=await documentClientStatus()
      return status.state==='Locked' ? undefined : {...status,connection:nativeConnection(documentClientPort(status.resourceId))}
    })()
    if(!found) return
    const value:Owned={kind:'single',connection:found.connection}
    if(disposed || current!==epoch || signal.aborted){await retire(value);return}
    owned=value;role.value=found.role;client=new TerminalClient(found.target,found.binding);client.connect(found.connection)
    if(found.connection.observerSnapshot)found.connection.onInvalidate?.(()=>{
      if(disposed||owned!==value)return
      output.value?.replaceChildren()
      state.value='Locked'
    })
  }
}
async function refresh() {
  if (busy.value || cancelling.value || disposed || state.value === 'Uncertain' || state.value === 'ViewOnly' || (trialAttempted.value && !client)) return
  inputStatus.value = ''
  busy.value = true
  const current = epoch
  try {
    if (!client) {
      openingWork=acquire(current)
      try {await openingWork} finally {openingWork=undefined;opening=undefined}
      if(disposed || current!==epoch) return
      if(!client){state.value='Locked';return}
    }
    if(!currentlyConnected()) return
    await client.snapshot()
    if (disposed || current !== epoch) return
    output.value?.replaceChildren(client.render(document) as HTMLElement)
    state.value = client.phase === 'ViewOnly' ? 'ViewOnly' : 'Synced'
  } catch {
    const report = current===epoch
    try { await close() } catch { /* 原 owner 保留未知收口，不重试。 */ }
    if (!disposed && report) state.value = retirementUnknown ? 'Uncertain' : 'Unavailable'
  } finally { busy.value = false }
}
async function send() {
  if (busy.value || cancelling.value || !currentlyConnected() || disposed || state.value !== 'Synced' || role.value !== 'Controller' || !client) return
  const bytes = new TextEncoder().encode(draft.value)
  if (bytes.length === 0) return
  if (bytes.length > 8192) { inputStatus.value = 'TooLarge'; return }
  const owned = client, current = epoch
  const check = () => { if (disposed || current !== epoch || client !== owned || !currentlyConnected()) throw Error('stale input') }
  busy.value = true; inputStatus.value = ''
  let submitted = false
  try {
    const cursor = await owned.cursor(); check()
    // A previous operation is reconciled separately; this draft never retries it.
    if (cursor.pending || cursor.nextSequence === null) { state.value = 'Uncertain'; return }
    await owned.snapshot(); check()
    await owned.window(); check()
    const id = crypto.getRandomValues(new Uint8Array(16))
    const operation = Array.from(id, byte => byte.toString(16).padStart(2, '0')).join('')
    submitted = true
    const receipt = await owned.submit(operation, cursor.nextSequence, bytes); check()
    if (receipt.message === 'InputNotAccepted') { inputStatus.value = 'Rejected'; return }
    if (receipt.outcome === 'Unknown' || receipt.outcome === 'Partial') { state.value = 'Uncertain'; return }
    inputStatus.value = receipt.outcome
    if (receipt.outcome === 'HostWritten') draft.value = ''
    await owned.settle(); check()
    if (owned.phase !== 'Idle') { state.value = 'Uncertain'; return }
    await owned.snapshot(); check()
    output.value?.replaceChildren(owned.render(document) as HTMLElement)
  } catch {
    if (!disposed && current === epoch) {
      state.value = submitted ? 'Uncertain' : 'Unavailable'
      try { await close() } catch { /* Original native ownership stays retained. */ }
    }
  } finally { busy.value = false }
}
onBeforeUnmount(() => { disposed = true; void close().catch(() => { console.warn('Remote session retirement unconfirmed') }) })
</script>
<template>
  <section class="remote-panel" :aria-label="t('remoteTerminalTitle')" data-remote-terminal>
    <h3>{{ t('remoteTerminalTitle') }}</h3>
    <p role="status">{{ t(`remoteTerminal${state}`) }}</p>
    <p class="remote-hint">{{ t(props.source ? 'remoteTerminalBrowserHint' : 'remoteTerminalPreviewHint') }}</p>
    <label v-if="props.reconnectSession"><input data-remote-retry-optin type="checkbox" v-model="retryOptIn" :disabled="busy || trialAttempted || !!owned">启用可取消重连试用（默认关闭）</label>
    <p v-if="retryOptIn">本地尝试预算最多60秒，安全空闲且原资源退出后最多3次替代；不是Host许可期限，Host可提前截止或撤销。未结算输入不重试、不重放；首次连接失败即结束。关闭、隐藏或离开页面均停止。</p>
    <p v-if="retryOptIn" data-retry-state>{{ retryState }} · 替代 {{ replacements }}/3 · 本地截止 {{ localDeadline ? new Date(localDeadline).toLocaleString() : '尚未开始' }}</p>
    <div class="remote-actions">
      <AppButton data-remote-refresh :disabled="busy || cancelling || state === 'Uncertain' || state === 'ViewOnly' || (trialAttempted && !client)" @click="refresh">{{ t('remoteTerminalRefresh') }}</AppButton>
      <AppButton data-remote-disconnect :disabled="cancelling || (!busy && !owned && state === 'Locked')" @click="disconnect">{{ t('remoteTerminalDisconnect') }}</AppButton>
    </div>
    <div ref="output" class="remote-output" tabindex="0" :aria-label="t('remoteTerminalOutput')" />
    <div v-if="state === 'Synced' && role === 'Controller'" class="remote-input">
      <label for="remote-literal-input">{{ t('remoteTerminalInput') }}</label>
      <textarea id="remote-literal-input" v-model="draft" data-remote-input :disabled="busy || cancelling || !connected" rows="3" maxlength="8192" />
      <AppButton data-remote-send :disabled="busy || cancelling || !connected || draft.length === 0" @click="send">{{ t('remoteTerminalSend') }}</AppButton>
    </div>
    <p v-if="inputStatus" role="status">{{ t(`remoteTerminal${inputStatus}`) }}</p>
  </section>
</template>
<style scoped>
.remote-panel { display: flex; flex-direction: column; gap: 12px; border-top: 1px solid var(--border-color); padding-top: 16px; min-width: 0; }
h3 { color: var(--text-primary); font-size: 15px; }
p { margin: 0; color: var(--text-secondary); }
.remote-hint { font-size: 12px; line-height: 1.5; }
.remote-actions { display: flex; flex-wrap: wrap; gap: 8px; }
.remote-output { overflow: auto; max-height: 50vh; font-family: var(--terminal-font-family, monospace); background: var(--terminal-background, var(--bg-secondary)); }
.remote-input { display: flex; flex-direction: column; gap: 8px; }
textarea { color: var(--text-primary); background: var(--bg-secondary); border: 1px solid var(--border-color); border-radius: var(--radius-md); padding: 8px; resize: vertical; }
</style>
