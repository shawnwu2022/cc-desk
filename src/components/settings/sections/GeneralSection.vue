<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import AppSelect from '@/components/ui/AppSelect.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
const { t } = useI18n()
const app = useAppStore()
</script>
<template>
  <section class="settings-section" data-settings-general :aria-label="t('settingsGeneral')">
    <h2>{{ t('settingsGeneral') }}</h2>
    <AppSelect data-settings-language :model-value="app.language" :label="t('language')" :options="[{ value: 'en', label: t('languageEn') }, { value: 'zh', label: t('languageZh') }]" @update:model-value="app.setLanguage" />
    <div class="setting-field">
      <AppSelect data-startup-destination :model-value="app.startupDestination" :label="t('settingsStartupDestination')"
        :options="[{ value: 'workspace', label: t('workspace') }, { value: 'projects', label: t('projects') }]" @update:model-value="app.setStartupDestination" />
      <p>{{ t('settingsStartupHint') }}</p>
    </div>
    <div class="setting-field">
      <AppSelect data-default-new-cli :model-value="app.defaultNewCli" :label="t('settingsDefaultCli')"
        :options="[{ value: 'claude', label: 'Claude Code' }, { value: 'codex', label: 'Codex CLI' }]" @update:model-value="app.setDefaultNewCli" />
      <p>{{ t('settingsDefaultCliHint') }}</p>
    </div>
    <div class="setting-field" data-close-window-behavior><h3>{{ t('settingsCloseWindow') }}</h3><p>{{ t('settingsCloseWindowHint') }}</p></div>
    <InlineNotice :message="t('settingsSavedLaunchOptionsHint')" />
  </section>
</template>
<style scoped>
.settings-section { display: flex; flex-direction: column; gap: 20px; min-width: 0; max-width: 560px; }
h2 { font-size: 20px; font-weight: 600; color: var(--text-primary); }
h3 { font-size: 13px; font-weight: 500; color: var(--text-primary); }
.setting-field { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
p { font-size: 12px; line-height: 1.5; color: var(--text-secondary); overflow-wrap: anywhere; }
</style>
