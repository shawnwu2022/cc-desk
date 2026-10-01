<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { SETTINGS_SECTIONS, useSidebarStore, normalizeSettingsSection } from '@/stores/sidebar'
import { useAppStore } from '@/stores/app'
import AppButton from '@/components/ui/AppButton.vue'
import IconButton from '@/components/ui/IconButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import GeneralSection from './sections/GeneralSection.vue'
import AppearanceSection from './sections/AppearanceSection.vue'
import ShortcutsSection from './sections/ShortcutsSection.vue'
import UpdateSection from './sections/UpdateSection.vue'
import AboutSection from './sections/AboutSection.vue'
const emit = defineEmits<{ close: [] }>()
const { t } = useI18n()
const sidebar = useSidebarStore()
const app = useAppStore()
const currentVersion = __APP_VERSION__
const section = computed(() => normalizeSettingsSection(sidebar.activeSettingsSection))
const labels = { general: 'settingsGeneral', appearance: 'appearance', terminal: 'settingsTerminal', 'launch-configurations': 'settingsLaunchConfigurations', shortcuts: 'shortcuts', update: 'update', about: 'about' }
const items = computed(() => SETTINGS_SECTIONS.map(id => ({ id, label: t(labels[id]) })))
</script>
<template>
  <section class="settings-view" :aria-label="t('settings')">
    <nav class="settings-nav" :aria-label="t('settingsCategories')">
      <header class="settings-nav-header"><h1>{{ t('settings') }}</h1><IconButton :label="t('close')" @click="emit('close')"><span aria-hidden="true">×</span></IconButton></header>
      <div class="nav-items">
        <AppButton v-for="item in items" :key="item.id" class="nav-item" variant="ghost" size="normal"
          :data-settings-section="item.id" :aria-current="section === item.id ? 'page' : undefined"
          :class="{ active: section === item.id }" @click="sidebar.activeSettingsSection = item.id">
          <span>{{ item.label }}</span><span v-if="item.id === 'update' && sidebar.updateAvailable" class="nav-badge" :aria-label="t('settingsUpdateAvailable')" />
        </AppButton>
      </div>
      <footer class="nav-footer">CC Desk v{{ currentVersion }}</footer>
    </nav>
    <div class="settings-content" :data-settings-content="section">
      <InlineNotice v-if="app.settingsSaveError" kind="warning" :message="t(app.settingsSaveError)" />
      <GeneralSection v-if="section === 'general'" />
      <AppearanceSection v-else-if="section === 'appearance'" />
      <EmptyState v-else-if="section === 'terminal'" :title="t('settingsTerminal')" :description="t('settingsTerminalPending')" />
      <EmptyState v-else-if="section === 'launch-configurations'" :title="t('settingsLaunchConfigurations')" :description="t('settingsLaunchConfigurationsPending')" />
      <ShortcutsSection v-else-if="section === 'shortcuts'" />
      <UpdateSection v-else-if="section === 'update'" />
      <AboutSection v-else-if="section === 'about'" />
    </div>
  </section>
</template>
<style scoped>
.settings-view { display: grid; grid-template-columns: 180px minmax(0, 1fr); flex: 1; min-width: 0; min-height: 0; height: 100%; max-width: 100%; overflow: hidden; background: var(--bg-primary); }
.settings-nav { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--bg-secondary); border-right: 1px solid var(--border-color); }
.settings-nav-header { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 10px 12px; border-bottom: 1px solid var(--border-color); }
.settings-nav-header h1 { font-size: 15px; font-weight: 600; color: var(--text-primary); }
.nav-items { display: flex; flex: 1; flex-direction: column; gap: 4px; min-height: 0; min-width: 0; overflow-y: auto; padding: 8px; }
.nav-item { display: flex; align-items: center; justify-content: space-between; gap: 6px; min-width: 0; width: 100%; height: auto; min-height: var(--control-height-normal); text-align: left; white-space: normal; overflow-wrap: anywhere; }
.nav-item.active { background: var(--selected-bg); color: var(--accent-gold-text); border-color: var(--selected-border); }
.nav-badge { width: 7px; height: 7px; border-radius: 50%; background: var(--status-info); flex-shrink: 0; }
.nav-footer { color: var(--text-tertiary); font-size: 11px; padding: 12px; border-top: 1px solid var(--border-color); }
.settings-content { display: flex; flex-direction: column; gap: 12px; min-width: 0; min-height: 0; overflow-y: auto; overflow-x: hidden; padding: 24px; }
.settings-content :deep(.section-content) { min-width: 0; max-width: 100%; }
@media (max-width: 760px) {
  .settings-view { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
  .settings-nav { border-right: 0; border-bottom: 1px solid var(--border-color); }
  .nav-items { flex-direction: row; flex-wrap: wrap; overflow: visible; }
  .nav-item { flex: 1 1 140px; width: auto; }
  .nav-footer { display: none; }
  .settings-content { padding: 16px; }
}
</style>
