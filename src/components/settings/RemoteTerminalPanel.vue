<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { documentClientPort, documentClientStatus } from '../../../remote/ui-terminal/src/ipc'
import { nativeConnection, type RetirableNativeConnection } from '../../../remote/ui-terminal/src/native'
import { TerminalClient } from '../../../remote/ui-terminal/src/client'
import AppButton from '@/components/ui/AppButton.vue'
const { t } = useI18n()
const state = ref<'Locked' | 'Synced' | 'Unavailable'>('Locked')
const busy = ref(false), output = ref<HTMLElement>()
let connection: RetirableNativeConnection | undefined, client: TerminalClient | undefined
let disposed = false, epoch = 0
async function close() {
  epoch++; client?.disconnect()
  const owned = connection
  if (owned) { owned.close(); await owned.waitClosed() }
  if (connection === owned) { connection = undefined; client = undefined }
}
async function disconnect() {
  if (busy.value) return
  busy.value = true
  try { await close(); if (!disposed) { state.value = 'Locked'; output.value?.replaceChildren() } }
  catch { if (!disposed) state.value = 'Unavailable' }
  finally { busy.value = false }
}
async function refresh() {
  if (busy.value || disposed) return
  busy.value = true
  const current = epoch
  try {
    if (!client) {
      const status = await documentClientStatus()
      if (disposed || current !== epoch) return
      if (status.state === 'Locked') { state.value = 'Locked'; return }
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
onBeforeUnmount(() => { disposed = true; void close().catch(() => {}) })
</script>
<template>
  <section class="remote-panel" :aria-label="t('remoteTerminalTitle')" data-remote-terminal>
    <h3>{{ t('remoteTerminalTitle') }}</h3>
    <p role="status">{{ t(`remoteTerminal${state}`) }}</p>
    <p class="remote-hint">{{ t('remoteTerminalPreviewHint') }}</p>
    <div class="remote-actions">
      <AppButton data-remote-refresh :disabled="busy" @click="refresh">{{ t('remoteTerminalRefresh') }}</AppButton>
      <AppButton data-remote-disconnect :disabled="busy || state === 'Locked'" @click="disconnect">{{ t('remoteTerminalDisconnect') }}</AppButton>
    </div>
    <div ref="output" class="remote-output" tabindex="0" :aria-label="t('remoteTerminalOutput')" />
  </section>
</template>
<style scoped>
.remote-panel { display: flex; flex-direction: column; gap: 12px; border-top: 1px solid var(--border-color); padding-top: 16px; min-width: 0; }
h3 { color: var(--text-primary); font-size: 15px; }
p { margin: 0; color: var(--text-secondary); }
.remote-hint { font-size: 12px; line-height: 1.5; }
.remote-actions { display: flex; flex-wrap: wrap; gap: 8px; }
.remote-output { overflow: auto; max-height: 50vh; font-family: var(--terminal-font-family, monospace); background: var(--terminal-background, var(--bg-secondary)); }
</style>
