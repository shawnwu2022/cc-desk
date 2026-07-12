<template>
  <aside class="sidebar-panel" :class="{ visible }">
    <div class="panel-inner">
      <!-- Sessions 面板 -->
      <SessionsPanel
        v-show="activePanel === 'sessions'"
        @close="$emit('close')"
        @switch-session="$emit('switchSession', $event)"
        @rename-session="(id, name) => $emit('renameSession', id, name)"
        @restart-session="$emit('restartSession')"
        @new-session="$emit('newSession')"
        @resume-session="$emit('resumeSession', $event)"
        @close-tab="$emit('closeTab', $event)"
        @close-all-tabs="$emit('closeAllTabs')"
        @close-other-tabs="$emit('closeOtherTabs')"
        @switch-to-project="$emit('switchToProject', $event)"
        @new-session-in="$emit('newSessionIn', $event)"
        @toggle-expand="$emit('toggleExpand', $event)"
        @close-all-sessions="$emit('closeAllSessionsIn', $event)"
        @toggle-favorite="$emit('toggleFavorite', $event)"
        @open-in-explorer="$emit('openInExplorer', $event)"
        @resume-session-in-project="(p, id, name) => $emit('resumeSessionInProject', p, id, name)"
      />

      <!-- Skills 面板 -->
      <SkillsPanel
        v-show="activePanel === 'skills'"
        @close="$emit('close')"
      />

      <!-- Agents 面板 -->
      <AgentsPanel
        v-show="activePanel === 'agents'"
        @close="$emit('close')"
      />

      <!-- MCP 面板 -->
      <McpPanel
        v-show="activePanel === 'mcp'"
        @close="$emit('close')"
      />

      <!-- Plugins 面板 -->
      <PluginsPanel
        v-show="activePanel === 'plugins'"
        @close="$emit('close')"
      />
    </div>
  </aside>
</template>

<script setup lang="ts">
import type { SidebarPanelType } from '@/stores/sidebar'
import SessionsPanel from '../sessions/SessionsPanel.vue'
import SkillsPanel from '../skills/SkillsPanel.vue'
import AgentsPanel from '../agents/AgentsPanel.vue'
import McpPanel from '../mcp/McpPanel.vue'
import PluginsPanel from '../plugins/PluginsPanel.vue'

defineProps<{
  visible: boolean
  activePanel: SidebarPanelType
}>()

defineEmits<{
  close: []
  switchSession: [tabId: string]
  renameSession: [tabId: string, name: string]
  restartSession: []
  newSession: []
  resumeSession: [sessionId: string]
  closeTab: [tabId: string]
  closeAllTabs: []
  closeOtherTabs: []
  switchToProject: [projectPath: string]
  newSessionIn: [projectPath: string]
  toggleExpand: [projectPath: string]
  closeAllSessionsIn: [projectPath: string]
  toggleFavorite: [projectPath: string]
  openInExplorer: [projectPath: string]
  resumeSessionInProject: [projectPath: string, sessionId: string, name?: string]
}>()
</script>

<style scoped>
.sidebar-panel {
  width: 0;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-color);
  transition: width 0.25s ease;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
}

.sidebar-panel.visible {
  width: 280px;
}

.panel-inner {
  width: 280px;
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
</style>
