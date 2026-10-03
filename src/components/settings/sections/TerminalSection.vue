<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import { TERMINAL_THEMES } from '@/config/terminalThemes'
import { TERMINAL_FONTS, normalizeTerminalFontSize, normalizeTerminalLineHeight } from '@/config/terminalPreferences'
import AppButton from '@/components/ui/AppButton.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppInput from '@/components/ui/AppInput.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import TerminalThemePreview from '../TerminalThemePreview.vue'
const { t } = useI18n()
const app = useAppStore()
const fonts = computed(() => TERMINAL_FONTS.map(value => ({ value, label: value === 'system' ? t('terminalSystemFont') : value })))
const sizeDraft = ref(String(app.fontSize)), heightDraft = ref(String(app.terminalLineHeight))
let editingSize = false, editingHeight = false
watch(() => app.fontSize, value => { if (!editingSize) sizeDraft.value = String(value) })
watch(() => app.terminalLineHeight, value => { if (!editingHeight) heightDraft.value = String(value) })
function editSize(value: string) { editingSize = true; sizeDraft.value = value }
function editHeight(value: string) { editingHeight = true; heightDraft.value = value }
function cancelSize() { editingSize = false; sizeDraft.value = String(app.fontSize) }
function cancelHeight() { editingHeight = false; heightDraft.value = String(app.terminalLineHeight) }
function commitSize() {
  editingSize = false
  if (!sizeDraft.value.trim() || !Number.isFinite(Number(sizeDraft.value))) { cancelSize(); return }
  const value = normalizeTerminalFontSize(Number(sizeDraft.value))
  if (value !== app.fontSize) void app.setFontSize(value)
  sizeDraft.value = String(app.fontSize)
}
function commitHeight() {
  editingHeight = false
  if (!heightDraft.value.trim() || !Number.isFinite(Number(heightDraft.value))) { cancelHeight(); return }
  const value = normalizeTerminalLineHeight(Number(heightDraft.value))
  if (value !== app.terminalLineHeight) void app.setTerminalLineHeight(value)
  heightDraft.value = String(app.terminalLineHeight)
}
</script>
<template>
  <section class="terminal-settings" data-settings-terminal :aria-label="t('settingsTerminal')">
    <h2>{{ t('settingsTerminal') }}</h2>
    <p class="settings-hint">{{ t('terminalSharedPreferencesHint') }}</p>
    <fieldset class="terminal-theme-fieldset">
      <legend>{{ t('terminalTheme') }}</legend>
      <div class="terminal-theme-grid">
        <AppButton v-for="theme in TERMINAL_THEMES" :key="theme.id" variant="ghost" class="terminal-theme-card" :data-terminal-theme="theme.id"
          :aria-pressed="app.terminalTheme === theme.id" @click="app.setTerminalTheme(theme.id)">
          <span class="theme-card-colors" :style="{ backgroundColor: theme.colors.background, color: theme.colors.foreground }" aria-hidden="true">
            Aa <span :style="{ color: theme.colors.green }">✓</span> <span :style="{ color: theme.colors.yellow }">›</span>
          </span>
          <span class="theme-card-name">{{ theme.name }}</span>
        </AppButton>
      </div>
    </fieldset>
    <TerminalThemePreview :preferences="app.terminalPreferences" />
    <AppSelect data-terminal-font-family :label="t('terminalFontFamily')" :model-value="app.terminalFontFamily" :options="fonts" @update:model-value="app.setTerminalFontFamily" />
    <p class="settings-hint">{{ t('terminalFontFallbackHint') }}</p>
    <div class="terminal-metrics">
      <AppInput data-terminal-font-size type="number" :label="t('fontSize')" :model-value="sizeDraft" min="10" max="24" step="1"
        @update:model-value="editSize" @blur="commitSize" @keydown.enter.prevent="commitSize" @keydown.esc.prevent="cancelSize" />
      <AppInput data-terminal-line-height type="number" :label="t('terminalLineHeight')" :model-value="heightDraft" min="1" max="2" step="0.05"
        @update:model-value="editHeight" @blur="commitHeight" @keydown.enter.prevent="commitHeight" @keydown.esc.prevent="cancelHeight" />
    </div>
    <AppSelect data-terminal-cursor-style :label="t('terminalCursorStyle')" :model-value="app.terminalCursorStyle"
      :options="[{ value: 'bar', label: t('terminalCursorBar') }, { value: 'block', label: t('terminalCursorBlock') }, { value: 'underline', label: t('terminalCursorUnderline') }]" @update:model-value="app.setTerminalCursorStyle" />
    <AppSelect data-terminal-cursor-blink :label="t('terminalCursorBlink')" :model-value="String(app.terminalCursorBlink)"
      :options="[{ value: 'true', label: t('terminalBlinkOn') }, { value: 'false', label: t('terminalBlinkOff') }]" @update:model-value="app.setTerminalCursorBlink($event === 'true')" />
    <AppSelect data-terminal-renderer :label="t('terminalRenderer')" :model-value="app.webglRenderer ? 'webgl' : 'dom'"
      :options="[{ value: 'dom', label: t('terminalRendererDom') }, { value: 'webgl', label: t('terminalRendererWebgl') }]" @update:model-value="app.setWebglRenderer($event === 'webgl')" />
    <InlineNotice :message="t('terminalRendererNextOpen')" />
  </section>
</template>
<style scoped>
.terminal-settings { display: flex; flex-direction: column; gap: 16px; min-width: 0; max-width: 720px; }
h2 { font-size: 20px; font-weight: 600; color: var(--text-primary); }
.settings-hint { margin: 0; font-size: 12px; line-height: 1.5; color: var(--text-secondary); overflow-wrap: anywhere; }
.terminal-theme-fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
legend { margin-bottom: 8px; font-size: 13px; font-weight: 600; color: var(--text-primary); }
.terminal-theme-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(130px, 100%), 1fr)); gap: 8px; min-width: 0; }
.terminal-theme-card { display: flex; flex-direction: column; gap: 5px; height: auto; padding: 6px; min-width: 0; text-align: left; }
.terminal-theme-card[aria-pressed="true"] { border-color: var(--selected-border); background: var(--selected-bg); }
.theme-card-colors { display: block; width: 100%; padding: 8px; border-radius: 4px; box-sizing: border-box; font-family: monospace; font-size: 15px; }
.theme-card-name { width: 100%; font-size: 11px; overflow-wrap: anywhere; white-space: normal; }
.terminal-metrics { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; min-width: 0; }
@media (max-width: 560px) { .terminal-metrics { grid-template-columns: minmax(0, 1fr); } }
</style>
