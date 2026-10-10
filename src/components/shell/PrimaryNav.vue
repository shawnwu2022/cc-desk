<script setup lang="ts">
import { computed, getCurrentInstance } from 'vue'
import { useI18n } from 'vue-i18n'
import { useShellStore, type ShellSection } from '@/stores/shell'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import IconButton from '@/components/ui/IconButton.vue'
import workspaceIcon from '@/assets/icons/sessions.svg'
import projectsIcon from '@/assets/icons/folder.svg'
import settingsIcon from '@/assets/icons/settings.svg'

const shell = useShellStore()
const catalog = useUnifiedSessionsStore()
const { t } = useI18n()
const workspaceStatusDescriptionId = `workspace-status-${getCurrentInstance()!.uid}`
const workspaceStatus = computed(() => {
  let errors = 0, permissions = 0
  for (const session of catalog.sessions) {
    if (session.archived || session.processState !== 'running') continue
    if (session.attentionKind === 'error' || session.activityState === 'error') ++errors
    else if (session.attentionKind === 'permission' || session.activityState === 'waiting_permission') ++permissions
  }
  if (errors > 0) return { kind: 'error', count: errors, labelKey: 'workspaceErrorCount' }
  if (permissions > 0) return { kind: 'permission', count: permissions, labelKey: 'workspacePermissionCount' }
  return null
})
const emit = defineEmits<{ navigate: [section: ShellSection] }>()
const destinations = [
  { section: 'workspace', label: 'workspace', icon: workspaceIcon },
  { section: 'projects', label: 'projects', icon: projectsIcon },
  { section: 'settings', label: 'settings', icon: settingsIcon },
] as const
function navigate(section: ShellSection) {
  shell.navigate(section)
  emit('navigate', section)
}
</script>

<template>
  <nav class="primary-nav" :aria-label="t('primaryNavigation')">
    <IconButton v-for="destination in destinations" :key="destination.section"
      class="primary-destination" :class="{ selected: shell.section === destination.section }"
      :label="t(destination.label)" :data-primary-section="destination.section"
      :aria-describedby="destination.section === 'workspace' && workspaceStatus ? workspaceStatusDescriptionId : undefined"
      :aria-current="shell.section === destination.section ? 'page' : undefined"
      @click="navigate(destination.section)">
      <img :src="destination.icon" alt="" width="18" height="18" />
      <span v-if="destination.section === 'workspace' && workspaceStatus" class="workspace-status-badge"
        :class="`workspace-status-badge--${workspaceStatus.kind}`" data-workspace-status-badge
        :data-status-kind="workspaceStatus.kind" aria-hidden="true" />
    </IconButton>
    <span v-if="workspaceStatus" :id="workspaceStatusDescriptionId" class="ui-sr-only">{{ t(workspaceStatus.labelKey, { count: workspaceStatus.count }) }}</span>
  </nav>
</template>

<style scoped>
.primary-nav { display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 8px 0; width: 44px; min-width: 0; background: var(--bg-secondary); border-right: 1px solid var(--border-color); }
.primary-nav :deep(.primary-destination) { position: relative; width: 32px; height: 32px; padding: 0; }
.primary-nav :deep(.primary-destination.selected) { background: var(--selected-bg); color: var(--accent-gold-text); }
.primary-nav :deep(.primary-destination.selected::before) { content: ''; position: absolute; left: -6px; top: 5px; bottom: 5px; width: 3px; border-radius: 0 2px 2px 0; background: var(--accent-gold); }
.primary-nav :deep([data-primary-section="settings"]) { margin-top: auto; }
.workspace-status-badge { position: absolute; top: 2px; right: 2px; width: 8px; height: 8px;
  border-radius: 50%; pointer-events: none; }
.workspace-status-badge--error { background: var(--status-error); }
.workspace-status-badge--permission { background: var(--accent-gold); }
</style>
