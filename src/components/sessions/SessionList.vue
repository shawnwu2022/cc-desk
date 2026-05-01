<template>
  <div class="session-list">
    <SessionItem
      v-for="item in items"
      :key="item.id"
      :id="item.id"
      :name="item.name"
      :is-active="item.id === activeId"
      :is-running="item.isRunning"
      :is-stopped="item.isStopped"
      :last-active-at="item.lastActiveAt"
      :closable="closable && item.isTab"
      :snippet="item.snippet"
      @switch="(id) => $emit('switch', id)"
      @rename="(id, name) => $emit('rename', id, name)"
      @restart="(id) => $emit('restart', id)"
      @close="(id) => $emit('close', id)"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import SessionItem from './SessionItem.vue'
import type { TerminalTab, HistorySession } from '@/stores/session'

const props = defineProps<{
  tabs?: TerminalTab[]
  history?: HistorySession[]
  activeId: string | null
  runningTabIds?: string[]
  closable?: boolean
  snippetMap?: Map<string, string>
}>()

defineEmits<{
  switch: [id: string]
  rename: [id: string, name: string]
  restart: [id: string]
  close: [id: string]
}>()

interface ListItem {
  id: string
  name: string
  isRunning: boolean
  isStopped: boolean
  isTab: boolean
  lastActiveAt: number
  snippet?: string
}

const items = computed<ListItem[]>(() => {
  const snippets = props.snippetMap

  const tabItems: ListItem[] = (props.tabs ?? []).map(tab => ({
    id: tab.tabId,
    name: tab.name,
    isRunning: tab.status === 'running',
    isStopped: tab.status === 'stopped',
    isTab: true,
    lastActiveAt: tab.lastActiveAt,
  }))

  const historyItems: ListItem[] = (props.history ?? []).map(s => ({
    id: s.sessionId,
    name: s.name,
    isRunning: false,
    isStopped: false,
    isTab: false,
    lastActiveAt: s.lastActiveAt,
    snippet: snippets?.get(s.sessionId),
  }))

  return [...tabItems, ...historyItems]
})
</script>

<style scoped>
.session-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
</style>
