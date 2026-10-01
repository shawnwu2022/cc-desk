<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppInput from '@/components/ui/AppInput.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
const { t } = useI18n()
const app = useAppStore()
const widthDraft = ref(String(app.sidebarWidth))
let editingWidth = false
watch(() => app.sidebarWidth, value => { if (!editingWidth) widthDraft.value = String(value) })
function editWidth(value: string) { editingWidth = true; widthDraft.value = value }
function cancelWidth() { editingWidth = false; widthDraft.value = String(app.sidebarWidth) }
function commitWidth() {
  editingWidth = false
  const value = Number(widthDraft.value)
  if (!widthDraft.value.trim() || !Number.isFinite(value)) { cancelWidth(); return }
  const normalized = Math.round(Math.max(240, Math.min(360, value)))
  if (normalized !== app.sidebarWidth) void app.setSidebarWidth(value)
  widthDraft.value = String(app.sidebarWidth)
}
</script>
<template>
  <section class="settings-section" data-settings-appearance :aria-label="t('appearance')">
    <h2>{{ t('appearanceTitle') }}</h2>
    <div class="setting-field">
      <AppSelect data-gui-theme :model-value="app.guiThemeMode" :label="t('theme')"
        :options="[{ value: 'light', label: t('light') }, { value: 'dark', label: t('dark') }, { value: 'system', label: t('settingsFollowSystem') }]" @update:model-value="app.setTheme" />
      <p>{{ t('settingsGuiThemeHint') }}</p>
    </div>
    <AppSelect data-gui-density :model-value="app.guiDensity" :label="t('settingsDensity')"
      :options="[{ value: 'standard', label: t('settingsDensityStandard') }, { value: 'compact', label: t('settingsDensityCompact') }]" @update:model-value="app.setGuiDensity" />
    <div class="setting-field">
      <AppInput data-settings-sidebar-width type="number" :model-value="widthDraft" :label="t('settingsSidebarWidth')" min="240" max="360" step="8" @update:model-value="editWidth" @blur="commitWidth" @keydown.enter.prevent="commitWidth" @keydown.esc.prevent="cancelWidth" />
      <p>{{ t('settingsSidebarWidthHint') }}</p>
    </div>
    <InlineNotice :message="t('settingsGuiAccentsHint')" />
  </section>
</template>
<style scoped>
.settings-section { display: flex; flex-direction: column; gap: 20px; min-width: 0; max-width: 560px; }
h2 { font-size: 20px; font-weight: 600; color: var(--text-primary); }
.setting-field { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
p { font-size: 12px; line-height: 1.5; color: var(--text-secondary); overflow-wrap: anywhere; }
</style>
