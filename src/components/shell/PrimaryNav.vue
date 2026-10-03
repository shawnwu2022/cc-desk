<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useShellStore, type ShellSection } from '@/stores/shell'
import IconButton from '@/components/ui/IconButton.vue'
import workspaceIcon from '@/assets/icons/sessions.svg'
import projectsIcon from '@/assets/icons/folder.svg'
import settingsIcon from '@/assets/icons/settings.svg'

const shell = useShellStore()
const { t } = useI18n()
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
      :aria-current="shell.section === destination.section ? 'page' : undefined"
      @click="navigate(destination.section)">
      <img :src="destination.icon" alt="" width="18" height="18" />
    </IconButton>
  </nav>
</template>

<style scoped>
.primary-nav { display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 8px 0; width: 44px; min-width: 0; background: var(--bg-secondary); border-right: 1px solid var(--border-color); }
.primary-nav :deep(.primary-destination) { position: relative; width: 32px; height: 32px; padding: 0; }
.primary-nav :deep(.primary-destination.selected) { background: var(--selected-bg); color: var(--accent-gold-text); }
.primary-nav :deep(.primary-destination.selected::before) { content: ''; position: absolute; left: -6px; top: 5px; bottom: 5px; width: 3px; border-radius: 0 2px 2px 0; background: var(--accent-gold); }
.primary-nav :deep([data-primary-section="settings"]) { margin-top: auto; }
</style>
