<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import TerminalView from '@/components/TerminalView.vue'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'
import { useShellStore } from '@/stores/shell'
import { useAppStore } from '@/stores/app'
import { computeTerminalSurfaceVars, getTerminalTheme } from '@/config/terminalThemes'
import type { OpenTerminalSession, UnifiedTerminalHostPort } from '@/terminal/unifiedTerminalHost'
import type { NativeAttemptIdentity } from '@/stores/nativeTabs'

const props = withDefaults(defineProps<{
  activeSessionId: string | null
  sessions: OpenTerminalSession[]
  visible?: boolean
}>(), { visible: true })
const shell = useShellStore()
const app = useAppStore()
const surface = computed(() => computeTerminalSurfaceVars(getTerminalTheme(app.terminalTheme)))
const legacy = ref<InstanceType<typeof TerminalView> | null>(null)
const native = new Map<string, InstanceType<typeof NativeCliTerminal>>()
const legacyMounted = ref(false)
watch(() => props.sessions.some(session => session.runtime === 'legacy-claude'), exists => {
  if (exists) legacyMounted.value = true
}, { immediate: true })
const active = computed(() => props.sessions.find(session => session.id === props.activeSessionId))
const nativeSessions = computed(() => props.sessions.filter(session => session.runtime === 'native-cli'))
const legacyVisible = computed(() => props.visible && active.value?.runtime === 'legacy-claude')
function setNative(id: string, value: unknown) {
  if (value) native.set(id, value as InstanceType<typeof NativeCliTerminal>)
  else native.delete(id)
}
function activeTerminal() {
  if (!props.visible || !active.value) return null
  return active.value.runtime === 'legacy-claude' ? legacy.value : native.get(active.value.adapterSessionId)
}
function focus() { activeTerminal()?.focus() }
function fitVisible() { activeTerminal()?.fitVisible() }
watch(() => [props.visible, props.activeSessionId], async () => {
  await nextTick()
  fitVisible()
  focus()
})
// Column toggles/width changes may not emit a window resize. A child's own
// ResizeObserver covers actual geometry; these explicit hooks cover shell layout.
watch(() => [shell.sidebarWidth, shell.sidebarVisible, shell.drawerWidth, shell.drawerVisible, shell.responsiveMode], async () => {
  await nextTick()
  fitVisible()
})
function requireLegacy() {
  if (!legacy.value) throw new Error('LEGACY_TERMINAL_NOT_READY')
  return legacy.value
}
function requireNative(tabId: string) {
  const terminal = native.get(tabId)
  if (!terminal) throw new Error('NATIVE_TERMINAL_NOT_READY')
  return terminal
}
const port: UnifiedTerminalHostPort = {
  async startLegacy(id) { await nextTick(); await requireLegacy().startTab(id) },
  async stopLegacy(id) { await requireLegacy().stopTab(id) },
  async restartLegacy(id) { await requireLegacy().restartTab(id) },
  async renameLegacy(id, title) { await requireLegacy().renameTab(id, title) },
  async stopNative(id: string, attempt: NativeAttemptIdentity) { await requireNative(id).stop(attempt) },
  async recoverNative(id: string, attempt: NativeAttemptIdentity) { await requireNative(id).recover(attempt) },
  focus,
}
defineExpose({ ...port, fitVisible })
</script>

<template>
  <div class="unified-terminal-host" data-unified-terminal-host :style="surface">
    <!-- XTermTerminal already aggregates Legacy tabs; there is exactly one owner. -->
    <TerminalView v-if="legacyMounted" ref="legacy" :visible="legacyVisible" v-show="legacyVisible" />
    <NativeCliTerminal v-for="session in nativeSessions" :key="session.id"
      :ref="value => setNative(session.adapterSessionId, value)" :tab-id="session.adapterSessionId"
      :active="visible && activeSessionId === session.id" />
    <div v-if="!active" class="unified-terminal-empty" data-unified-terminal-empty><slot /></div>
  </div>
</template>

<style scoped>
.unified-terminal-host { position: relative; display: flex; flex: 1; min-width: 0; min-height: 0; overflow: hidden; background: var(--terminal-surface-bg); }
.unified-terminal-empty { display: grid; flex: 1; min-width: 0; min-height: 0; background: var(--bg-primary); }
</style>
