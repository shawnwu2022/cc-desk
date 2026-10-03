<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useShellStore } from '@/stores/shell'
import TitleBar from '@/components/TitleBar.vue'
import PrimaryNav from './PrimaryNav.vue'
import AppDrawer from '@/components/ui/AppDrawer.vue'
import IconButton from '@/components/ui/IconButton.vue'

withDefaults(defineProps<{ title?: string; contextTitle?: string }>(), { title: 'CC Desk' })
const shell = useShellStore()
const { t } = useI18n()
const showSidebar = computed(() => shell.section === 'workspace' && shell.sidebarVisible)
const showContext = computed(() => shell.section === 'workspace' && shell.drawerVisible)
const inlineContext = computed(() => showContext.value && shell.responsiveMode === 'wide')
const columnStyle = computed(() => ({
  '--session-column-width': showSidebar.value ? `${shell.sidebarWidth}px` : '0px',
  '--context-column-width': inlineContext.value ? `${shell.drawerWidth}px` : '0px',
}))
function resize() { shell.setViewportWidth(window.innerWidth) }
onMounted(() => { resize(); window.addEventListener('resize', resize) })
onUnmounted(() => { window.removeEventListener('resize', resize) })
</script>

<template>
  <div class="app-shell" :data-responsive-mode="shell.responsiveMode">
    <TitleBar :title="title" />
    <div class="shell-columns" :style="columnStyle">
      <PrimaryNav />
      <aside v-show="showSidebar" class="shell-sidebar" data-session-column :aria-label="t('sessions')">
        <slot name="sidebar" />
      </aside>
      <main class="shell-main"><slot /></main>
      <aside v-if="inlineContext" class="shell-context" data-inline-context :aria-label="contextTitle ?? t('contextResources')">
        <header class="context-header">
          <h2>{{ contextTitle ?? t('contextResources') }}</h2>
          <IconButton :label="t('close')" @click="shell.drawerVisible = false"><span>×</span></IconButton>
        </header>
        <div class="context-content"><slot name="context" /></div>
      </aside>
    </div>
    <AppDrawer :open="showContext && !inlineContext" :title="contextTitle ?? t('contextResources')"
      :style="{ width: `${shell.drawerWidth}px`, maxWidth: 'calc(100vw - 44px)' }"
      @update:open="shell.drawerVisible = $event">
      <slot name="context" />
    </AppDrawer>
  </div>
</template>

<style scoped>
.app-shell { display: flex; flex-direction: column; height: 100vh; height: 100dvh; width: 100%; min-width: 0; min-height: 0; overflow: hidden; background: var(--bg-primary); }
.shell-columns { display: grid; grid-template-columns: 44px var(--session-column-width) minmax(0, 1fr) var(--context-column-width); flex: 1; min-width: 0; min-height: 0; overflow: hidden; }
.shell-sidebar { grid-column: 2; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; background: var(--bg-secondary); border-right: 1px solid var(--border-color); }
.shell-main { grid-column: 3; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
.shell-context { grid-column: 4; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; background: var(--bg-secondary); border-left: 1px solid var(--border-color); }
.context-header { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 8px 12px; min-width: 0; border-bottom: 1px solid var(--border-color); }
.context-header h2 { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 13px; }
.context-content { flex: 1; min-width: 0; min-height: 0; overflow: auto; padding: 12px; }
</style>
