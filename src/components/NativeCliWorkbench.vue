<template>
  <section class="native-workbench">
    <header class="native-toolbar">
      <button class="back-btn" @click="$emit('back')">←</button>
      <strong>Native CLI</strong>

      <div class="cli-switch">
        <button
          v-for="kind in nativeClis"
          :key="kind"
          :class="{ active: workbench.cli === kind }"
          :disabled="!workbench.profiles.selected[kind]"
          @click="switchCli(kind)"
        >
          {{ kind === 'claude' ? 'Claude Code' : 'Codex CLI' }}
        </button>
      </div>

      <select
        class="select"
        :value="workbench.profiles.selected[workbench.cli]?.id ?? ''"
        @change="changeProfile"
      >
        <option
          v-for="profile in workbench.profiles.byCli[workbench.cli]"
          :key="profile.id"
          :value="profile.id"
        >
          {{ profile.name }}
        </option>
      </select>

      <select
        class="select project-select"
        :value="workbench.selectedProjectId ?? ''"
        @change="changeProject"
      >
        <option value="" disabled>Select project</option>
        <option
          v-for="project in workbench.workspace.projects"
          :key="project.projectId"
          :value="project.projectId"
        >
          {{ projectLabel(project) }}
        </option>
      </select>
      <button @click="addProject">+ Project</button>

      <button :disabled="!canCreate" @click="createNew">New</button>
      <button :disabled="!canCreate" @click="createPicker">Resume…</button>
      <input
        v-model="resumeId"
        class="resume-input"
        placeholder="Session ID"
        @keyup.enter="createKnownResume"
      />
      <button :disabled="!canCreate || !resumeId.trim()" @click="createKnownResume">
        Resume ID
      </button>
    </header>

    <div v-if="workbench.status === 'loading'" class="state-banner">Loading native workspace…</div>
    <div v-else-if="workbench.error" class="state-banner error">{{ workbench.error }}</div>

    <div class="native-body">
      <main class="terminal-area">
        <div class="tab-strip">
          <button
            v-for="tab in tabList"
            :key="tab.tabId"
            class="tab"
            :class="{ active: workbench.tabs.activeTabId === tab.tabId }"
            @click="workbench.tabs.setActive(tab.tabId)"
          >
            <span>{{ tab.cli === 'claude' ? 'Claude' : 'Codex' }}</span>
            <small>{{ tab.status }}</small>
            <span v-if="tab.errorCode" class="tab-error">!</span>
            <span class="tab-close" @click.stop="closeTab(tab.tabId)">×</span>
          </button>
        </div>

        <div class="terminal-stack">
          <div v-if="tabList.length === 0" class="empty-terminal">
            Select a profile and project, then start a native CLI session.
          </div>
          <NativeCliTerminal
            v-for="tab in tabList"
            :key="tab.tabId"
            :ref="el => setTerminalRef(tab.tabId, el)"
            :tab-id="tab.tabId"
            :active="workbench.tabs.activeTabId === tab.tabId"
          />
        </div>

        <footer v-if="activeTab" class="run-controls">
          <span>
            {{ activeTab.cli }} · {{ activeTab.profileId }}@{{ activeTab.profileRevision }}
            · gen {{ activeTab.generation }}
          </span>
          <span v-if="activeTab.errorCode" class="run-error">{{ activeTab.errorCode }}</span>
          <button
            v-if="activeTab.status === 'unknown'"
            @click="recoverActive"
          >
            Recover
          </button>
          <button
            v-if="['running', 'starting', 'unknown'].includes(activeTab.status)"
            @click="stopActive"
          >
            Stop
          </button>
          <button
            v-if="['stopped', 'failed', 'exited'].includes(activeTab.status)"
            @click="restartActive"
          >
            Restart
          </button>
        </footer>
      </main>

      <aside class="resource-panel">
        <div class="resource-header">
          <strong>Resources</strong>
          <select v-model="resourceKind" class="select">
            <option v-for="kind in resourceKinds" :key="kind" :value="kind">{{ kind }}</option>
          </select>
          <button :disabled="workbench.workspace.status !== 'ready'" @click="loadResource">
            Refresh
          </button>
        </div>

        <div v-if="workbench.workspace.error" class="resource-error">
          {{ workbench.workspace.error }}
        </div>
        <div v-else-if="workbench.workspace.resource?.state === 'unavailable'" class="resource-error">
          {{ workbench.workspace.resource.reason ?? 'RESOURCE_UNAVAILABLE' }}
        </div>
        <pre v-else class="resource-content">{{ resourceText }}</pre>
      </aside>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import { selectDirectory } from '@/api/tauri'
import type { NativeCliKind } from '@/types/cli'
import type { ResourceKind } from '@/types/nativeProjection'
import type { RegisteredProject } from '@/types/workspace'
import { useNativeWorkbenchStore } from '@/stores/nativeWorkbench'
import NativeCliTerminal from '@/components/NativeCliTerminal.vue'

defineEmits<{ back: [] }>()

const nativeClis: NativeCliKind[] = ['claude', 'codex']
const resourceKinds: ResourceKind[] = [
  'history',
  'config',
  'mcp',
  'skills',
  'agents',
  'plugins',
  'instructions',
]

const workbench = useNativeWorkbenchStore()
const resumeId = ref('')
const resourceKind = ref<ResourceKind>('history')
const terminalRefs = new Map<string, any>()

const tabList = computed(() => [...workbench.tabs.tabs.values()])
const activeTab = computed(() => {
  const id = workbench.tabs.activeTabId
  return id ? workbench.tabs.tab(id) ?? null : null
})
const canCreate = computed(() =>
  workbench.status === 'ready'
  && Boolean(workbench.profiles.selected[workbench.cli])
  && Boolean(workbench.selectedProject),
)
const resourceText = computed(() => {
  const result = workbench.workspace.resource
  if (!result || result.state !== 'ready') return ''
  return JSON.stringify(result.items, null, 2)
})

function setTerminalRef(tabId: string, value: unknown) {
  if (value) terminalRefs.set(tabId, value)
  else terminalRefs.delete(tabId)
}

function projectLabel(project: RegisteredProject): string {
  const metadata = workbench.workspace.projects.find(p => p.projectId === project.projectId)
  if (metadata?.alias.mode === 'set' && metadata.alias.value) return metadata.alias.value
  return project.selectedPath
}

async function switchCli(cli: NativeCliKind) {
  await workbench.selectCli(cli).catch(() => {})
}

async function changeProfile(event: Event) {
  const id = (event.target as HTMLSelectElement).value
  if (!id) return
  await workbench.selectProfile(workbench.cli, id).catch(() => {})
}

function changeProject(event: Event) {
  const id = (event.target as HTMLSelectElement).value
  if (id) workbench.selectProject(id)
}

async function addProject() {
  const result = await selectDirectory()
  if (!result) return
  await workbench.workspace.open(workbench.cli).catch(() => {})
  const added = workbench.workspace.projects.find(project => project.selectedPath === result.path)
  if (added) workbench.selectProject(added.projectId)
}

function createNew() {
  workbench.createTab({ kind: 'new' })
}

function createPicker() {
  workbench.createTab({ kind: 'resume-picker', scope: 'current-project' })
}

function createKnownResume() {
  const id = resumeId.value.trim()
  if (!id) return
  workbench.createTab({ kind: 'resume-id', nativeSessionId: id })
  resumeId.value = ''
}

async function recoverActive() {
  const tab = activeTab.value
  if (!tab) return
  await terminalRefs.get(tab.tabId)?.recover?.()
}

async function stopActive() {
  const tab = activeTab.value
  if (!tab) return
  await terminalRefs.get(tab.tabId)?.stop?.().catch(() => {})
}

async function restartActive() {
  const tab = activeTab.value
  if (!tab) return
  try {
    workbench.restartTab(tab.tabId)
    await nextTick()
    terminalRefs.get(tab.tabId)?.focus?.()
  } catch {
    // Store keeps a safe error/status; restart never creates a replacement for unknown state.
  }
}

async function closeTab(tabId: string) {
  const tab = workbench.tabs.tab(tabId)
  if (!tab) return
  if (['running', 'starting', 'unknown'].includes(tab.status)) {
    try {
      await terminalRefs.get(tabId)?.stop?.()
    } catch {
      return
    }
  }
  workbench.closeTab(tabId)
}

async function loadResource() {
  await workbench.workspace.loadResource(resourceKind.value).catch(() => {})
}

onMounted(async () => {
  await workbench.initialize('codex').catch(() => {})
})
</script>

<style scoped>
.native-workbench {
  position: absolute;
  inset: 32px 0 0;
  z-index: 20;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
  color: var(--text-primary);
}

.native-toolbar,
.resource-header,
.run-controls,
.tab-strip {
  display: flex;
  align-items: center;
  gap: 8px;
}

.native-toolbar {
  min-height: 48px;
  padding: 6px 10px;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.native-toolbar button,
.resource-header button,
.run-controls button,
.select,
.resume-input {
  min-height: 30px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-primary);
  padding: 4px 8px;
}

.native-toolbar button,
.resource-header button,
.run-controls button {
  cursor: pointer;
}

.native-toolbar button:disabled,
.resource-header button:disabled,
.run-controls button:disabled {
  cursor: default;
  opacity: 0.45;
}

.cli-switch {
  display: flex;
  gap: 2px;
}

.cli-switch button.active {
  border-color: var(--accent-gold);
  color: var(--accent-gold-text);
}

.project-select {
  min-width: 180px;
  max-width: 320px;
}

.resume-input {
  width: 150px;
}

.state-banner,
.resource-error {
  padding: 8px 12px;
  font-size: 12px;
}

.error,
.resource-error,
.run-error,
.tab-error {
  color: var(--status-error);
}

.native-body {
  display: flex;
  flex: 1;
  min-height: 0;
}

.terminal-area {
  display: flex;
  flex: 1;
  min-width: 0;
  flex-direction: column;
}

.tab-strip {
  min-height: 34px;
  overflow-x: auto;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.tab {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  margin-left: 4px;
  padding: 0 8px;
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
}

.tab.active {
  border-color: var(--border-color);
  background: var(--selected-bg);
  color: var(--text-primary);
}

.tab small {
  opacity: 0.7;
}

.tab-close {
  font-size: 16px;
}

.terminal-stack {
  position: relative;
  flex: 1;
  min-height: 0;
  background: var(--bg-primary);
}

.empty-terminal {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  color: var(--text-secondary);
}

.run-controls {
  min-height: 38px;
  padding: 4px 10px;
  border-top: 1px solid var(--border-color);
  font-size: 12px;
}

.run-controls > span:first-child {
  margin-right: auto;
}

.resource-panel {
  width: 320px;
  min-width: 240px;
  border-left: 1px solid var(--border-color);
  background: var(--bg-secondary);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.resource-header {
  padding: 8px;
  border-bottom: 1px solid var(--border-color);
}

.resource-content {
  flex: 1;
  margin: 0;
  padding: 10px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 11px;
  color: var(--text-secondary);
}
</style>
