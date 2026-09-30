<script setup lang="ts">
import { computed } from 'vue'
import SessionItem from './SessionItem.vue'
import type { TerminalTab, HistorySession } from '@/stores/session'
import { useAttentionStore } from '@/stores/attention'
import { SESSION_MENU_ACTION_DEFINITIONS } from '@/utils/sessionPresentation'
import type { SessionMenuAction, SessionMenuActionVisibility, SessionPrimaryAction, UnifiedSession } from '@/types/unifiedSession'

const props = defineProps<{
  sessions?: UnifiedSession[]
  selectedId?: string | null
  primaryActions?: Readonly<Record<string, SessionPrimaryAction | null>>
  menuActionVisibility?: SessionMenuActionVisibility
  // Temporary data/event boundary for ProjectNode; removed by Task 9.
  tabs?: TerminalTab[]
  history?: HistorySession[]
  activeId?: string | null
  runningTabIds?: string[]
  closable?: boolean
  snippetMap?: Map<string, string>
}>()
const emit = defineEmits<{
  activate: [id: string]
  'primary-action': [id: string, action: SessionPrimaryAction]
  'menu-action': [id: string, action: SessionMenuAction]
  'rename-commit': [id: string, title: string]
  'rename-cancel': [id: string]
  switch: [id: string]
  rename: [id: string, name: string]
  restart: [id: string]
  close: [id: string]
  archive: [id: string]
}>()
interface ListItem {
  session: UnifiedSession
  primaryAction?: SessionPrimaryAction | null
  visibility?: SessionMenuActionVisibility
}
const isLegacy = computed(() => props.sessions === undefined)
const selectedId = computed(() => isLegacy.value ? props.activeId : props.selectedId)
function legacyVisibility(allowed: SessionMenuAction[]): SessionMenuActionVisibility {
  return Object.fromEntries(SESSION_MENU_ACTION_DEFINITIONS.map(({ id }) => [id, allowed.includes(id)]))
}
const items = computed<ListItem[]>(() => {
  if (props.sessions !== undefined) return props.sessions.map((session) => ({
    session, primaryAction: props.primaryActions?.[session.id], visibility: props.menuActionVisibility,
  }))
  const attention = useAttentionStore()
  const tabs = (props.tabs ?? []).map((tab): ListItem => {
    const canResume = !!tab.sessionId
    const needsUser = tab.pending || (tab.ptyId ? !!attention.getItem(tab.ptyId) : false)
    const allowed: SessionMenuAction[] = []
    if (tab.tabId === props.activeId) allowed.push('rename')
    if (tab.status === 'stopped' && canResume) allowed.push('resume', 'restart')
    if (props.closable) allowed.push('close')
    return {
      session: {
        id: tab.tabId, projectKey: tab.projectPath, projectPath: tab.projectPath,
        cli: 'claude', runtime: 'legacy-claude', title: tab.name,
        processState: tab.status, attentionState: needsUser ? 'needs-user' : 'none',
        lastActivityAt: tab.lastActiveAt, archived: false, resumable: canResume,
        adapterSessionId: tab.tabId, nativeSessionId: tab.sessionId,
      },
      primaryAction: tab.status === 'stopped' && canResume ? 'resume' : null,
      visibility: legacyVisibility(allowed),
    }
  })
  const history = (props.history ?? []).map((session): ListItem => ({
    session: {
      id: session.sessionId, projectKey: session.projectPath, projectPath: session.projectPath,
      cli: 'claude', runtime: 'legacy-claude', title: session.name,
      processState: 'stopped', attentionState: 'none', lastActivityAt: session.lastActiveAt,
      archived: false, resumable: true, adapterSessionId: session.sessionId, nativeSessionId: session.sessionId,
    },
    primaryAction: 'resume', visibility: legacyVisibility(['resume', 'archive']),
  }))
  return [...tabs, ...history]
})
function activate(id: string) {
  if (isLegacy.value) emit('switch', id)
  else emit('activate', id)
}
function primaryAction(id: string, action: SessionPrimaryAction) {
  if (!isLegacy.value) { emit('primary-action', id, action); return }
  if (action === 'resume') {
    if (props.tabs?.some((tab) => tab.tabId === id)) emit('restart', id)
    else emit('switch', id)
  }
}
function menuAction(id: string, action: SessionMenuAction) {
  if (!isLegacy.value) { emit('menu-action', id, action); return }
  if (action === 'resume') primaryAction(id, 'resume')
  else if (action === 'restart') emit('restart', id)
  else if (action === 'close') emit('close', id)
  else if (action === 'archive') emit('archive', id)
}
function renameCommit(id: string, title: string) {
  if (isLegacy.value) emit('rename', id, title)
  else emit('rename-commit', id, title)
}
</script>

<template>
  <div class="session-list" role="group">
    <SessionItem v-for="item in items" :key="item.session.id" :session="item.session"
      :selected="item.session.id === selectedId" :primary-action="item.primaryAction"
      :menu-action-visibility="item.visibility" @activate="activate" @primary-action="primaryAction"
      @menu-action="menuAction" @rename-commit="renameCommit" @rename-cancel="emit('rename-cancel', $event)" />
  </div>
</template>

<style scoped>
.session-list { display: flex; flex-direction: column; gap: 2px; }
</style>
