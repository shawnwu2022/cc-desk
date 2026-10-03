<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useShellStore } from '@/stores/shell'
import IconButton from '@/components/ui/IconButton.vue'
import AppButton from '@/components/ui/AppButton.vue'

withDefaults(defineProps<{ projectTitle?: string; sessionTitle?: string; hasProject?: boolean }>(), { projectTitle: '', sessionTitle: '', hasProject: false })
const emit = defineEmits<{ 'new-session-request': []; 'add-project': [] }>()
const shell = useShellStore()
const { t } = useI18n()
</script>

<template>
  <header class="workspace-header">
    <IconButton :label="t(shell.sidebarVisible ? 'collapseSessions' : 'expandSessions')"
      :aria-expanded="shell.sidebarVisible" @click="shell.toggleSidebar()">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 4h18v16H3zM9 4v16" /></svg>
    </IconButton>
    <div class="workspace-context">
      <span class="context-title project-title" data-project-title :title="projectTitle || t('workspace')">{{ projectTitle || t('workspace') }}</span>
      <span v-if="sessionTitle" aria-hidden="true" class="context-separator">/</span>
      <span v-if="sessionTitle" class="context-title session-title" data-session-title :title="sessionTitle">{{ sessionTitle }}</span>
    </div>
    <AppButton v-if="hasProject" data-new-session size="compact" @click="emit('new-session-request')">{{ t('newSession') }}</AppButton>
    <AppButton v-else size="compact" @click="emit('add-project')">{{ t('addProject') }}</AppButton>
    <IconButton :label="t('contextResources')" :aria-expanded="shell.drawerVisible" @click="shell.toggleDrawer()">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 4h18v16H3zM15 4v16" /></svg>
    </IconButton>
  </header>
</template>

<style scoped>
.workspace-header { display: flex; align-items: center; gap: 8px; padding: 8px 12px; flex-shrink: 0; min-width: 0; border-bottom: 1px solid var(--border-color); }
.workspace-context { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; }
.context-title { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; color: var(--text-primary); }
.project-title { flex: 0 1 auto; font-weight: 600; }
.session-title { flex: 1 1 0; color: var(--text-secondary); }
.context-separator { flex-shrink: 0; color: var(--text-tertiary); }
</style>
