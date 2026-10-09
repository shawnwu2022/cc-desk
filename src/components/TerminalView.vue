<script setup lang="ts">
import { computed, ref } from 'vue'
import { useAppStore } from '@/stores/app'
import { useSessionStore } from '@/stores/session'
import { computeTerminalSurfaceVars, getTerminalTheme } from '@/config/terminalThemes'
import { useWindowAttention } from '@/composables/useWindowAttention'
import { useStatusMonitor } from '@/composables/useStatusMonitor'
import XTermTerminal from './XTermTerminal.vue'

// One Legacy aggregate owned by UnifiedTerminalHost. Mounting has no launch,
// history-selection, navigation or project-configuration side effects.
const props = withDefaults(defineProps<{ visible?: boolean }>(), { visible: true })
const app = useAppStore()
const sessions = useSessionStore()
const terminalRef = ref<InstanceType<typeof XTermTerminal> | null>(null)
const terminalSurfaceStyle = computed(() => computeTerminalSurfaceVars(getTerminalTheme(app.terminalTheme)))
const { isFocused } = useWindowAttention()
useStatusMonitor({ isFocused, isTerminalVisible: computed(() => props.visible) })

function handlePtyStarted(tabId: string, ptyId: string) {
  const tab = sessions.tabs.get(tabId)
  if (!tab || tab.ptyId !== ptyId) return
  app.ensureProjectInList(tab.projectPath)
  if (!tab.isResume) void sessions.loadHistorySessions(tab.projectPath, true)
}

function requireTerminal() {
  if (!terminalRef.value) throw new Error('LEGACY_TERMINAL_NOT_READY')
  return terminalRef.value
}
async function startTab(tabId: string) {
  const result = await requireTerminal().startTab(tabId)
  if (!result?.ok) throw new Error('LEGACY_LAUNCH_FAILED')
}
async function stopTab(tabId: string) { await requireTerminal().stopTab(tabId) }
async function restartTab(tabId: string) { await requireTerminal().restartTab(tabId) }
async function renameTab(tabId: string, title: string) { await requireTerminal().renameTab(tabId, title) }
async function recover() { await requireTerminal().recover() }
function focus() { if (props.visible) terminalRef.value?.focus() }
function fitVisible() { terminalRef.value?.fitVisible() }
defineExpose({ startTab, stopTab, stop: stopTab, recover, restartTab, renameTab, focus, fitVisible })
</script>

<template>
  <div class="terminal-view" data-terminal-view :style="terminalSurfaceStyle">
    <XTermTerminal ref="terminalRef" :visible="visible" @pty-started="handlePtyStarted" />
  </div>
</template>

<style scoped>
.terminal-view { position: relative; display: flex; flex: 1; min-width: 0; min-height: 0; overflow: hidden; background: var(--terminal-surface-bg); }
</style>
