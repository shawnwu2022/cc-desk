<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { documentClientPort, documentClientStatus } from '../../../remote/ui-terminal/src/ipc'
import { nativeConnection, type RetirableNativeConnection } from '../../../remote/ui-terminal/src/native'
import { TerminalClient } from '../../../remote/ui-terminal/src/client'
import AppButton from '@/components/ui/AppButton.vue'
const { t } = useI18n()
const state = ref<'Locked' | 'Synced' | 'Unavailable' | 'Uncertain'>('Locked')
const busy = ref(false), output = ref<HTMLElement>()
const role = ref<'Controller' | 'Viewer'>('Viewer'), draft = ref('')
const inputStatus = ref<'' | 'HostWritten' | 'Rejected' | 'TooLarge'>('')
let connection: RetirableNativeConnection | undefined, client: TerminalClient | undefined
let disposed = false, epoch = 0
async function close() {
  epoch++; client?.disconnect()
  const owned = connection
  if (owned) { owned.close(); await owned.waitClosed() }
  if (connection === owned) { connection = undefined; client = undefined; role.value = 'Viewer' }
}
async function disconnect() {
  if (busy.value) return
  inputStatus.value = ''
  busy.value = true
  try { await close(); if (!disposed) { state.value = 'Locked'; output.value?.replaceChildren() } }
  catch { if (!disposed) state.value = 'Unavailable' }
  finally { busy.value = false }
}
async function refresh() {
  if (busy.value || disposed || state.value === 'Uncertain') return
  inputStatus.value = ''
  busy.value = true
  const current = epoch
  try {
    if (!client) {
      const status = await documentClientStatus()
      if (disposed || current !== epoch) return
      if (status.state === 'Locked') { state.value = 'Locked'; return }
      role.value = status.role
      connection = nativeConnection(documentClientPort(status.resourceId))
      client = new TerminalClient(status.target, status.binding)
      client.connect(connection)
    }
    await client.snapshot()
    if (disposed || current !== epoch) return
    output.value?.replaceChildren(client.render(document) as HTMLElement)
    state.value = 'Synced'
  } catch {
    try { await close() } catch { /* Native owner retains its failed retirement. */ }
    if (!disposed) state.value = 'Unavailable'
  } finally { busy.value = false }
}
async function send() {
  if (busy.value || disposed || state.value !== 'Synced' || role.value !== 'Controller' || !client) return
  const bytes = new TextEncoder().encode(draft.value)
  if (bytes.length === 0) return
  if (bytes.length > 8192) { inputStatus.value = 'TooLarge'; return }
  const owned = client, current = epoch
  const check = () => { if (disposed || current !== epoch || client !== owned) throw Error('stale input') }
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
onBeforeUnmount(() => { disposed = true; void close().catch(() => {}) })
</script>
<template>
  <section class="remote-panel" :aria-label="t('remoteTerminalTitle')" data-remote-terminal>
    <h3>{{ t('remoteTerminalTitle') }}</h3>
    <p role="status">{{ t(`remoteTerminal${state}`) }}</p>
    <p class="remote-hint">{{ t('remoteTerminalPreviewHint') }}</p>
    <div class="remote-actions">
      <AppButton data-remote-refresh :disabled="busy || state === 'Uncertain'" @click="refresh">{{ t('remoteTerminalRefresh') }}</AppButton>
      <AppButton data-remote-disconnect :disabled="busy || state === 'Locked'" @click="disconnect">{{ t('remoteTerminalDisconnect') }}</AppButton>
    </div>
    <div ref="output" class="remote-output" tabindex="0" :aria-label="t('remoteTerminalOutput')" />
    <div v-if="state === 'Synced' && role === 'Controller'" class="remote-input">
      <label for="remote-literal-input">{{ t('remoteTerminalInput') }}</label>
      <textarea id="remote-literal-input" v-model="draft" data-remote-input :disabled="busy" rows="3" maxlength="8192" />
      <AppButton data-remote-send :disabled="busy || draft.length === 0" @click="send">{{ t('remoteTerminalSend') }}</AppButton>
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
