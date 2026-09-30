<script setup lang="ts">
// Temporary adapter for the development compatibility terminal only. Normal
// AppShell renders PrimaryNav directly and never uses the old resource bar.
import PrimaryNav from '@/components/shell/PrimaryNav.vue'
import { useSidebarStore, type SidebarPanelType } from '@/stores/sidebar'
import type { ShellSection } from '@/stores/shell'
defineProps<{ activePanel?: SidebarPanelType }>()
const emit = defineEmits<{ toggle: [panel: SidebarPanelType]; toggleSettings: []; openFolder: [] }>()
const sidebar = useSidebarStore()
function navigate(section: ShellSection) {
  if (section === 'workspace') emit('toggle', 'sessions')
  else if (section === 'projects') window.dispatchEvent(new Event('app:toggleHome'))
  else sidebar.openSettings()
}
</script>

<template><PrimaryNav @navigate="navigate" /></template>
