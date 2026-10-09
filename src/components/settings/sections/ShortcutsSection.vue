<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import { APP_SHORTCUT_ACTIONS, DEFAULT_SHORTCUT_BINDINGS, SHORTCUT_LABELS, captureShortcut, formatShortcut, type AppShortcutAction, type ShortcutBindings } from '@/config/appShortcuts'
import { isMac } from '@/utils/platform'
import AppButton from '@/components/ui/AppButton.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppDialog from '@/components/ui/AppDialog.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const { t } = useI18n(); const app = useAppStore()
const query = ref(''), capturing = ref<AppShortcutAction | null>(null), resetAll = ref(false), busy = ref(false), error = ref<string | null>(null)
const pending = ref<{ action: AppShortcutAction; binding: string; conflict: AppShortcutAction; baseline: string } | null>(null)
let owner = 0
const ready = ref(false), reading = ref(false)
const blocked = computed(() => !ready.value || reading.value || busy.value || app.shortcutBindingsSaving)
async function reload(force = false) {
  if (!props.active || reading.value) return
  const version = owner; reading.value = true
  try {
    await app.loadSettingsPreferences(force || app.shortcutBindingsError === 'settingsSaveReloadFailed')
    if (version === owner && props.active) { ready.value = true; error.value = null }
  } catch { if (version === owner && props.active) { ready.value = false; error.value = 'settingsSaveReadFailed' } }
  finally { if (version === owner) reading.value = false }
}
watch(() => app.shortcutBindingsError, value => { if (value === 'settingsSaveReloadFailed') ready.value = false })
const rows = computed(() => APP_SHORTCUT_ACTIONS.map(action => ({ action, label: t(SHORTCUT_LABELS[action]), binding: formatShortcut(app.shortcutBindings[action], isMac) }))
  .filter(row => `${row.label} ${row.binding}`.toLowerCase().includes(query.value.trim().toLowerCase())))
function close() { ++owner; capturing.value = null; pending.value = null; resetAll.value = false; busy.value = false; error.value = null }
watch(() => props.active, active => { if (!active) { close(); reading.value = false } else void reload() }, { immediate: true, flush: 'sync' }); onBeforeUnmount(close)
function begin(action: AppShortcutAction) { if (!props.active || blocked.value) return; close(); capturing.value = action }
async function save(bindings: ShortcutBindings) {
  if (!props.active || blocked.value) return
  const version = owner; busy.value = true; error.value = null
  const saved = await app.setShortcutBindings(bindings)
  if (version !== owner || !props.active) return
  busy.value = false
  if (saved) close()
  else error.value = app.settingsSaveError ?? 'settingsSaveFailed'
}
function propose(action: AppShortcutAction, binding: string) {
  if (!props.active || blocked.value) return
  const conflict = APP_SHORTCUT_ACTIONS.find(other => other !== action && app.shortcutBindings[other] === binding)
  if (conflict) { capturing.value = action; pending.value = { action, binding, conflict, baseline: JSON.stringify(app.shortcutBindings) }; error.value = null }
  else void save({ ...app.shortcutBindings, [action]: binding })
}
function capture(event: KeyboardEvent) {
  if (event.key === 'Escape' || event.key === 'Tab' || (!event.ctrlKey && !event.metaKey && !event.altKey && ['Enter', ' '].includes(event.key) && event.target instanceof HTMLElement && event.target.closest('button'))) return
  event.preventDefault(); event.stopPropagation()
  if (!capturing.value || busy.value) return
  const binding = captureShortcut(event)
  if (!binding) { if (!['Control', 'Meta', 'Alt', 'Shift'].includes(event.key)) error.value = 'shortcutBindingInvalid'; return }
  propose(capturing.value, binding)
}
function replace() {
  const choice = pending.value
  if (!choice || blocked.value) return
  if (choice.baseline !== JSON.stringify(app.shortcutBindings)) { pending.value = null; error.value = 'shortcutBindingsChanged'; return }
  void save({ ...app.shortcutBindings, [choice.action]: choice.binding, [choice.conflict]: null })
}
function reset(action: AppShortcutAction) { if (blocked.value) return; close(); propose(action, DEFAULT_SHORTCUT_BINDINGS[action]!) }
function requestResetAll() { if (props.active && !blocked.value) { close(); resetAll.value = true } }
</script>
<template>
  <section class="remaining-settings" data-settings-shortcuts>
    <h2>{{ t('keyboardShortcuts') }}</h2>
    <p class="settings-hint">{{ t('shortcutScopeHint') }}</p>
    <AppInput v-model="query" data-shortcut-search :label="t('shortcutSearch')" />
    <div class="shortcut-list">
      <div v-for="row in rows" :key="row.action" class="shortcut-row" :data-shortcut-row="row.action">
        <span class="shortcut-label">{{ row.label }}</span>
        <AppButton :data-shortcut-edit="row.action" :aria-label="t('shortcutEdit', { action: row.label })" :disabled="blocked" @click="begin(row.action)"><kbd>{{ row.binding }}</kbd></AppButton>
        <AppButton variant="ghost" size="compact" :data-shortcut-reset="row.action" :disabled="blocked" @click="reset(row.action)">{{ t('restoreDefault') }}</AppButton>
      </div>
    </div>
    <InlineNotice v-if="error && !capturing && !resetAll" kind="warning" :message="t(error)" />
    <AppButton v-if="!ready" :disabled="reading" @click="reload(true)">{{ t('refresh') }}</AppButton>
    <AppButton data-shortcut-reset-all variant="secondary" :disabled="blocked" @click="requestResetAll">{{ t('shortcutResetAll') }}</AppButton>
    <AppDialog :open="!!capturing && active" data-shortcut-capture :title="t('shortcutCaptureTitle')" :description="t('shortcutCaptureHint')" @close="close" @keydown="capture">
      <p v-if="capturing">{{ t(SHORTCUT_LABELS[capturing]) }}</p>
      <InlineNotice v-if="pending" data-shortcut-conflict kind="warning" :message="t('shortcutConflict', { key: formatShortcut(pending.binding, isMac), action: t(SHORTCUT_LABELS[pending.conflict]) })" />
      <InlineNotice v-if="error" kind="warning" :message="t(error)" :action-label="!ready ? t('refresh') : undefined" @action="reload(true)" />
      <template #footer><AppButton @click="close">{{ t('cancel') }}</AppButton><AppButton v-if="pending" data-shortcut-replace variant="danger" :disabled="blocked" @click="replace">{{ t('shortcutReplace') }}</AppButton></template>
    </AppDialog>
    <AppDialog :open="resetAll && active" :title="t('shortcutResetAll')" :description="t('shortcutResetAllConfirm')" @close="close">
      <InlineNotice v-if="error" kind="warning" :message="t(error)" :action-label="!ready ? t('refresh') : undefined" @action="reload(true)" />
      <template #footer><AppButton @click="close">{{ t('cancel') }}</AppButton><AppButton data-shortcut-reset-confirm variant="danger" :disabled="blocked" @click="save({ ...DEFAULT_SHORTCUT_BINDINGS })">{{ t('restoreDefault') }}</AppButton></template>
    </AppDialog>
  </section>
</template>
<style scoped>
.remaining-settings { display: flex; flex-direction: column; align-items: flex-start; gap: 16px; min-width: 0; width: 100%; max-width: 760px; }
h2 { color: var(--text-primary); font-size: 20px; }
.settings-hint { color: var(--text-secondary); font-size: 12px; line-height: 1.6; }
.shortcut-list { width: 100%; display: grid; gap: 6px; }
.shortcut-row { display: grid; grid-template-columns: minmax(0, 1fr) minmax(90px, auto) auto; gap: 8px; align-items: center; min-width: 0; }
.shortcut-label { overflow-wrap: anywhere; font-size: 13px; color: var(--text-primary); }
kbd { font-family: var(--font-mono); overflow-wrap: anywhere; }
@media (max-width: 760px) { .shortcut-row { grid-template-columns: minmax(0, 1fr) auto; } .shortcut-row > :last-child { grid-column: 2; } }
</style>
