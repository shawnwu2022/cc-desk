<script lang="ts">
let nextFormId = 0
</script>
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppButton from '@/components/ui/AppButton.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import { useNewSessionDraftStore, type NewSessionStartMode } from '@/stores/newSessionDraft'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import type { CreateUnifiedSessionInput, UnifiedCliKind, UnifiedProjectIdentity } from '@/types/unifiedSession'
const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const emit = defineEmits<{ create: [input: CreateUnifiedSessionInput]; restore: [request: { project: UnifiedProjectIdentity; cli: UnifiedCliKind; mode: Exclude<NewSessionStartMode, 'new'>; launchConfigId?: string; launchConfigRevision?: string }] }>()
const { t } = useI18n()
const draft = useNewSessionDraftStore()
const profiles = useCliProfilesStore()
const error = ref('')
const formId = `new-session-form-${++nextFormId}`
const configurations = computed(() => [{ value: '', label: t('newSessionAutomaticConfig') }, ...profiles.byCli[draft.cli].map(profile => ({ value: profile.id, label: profile.name }))])
const selected = computed(() => draft.launchConfigId ? profiles.profile(draft.launchConfigId) : draft.project ? draft.preferred(draft.project, draft.cli) : null)
const permissionText = computed(() => draft.rawEnabled && draft.startMode === 'new' ? t('newSessionRawPermissions') : draft.cli === 'codex' ? t('newSessionCodexPermissions')
  : selected.value?.skipPermissions.mode === 'set' && selected.value.skipPermissions.value ? t('newSessionSkipPermissions')
    : selected.value?.id === 'legacyClaude' && selected.value.skipPermissions.mode === 'inherit' ? t('newSessionInheritedPermissions') : t('newSessionStandardPermissions'))
watch(() => [props.active, draft.visible], ([active]) => { if (!active) draft.visible = false }, { flush: 'sync', immediate: true })
watch(() => draft.visible, () => { error.value = '' })
watch(() => draft.cli, () => { draft.launchConfigId = '' })
function format(value: string) { try { draft.setArgvFormat(value as 'lines' | 'json'); error.value = '' } catch { error.value = 'newSessionArgvFormatError' } }
function submit() {
  if (!props.active || !draft.visible || !draft.project) return
  try {
    if (draft.startMode !== 'new') emit('restore', { project: { ...draft.project }, cli: draft.cli, mode: draft.startMode, ...(draft.launchConfigId && selected.value ? { launchConfigId: selected.value.id, launchConfigRevision: selected.value.revision } : {}) })
    else emit('create', draft.toInput())
    draft.visible = false
  } catch { error.value = 'newSessionArgvError' }
}
</script>
<template>
  <AppDialog v-model:open="draft.visible" :title="t('newSessionTitle')" class="new-session-dialog">
    <form :id="formId" class="new-session-fields" @submit.prevent="submit">
      <fieldset><legend>{{ t('newSessionBasic') }}</legend>
        <AppInput :model-value="draft.project?.projectPath ?? ''" :label="t('newSessionProject')" readonly />
        <AppSelect v-model="draft.cli" :label="t('newSessionTool')" :options="[{ value: 'claude', label: 'Claude Code' }, { value: 'codex', label: 'Codex CLI' }]" />
        <AppSelect v-model="draft.startMode" :label="t('newSessionStartMode')" :options="[
          { value: 'new', label: t('newSession') }, { value: 'history', label: t('newSessionRestore') },
          { value: 'resume-picker', label: t('newSessionNativePicker') }, { value: 'resume-id', label: t('newSessionById') }]" />
        <AppSelect v-model="draft.launchConfigId" :label="t('newSessionConfiguration')" :options="configurations" />
        <div class="ui-field"><span class="ui-field-label">{{ t('newSessionPermissions') }}</span><p>{{ permissionText }}</p><p v-if="!draft.rawEnabled || draft.startMode !== 'new'">{{ t('newSessionPermissionsHint') }}</p></div>
        <InlineNotice v-if="!selected && draft.startMode === 'new'" :message="t('newSessionSafeDefaultHint')" />
        <InlineNotice v-if="draft.startMode !== 'new'" :message="t('newSessionRestorePending')" />
      </fieldset>
      <details><summary>{{ t('newSessionMoreOptions') }}</summary><AppInput v-model="draft.title" :label="t('newSessionName')" /></details>
      <details v-if="draft.startMode === 'new'"><summary>{{ t('newSessionDeveloperOptions') }}</summary>
        <label class="raw-toggle"><input v-model="draft.rawEnabled" type="checkbox" />{{ t('newSessionUseRaw') }}</label>
        <template v-if="draft.rawEnabled">
          <AppSelect :model-value="draft.argvFormat" :label="t('newSessionArgvFormat')" :options="[{ value: 'lines', label: t('newSessionArgvLines') }, { value: 'json', label: t('newSessionArgvJson') }]" @update:model-value="format" />
          <label class="ui-field-label" for="new-session-argv">{{ t('newSessionArgv') }}</label>
          <textarea id="new-session-argv" v-model="draft.argvText" class="ui-input" rows="5" spellcheck="false" aria-describedby="new-session-argv-hint" />
          <p id="new-session-argv-hint">{{ t('newSessionArgvHint') }}</p>
        </template>
      </details>
      <InlineNotice v-if="error" kind="error" :message="t(error)" />
    </form>
    <template #footer>
      <AppButton data-create-session type="submit" :form="formId" variant="primary">{{ t(draft.startMode === 'new' ? 'newSessionCreate' : 'newSessionContinueRestore') }}</AppButton>
    </template>
  </AppDialog>
</template>
<style scoped>
.new-session-dialog { width: min(520px, calc(100vw - 32px)); }
.new-session-fields, fieldset { display: flex; flex-direction: column; gap: 14px; min-width: 0; }
fieldset { margin: 0; padding: 0; border: 0; }
legend, summary { font-weight: 600; margin-bottom: 12px; }
summary { cursor: pointer; }
p { color: var(--text-secondary); font-size: 12px; margin: 4px 0; }
.raw-toggle { display: flex; gap: 8px; align-items: center; margin-bottom: 12px; }
textarea { width: 100%; resize: vertical; min-height: 88px; }
</style>
