<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useNotificationsStore } from '@/stores/notifications'
import { argvFromLines } from '@/stores/newSessionDraft'
import { parseNativeRawArgv } from '@/utils/nativeRawArgv'
import { createNativeId } from '@/utils/nativeId'
import { mapSafeUserError, safeUserErrorCode, type UserErrorPresentation } from '@/utils/userError'
import type { CliProfile, LaunchConfigurationEditorRequest, ProfileChanges, ProfileOverride, ShellDialect } from '@/types/profile'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import ErrorDetails from '@/components/ui/ErrorDetails.vue'
const props = withDefaults(defineProps<{ request: LaunchConfigurationEditorRequest; active?: boolean }>(), { active: true })
const emit = defineEmits<{ close: [] }>()
const { t } = useI18n()
const profiles = useCliProfilesStore()
const request = props.request
const original = request.kind === 'create' ? undefined : profiles.profile(request.profileId)
const workspaceRevision = profiles.revision
const source = original ? { id: original.id, revision: original.revision } : undefined
const cli = request.kind === 'create' ? request.cli : original?.cli === 'codex' ? 'codex' : 'claude'
const createdId = request.kind === 'create' || request.kind === 'copy' ? createNativeId('configuration') : undefined
const name = ref(request.kind === 'copy' ? `${original?.name ?? ''} ${t('launchConfigCopySuffix')}` : original?.name ?? (cli === 'claude' ? 'Claude Code' : 'Codex CLI'))
const pathMode = ref(original?.programPath.mode ?? 'inherit')
const pathText = ref(original?.programPath.mode === 'set' ? original.programPath.value : '')
const argsMode = ref(original?.defaultArgs.mode ?? 'set')
const initialArgs = original?.defaultArgs.mode === 'set' ? original.defaultArgs.value : []
const needsJson = (values: string[]) => values.some(value => /[\r\n]/.test(value)) || (values.length === 1 && values[0] === '')
const format = ref<'lines' | 'json'>(needsJson(initialArgs) ? 'json' : 'lines')
const argsText = ref(format.value === 'json' ? JSON.stringify(initialArgs) : initialArgs.join('\n'))
function booleanChoice(value?: ProfileOverride<boolean>): string { return value?.mode === 'set' ? String(value.value) : value?.mode ?? 'inherit' }
const permission = ref(booleanChoice(original?.skipPermissions ?? (cli === 'claude' ? { mode: 'set', value: false } : undefined)))
const observer = ref(booleanChoice(original?.observer ?? { mode: 'set', value: false }))
const launcherKind = ref(original?.launcher.kind ?? 'native')
const launcherProgram = ref(original?.launcher.kind === 'shell' ? original.launcher.program : original?.launcher.kind === 'shim' ? original.launcher.runner : '')
const dialect = ref<ShellDialect>(original?.launcher.kind !== 'native' && original ? original.launcher.dialect : 'bash')
// Only names and override states enter the rendered editor; values/references stay opaque.
const envIndicators = Object.entries(original?.env ?? {}).map(([name, entry]) => ({ name, mode: entry.mode }))
const busy = ref(false), blocked = ref(false), validation = ref<string | null>(null)
const error = ref<UserErrorPresentation | null>(null)
let current = true
function close() { current = false; emit('close') }
watch(() => props.active, active => { if (!active) close() }, { flush: 'sync', immediate: true })
onBeforeUnmount(() => { current = false })
const own = () => current && props.active && props.request === request
const title = computed(() => t(request.kind === 'rename' ? 'launchConfigRename' : request.kind === 'copy' ? 'launchConfigCopy' : request.kind === 'create' ? 'launchConfigCreate' : 'launchConfigEdit'))
const modes = computed(() => [{ value: 'inherit', label: t('launchConfigInherit') }, { value: 'set', label: t('launchConfigSet') }, { value: 'unset', label: t('launchConfigUnset') }])
const booleans = computed(() => [{ value: 'inherit', label: t('launchConfigInherit') }, { value: 'false', label: t('launchConfigOff') }, { value: 'true', label: t('launchConfigOn') }, { value: 'unset', label: t('launchConfigUnset') }])
function args(): string[] {
  const values = format.value === 'json' ? parseNativeRawArgv(argsText.value) : argvFromLines(argsText.value)
  if (values.some(value => value.includes('\0'))) throw new Error('INVALID_RAW_ARGV_JSON')
  return values
}
function changeFormat(value: string) {
  try {
    const values = args()
    if (value === 'lines' && needsJson(values)) { validation.value = 'newSessionArgvFormatError'; return }
    format.value = value === 'json' ? 'json' : 'lines'
    argsText.value = format.value === 'json' ? JSON.stringify(values) : values.join('\n'); validation.value = null
  } catch { validation.value = 'newSessionArgvError' }
}
function bool(value: string): ProfileOverride<boolean> { return value === 'true' || value === 'false' ? { mode: 'set', value: value === 'true' } : { mode: value === 'unset' ? 'unset' : 'inherit' } }
async function save() {
  if (!own() || busy.value || blocked.value) return
  validation.value = null; error.value = null
  if (!name.value.trim() || /\p{Cc}/u.test(name.value)) { validation.value = 'launchConfigNameInvalid'; return }
  if (!original && request.kind !== 'create') { error.value = mapSafeUserError('PROFILE_SELECTION_CHANGED', 'settings'); blocked.value = true; return }
  let changes: ProfileChanges
  try {
    changes = { name: name.value.trim() }
    if (request.kind !== 'rename') {
      if ((pathMode.value === 'set' && !pathText.value.trim()) || pathText.value.includes('\0') || (launcherKind.value !== 'native' && !launcherProgram.value.trim()) || launcherProgram.value.includes('\0')) {
        validation.value = 'launchConfigPathInvalid'; return
      }
      changes = { ...changes, programPath: pathMode.value === 'set' ? { mode: 'set', value: pathText.value } : { mode: pathMode.value },
        defaultArgs: argsMode.value === 'set' ? { mode: 'set', value: args() } : { mode: argsMode.value },
        skipPermissions: cli === 'claude' ? bool(permission.value) : original?.skipPermissions ?? { mode: 'inherit' }, observer: bool(observer.value),
        launcher: launcherKind.value === 'native' ? { kind: 'native' } : launcherKind.value === 'shell'
          ? { kind: 'shell', program: launcherProgram.value, dialect: dialect.value } : { kind: 'shim', runner: launcherProgram.value, dialect: dialect.value } }
    }
  } catch { validation.value = 'newSessionArgvError'; return }
  const creating = request.kind === 'copy' || request.kind === 'create'
  const patch = creating ? { op: 'create' as const, profile: {
    id: createdId!, revision: '0', cli, name: changes.name!, launcher: changes.launcher!, programPath: changes.programPath!,
    defaultArgs: changes.defaultArgs!, skipPermissions: changes.skipPermissions!, observer: changes.observer!,
    env: original?.env ?? {},
  } satisfies CliProfile } : { op: 'update' as const, id: original!.id, changes }
  busy.value = true
  try {
    await profiles.saveConfiguration({ expectedRevision: workspaceRevision, source, patch }, own)
    if (own()) { useNotificationsStore().pushToast({ kind: 'success', messageKey: 'launchConfigSaved' }); close() }
  } catch (failure) {
    if (own()) { error.value = mapSafeUserError(safeUserErrorCode(failure), 'settings'); blocked.value = true }
  } finally { if (own()) busy.value = false }
}
</script>
<template>
  <AppDialog :open="active && current" :title="title" :description="t('launchConfigFutureOnly')" @close="close">
    <div class="launch-editor">
      <AppInput data-launch-name :label="t('launchConfigName')" v-model="name" :disabled="busy || blocked" />
      <template v-if="request.kind !== 'rename'">
        <p>{{ cli === 'claude' ? 'Claude Code' : 'Codex CLI' }}</p>
        <details><summary>{{ t('newSessionMoreOptions') }}</summary>
          <AppSelect :label="t('launchConfigProgram')" v-model="pathMode" :options="modes" :disabled="busy || blocked" />
          <AppInput v-if="pathMode === 'set'" data-launch-program :label="t('launchConfigProgramPath')" v-model="pathText" :disabled="busy || blocked" />
          <AppSelect v-if="cli === 'claude'" :label="t('launchConfigPermissions')" v-model="permission" :options="booleans" :disabled="busy || blocked" />
          <p>{{ t('launchConfigPermissionHint') }}</p>
          <AppSelect :label="t('launchConfigObserver')" v-model="observer" :options="booleans" :disabled="busy || blocked" />
        </details>
        <details><summary>{{ t('newSessionDeveloperOptions') }}</summary>
          <AppSelect :label="t('launchConfigLauncher')" v-model="launcherKind" :options="[{ value: 'native', label: t('launchConfigDirect') }, { value: 'shell', label: t('launchConfigShell') }, { value: 'shim', label: t('launchConfigShim') }]" :disabled="busy || blocked" />
          <template v-if="launcherKind !== 'native'">
            <AppInput :label="t('launchConfigLauncherProgram')" v-model="launcherProgram" :disabled="busy || blocked" />
            <AppSelect :label="t('launchConfigDialect')" v-model="dialect" :options="[{ value: 'bash', label: 'Bash' }, { value: 'power-shell', label: 'PowerShell' }, { value: 'cmd', label: 'cmd' }]" :disabled="busy || blocked" />
          </template>
          <AppSelect data-launch-args-mode :label="t('launchConfigArguments')" v-model="argsMode" :options="modes" :disabled="busy || blocked" />
          <template v-if="argsMode === 'set'">
            <AppSelect data-launch-args-format :label="t('launchConfigArgumentFormat')" :model-value="format" :options="[{ value: 'lines', label: t('newSessionArgvLines') }, { value: 'json', label: 'JSON' }]" @update:model-value="changeFormat" :disabled="busy || blocked" />
            <label class="ui-field-label" for="launch-configuration-argv">{{ t('launchConfigArguments') }}</label>
            <textarea id="launch-configuration-argv" data-launch-argv class="ui-input launch-argv" v-model="argsText" :disabled="busy || blocked" spellcheck="false" />
            <p>{{ t('launchConfigExactArguments') }}</p>
          </template>
          <h3>{{ t('launchConfigEnvironment') }}</h3><p>{{ t('launchConfigEnvironmentHint') }}</p>
          <ul class="launch-env"><li v-for="entry in envIndicators" :key="entry.name"><span>{{ entry.name }}</span><span>{{ t(entry.mode === 'set' ? 'launchConfigSet' : entry.mode === 'unset' ? 'launchConfigUnset' : 'launchConfigInherit') }}</span></li></ul>
          <p v-if="!envIndicators.length">{{ t('launchConfigInherit') }}</p>
        </details>
      </template>
      <InlineNotice v-if="validation" kind="warning" :message="t(validation)" />
      <InlineNotice v-if="error" :kind="error.severity" :message="t(error.messageKey)" />
      <ErrorDetails v-if="error" :code="error.detailCode" context="settings" />
      <p v-if="blocked">{{ t('launchConfigReopenHint') }}</p>
    </div>
    <template #footer><AppButton data-launch-cancel @click="close">{{ t('cancel') }}</AppButton><AppButton data-launch-save variant="primary" :disabled="busy || blocked" :loading="busy" @click="save">{{ t('save') }}</AppButton></template>
  </AppDialog>
</template>
<style scoped>
.launch-editor, .launch-editor details { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
.launch-editor p { color: var(--text-secondary); overflow-wrap: anywhere; }
.launch-editor summary { cursor: pointer; font-weight: 600; margin-bottom: 12px; }
.launch-argv { width: 100%; min-height: 110px; resize: vertical; font-family: monospace; white-space: pre; }
.launch-env { padding: 0; list-style: none; }
.launch-env li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 12px; overflow-wrap: anywhere; }
</style>
