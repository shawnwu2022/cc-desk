<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import { useCliSupportStore } from '@/stores/cliSupport'
import AppSelect from '@/components/ui/AppSelect.vue'
import IconButton from '@/components/ui/IconButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import AppButton from '@/components/ui/AppButton.vue'
import { useSessionIndexMaintenanceStore } from '@/stores/sessionIndexMaintenance'
import claudeIcon from '@/assets/icons/cli/claude.svg'
import codexIcon from '@/assets/icons/cli/codex.svg'
const { t } = useI18n()
const app = useAppStore()
const support = useCliSupportStore()
const indexMaintenance = useSessionIndexMaintenanceStore()
const indexStateLabels = { idle:'indexRebuildTitle',preparing:'indexRebuildState_preparing',scanning:'indexRebuildState_scanning',committing:'indexRebuildState_committing',completed:'indexRebuildState_completed',cancelled:'indexRebuildState_cancelled',failed:'indexRebuildState_failed' }
const cliName = { claude: 'Claude Code', codex: 'Codex CLI' } as const
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
    <div class="setting-field">
      <span class="ui-field-label">{{ t('settingsCliSupport') }}</span>
      <div class="cli-support-row" role="group" :aria-label="t('settingsCliSupport')">
        <IconButton v-for="cli in (['claude', 'codex'] as const)" :key="cli" :data-cli-support="cli"
          :label="t(support.isEnabled(cli) ? 'settingsCliClickToDisable' : 'settingsCliClickToEnable', { cli: cliName[cli] })"
          :aria-pressed="support.isEnabled(cli)" class="cli-support-toggle"
          :class="{ 'cli-support-toggle--off': !support.isEnabled(cli) }"
          @click="support.setCliEnabled(cli, !support.isEnabled(cli))">
          <img :src="cli === 'claude' ? claudeIcon : codexIcon" width="16" height="16" alt="" />
        </IconButton>
      </div>
      <p>{{ t('settingsCliSupportHint') }}</p>
    </div>
    <div class="setting-field" data-close-window-behavior><h3>{{ t('settingsCloseWindow') }}</h3><p>{{ t('settingsCloseWindowHint') }}</p></div>
    <InlineNotice :message="t('settingsSavedLaunchOptionsHint')" />
    <div class="setting-field" data-session-index-maintenance>
      <span class="ui-field-label">{{ t('indexRebuildTitle') }}</span>
      <p>{{ t('indexRebuildHint') }}</p>
      <div class="cli-support-row">
        <AppButton :disabled="indexMaintenance.busy" @click="indexMaintenance.run">{{ t(indexMaintenance.needsReconciliation ? 'indexRebuildCheck' : indexMaintenance.needsRefresh ? 'refresh' : 'indexRebuildTitle') }}</AppButton>
        <AppButton v-if="indexMaintenance.canCancel" @click="indexMaintenance.cancel">{{ t('cancel') }}</AppButton>
      </div>
      <p role="status" aria-live="polite" v-if="indexMaintenance.state !== 'idle'">{{ t(indexStateLabels[indexMaintenance.state]) }} <span v-if="indexMaintenance.busy">{{ indexMaintenance.done }}/{{ indexMaintenance.total }}</span></p>
      <InlineNotice v-if="indexMaintenance.error" kind="warning" :message="t(indexMaintenance.error)" />
    </div>
  </section>
</template>
<style scoped>
.settings-section { display: flex; flex-direction: column; gap: 20px; min-width: 0; max-width: 560px; }
h2 { font-size: 20px; font-weight: 600; color: var(--text-primary); }
h3 { font-size: 13px; font-weight: 500; color: var(--text-primary); }
.setting-field { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
p { font-size: 12px; line-height: 1.5; color: var(--text-secondary); overflow-wrap: anywhere; }
.cli-support-row { display: flex; gap: 10px; }
.cli-support-toggle img { display: block; }
/* 灰态 = 停用：沿用 SessionAppIcon 的 grayscale 先例，禁用控件透明度规范 .5。 */
.cli-support-toggle--off { opacity: 0.5; }
.cli-support-toggle--off img { filter: grayscale(1); }
</style>
