<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppButton from '@/components/ui/AppButton.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import LoadingState from '@/components/ui/LoadingState.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import CliAppIcon from './CliAppIcon.vue'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useWorkspaceStore } from '@/stores/workspace'
import { sameProjectPath } from '@/utils/path'
import { projectBasename } from '@/utils/displayName'
import type { CreateUnifiedSessionInput, UnifiedCliKind, UnifiedSession } from '@/types/unifiedSession'

const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })
const { t } = useI18n()
const catalog = useUnifiedSessionsStore()
const profiles = useCliProfilesStore()
const workspace = useWorkspaceStore()
const query = ref('')
const cli = ref('')
const scope = ref<'current-project' | 'all'>('current-project')
const age = ref('all')
const configId = ref('')
const configRevision = ref('')
const projectId = ref('')
const sessionId = ref('')
const rows = ref<UnifiedSession[]>([])
const selected = ref<UnifiedSession | null>(null)
const direct = ref<CreateUnifiedSessionInput | null>(null)
const loading = ref(false)
const busy = ref(false)
const partial = ref(false)
const error = ref('')
const removing = ref(false)
let initializing = false
let searchEpoch = 0
let operationEpoch = 0
const open = computed({ get: () => !!catalog.resumeDialog, set: value => { if (!value) catalog.closeResumeDialog() } })
const mode = computed(() => catalog.resumeDialog?.mode ?? 'history')
const configurations = computed(() => [{ value: '', label: t('resumeChooseConfiguration') }, ...profiles.profiles.filter(profile => profile.cli === cli.value).map(profile => ({ value: profile.id, label: profile.name }))])
const registeredProjects = computed(() => [{ value: '', label: t('resumeChooseProject') }, ...workspace.projects.map(project => ({ value: project.projectId, label: projectBasename(project.selectedPath) }))])
const title = computed(() => t(mode.value === 'resume-id' ? 'newSessionById' : mode.value === 'resume-picker' ? 'newSessionNativePicker' : 'newSessionRestore'))
function safeMessage(failure: unknown) {
  const code = failure instanceof Error ? failure.message : failure && typeof failure === 'object' && 'code' in failure && typeof failure.code === 'string' ? failure.code : ''
  if (code === 'SESSION_NOT_FOUND') return 'errorSessionNotFound'
  if (['SOURCE_CHANGED', 'HISTORY_ABSENCE_UNVERIFIED'].includes(code)) return 'resumeHistoryUncertain'
  if (code === 'SESSION_ORIGIN_AMBIGUOUS') return 'resumeAmbiguous'
  if (['PROFILE_SELECTION_CHANGED', 'RESTORE_CONFIGURATION_REQUIRED', 'PROJECT_NOT_FOUND', 'PROJECT_IDENTITY_CHANGED', 'PROFILE_NOT_FOUND', 'CLI_PROFILE_REQUIRED', 'REVISION_CONFLICT'].includes(code)) return 'resumeConfigurationChanged'
  if (code === 'SESSION_EXISTS') return 'resumeFoundAgain'
  if (code === 'LAUNCH_STATE_UNKNOWN') return 'errorLaunchStateUnknown'
  return 'resumeUnavailable'
}
async function search() {
  const request = catalog.resumeDialog
  const epoch = ++searchEpoch
  if (!props.active || !request || request.mode !== 'history') { loading.value = false; return }
  loading.value = true
  try {
    const result = await catalog.searchSessions({ projectPath: request.project.projectPath, scope: scope.value,
      cli: cli.value ? cli.value as UnifiedCliKind : undefined, query: query.value,
      since: age.value === 'all' ? undefined : Date.now() - Number(age.value) * 86400000 })
    if (epoch !== searchEpoch || !props.active || catalog.resumeDialog !== request) return
    rows.value = result.sessions
    partial.value = result.partial
  } catch {
    if (epoch === searchEpoch) { rows.value = []; partial.value = true }
  } finally { if (epoch === searchEpoch) loading.value = false }
}
watch(() => [props.active, catalog.resumeDialog] as const, ([active, request]) => {
  ++searchEpoch; ++operationEpoch; busy.value = false; error.value = ''; removing.value = false
  if (!active) { catalog.closeResumeDialog(); return }
  if (!request) return
  initializing = true
  query.value = ''; cli.value = request.cli ?? (request.mode === 'history' ? '' : 'claude')
  scope.value = 'current-project'; age.value = 'all'; rows.value = []; configId.value = request.launchConfigId ?? ''; configRevision.value = request.launchConfigRevision ?? ''; sessionId.value = ''; direct.value = null
  const matches = workspace.projects.filter(project => sameProjectPath(project.selectedPath, request.project.projectPath))
  projectId.value = matches.length === 1 ? matches[0].projectId : ''
  selected.value = request.sessionId ? catalog.sessions.find(row => row.id === request.sessionId) ?? null : null
  initializing = false
  void search()
}, { immediate: true, flush: 'sync' })
watch([query, cli, scope, age], () => { if (initializing) return; selected.value = null; direct.value = null; error.value = ''; void search() }, { flush: 'sync' })
watch(configId, id => { if (!initializing) configRevision.value = profiles.profile(id)?.revision ?? '' }, { flush: 'sync' })
watch([configId, projectId, sessionId], () => { direct.value = null; error.value = '' })
function choose(row: UnifiedSession) { if (!busy.value) { selected.value = { ...row, nativeOrigin: row.nativeOrigin ? { ...row.nativeOrigin } : undefined }; error.value = ''; removing.value = false } }
function prepareDirect() {
  const profile = profiles.profile(configId.value)
  const project = workspace.projects.find(row => row.projectId === projectId.value)
  const id = sessionId.value.trim()
  if (!profile || profile.cli !== cli.value || !project || mode.value === 'history') { error.value = 'resumeChooseRequired'; return }
  if (profile.revision !== configRevision.value) { error.value = 'resumeConfigurationChanged'; return }
  if (mode.value === 'resume-id' && (!id || id.length > 256 || /\p{Cc}/u.test(id))) { error.value = 'resumeInvalidId'; return }
  direct.value = { projectKey: project.selectedPath, projectPath: project.selectedPath, cli: cli.value as UnifiedCliKind,
    launchConfigId: profile.id, launchConfigRevision: configRevision.value, registeredProjectId: project.projectId,
    action: mode.value === 'resume-id' ? { kind: 'resume-id', nativeSessionId: id } : { kind: 'resume-picker', scope: scope.value } }
  error.value = ''
}
async function confirm() {
  if (busy.value || !props.active || !catalog.resumeDialog) return
  const owner = ++operationEpoch
  const request = catalog.resumeDialog
  const current = () => props.active && owner === operationEpoch && catalog.resumeDialog === request
  busy.value = true; error.value = ''
  try {
    if (direct.value) await catalog.launchResume(direct.value, current)
    else if (selected.value) await catalog.resumeCatalogSession(selected.value, current)
    else return
    if (current()) catalog.closeResumeDialog()
  } catch (failure) { if (current()) error.value = safeMessage(failure) }
  finally { if (current()) busy.value = false }
}
async function removeRecord() {
  if (!selected.value || busy.value) return
  const owner = ++operationEpoch
  busy.value = true
  try {
    await catalog.removeMissingRecord(selected.value.id)
    if (owner !== operationEpoch) return
    selected.value = null; removing.value = false; error.value = ''; await search()
  } catch (failure) { if (owner === operationEpoch) { removing.value = false; error.value = safeMessage(failure) } }
  finally { if (owner === operationEpoch) busy.value = false }
}
onUnmounted(() => { ++searchEpoch; ++operationEpoch })
</script>

<template>
  <AppDialog v-model:open="open" :title="title" class="resume-session-dialog">
    <div class="resume-fields">
      <template v-if="mode === 'history'">
        <AppInput v-model="query" data-resume-query :label="t('resumeSearch')" :disabled="busy" />
        <div class="resume-filters">
          <AppSelect v-model="cli" data-resume-cli :label="t('newSessionTool')" :disabled="busy" :options="[{ value: '', label: t('resumeAllTools') }, { value: 'claude', label: 'Claude Code' }, { value: 'codex', label: 'Codex CLI' }]" />
          <AppSelect v-model="scope" data-resume-scope :label="t('resumeScope')" :disabled="busy" :options="[{ value: 'current-project', label: t('resumeCurrentProject') }, { value: 'all', label: t('resumeAllProjects') }]" />
          <AppSelect v-model="age" data-resume-age :label="t('resumeTime')" :disabled="busy" :options="[{ value: 'all', label: t('resumeAnyTime') }, { value: '1', label: t('resumeLastDay') }, { value: '7', label: t('resumeLastWeek') }, { value: '30', label: t('resumeLastMonth') }]" />
        </div>
        <LoadingState v-if="loading" :label="t('loading')" />
        <InlineNotice v-if="partial" kind="warning" :message="t('resumePartial')" />
        <EmptyState v-if="!loading && !rows.length" :title="t('resumeNoResults')" :description="t('resumeNoResultsHint')" :action-label="t('retry')" @action="search" />
        <div class="resume-results" :aria-label="t('newSessionRestore')">
          <AppButton v-for="row in rows" :key="row.id" data-resume-result variant="ghost" :disabled="busy" :aria-pressed="selected?.id === row.id" @click="choose(row)">
            <CliAppIcon :cli="row.cli" /><span class="resume-result-copy"><strong>{{ row.title }}</strong><small>{{ row.nativeSessionId }}</small><small v-if="scope === 'all'">{{ projectBasename(row.projectPath) }}</small><small v-if="row.launchConfigId">{{ profiles.profile(row.launchConfigId)?.name }}</small></span>
          </AppButton>
        </div>
      </template>
      <template v-else>
        <AppSelect v-model="cli" data-resume-cli :label="t('newSessionTool')" :disabled="busy" :options="[{ value: 'claude', label: 'Claude Code' }, { value: 'codex', label: 'Codex CLI' }]" />
        <AppSelect v-model="projectId" data-resume-project :label="t('newSessionProject')" :options="registeredProjects" :disabled="busy" />
        <AppSelect v-model="configId" data-resume-config :label="t('newSessionConfiguration')" :options="configurations" :disabled="busy" />
        <AppInput v-if="mode === 'resume-id'" v-model="sessionId" data-resume-id :label="t('resumeSessionId')" :disabled="busy" />
        <AppSelect v-else v-model="scope" data-resume-scope :label="t('resumeScope')" :disabled="busy" :options="[{ value: 'current-project', label: t('resumeCurrentProject') }, { value: 'all', label: t('resumeAllProjects') }]" />
        <InlineNotice :message="t('resumeDirectHint')" />
        <AppButton v-if="!direct" data-prepare-resume :disabled="busy" @click="prepareDirect">{{ t('newSessionContinueRestore') }}</AppButton>
      </template>
      <InlineNotice v-if="selected || direct" :message="t(removing ? 'resumeRemoveHint' : 'resumeConfirmHint')" />
      <p v-if="selected" class="resume-confirm-title">{{ selected.title }}</p>
      <InlineNotice v-if="error" kind="warning" :message="t(error)">
        <AppButton v-if="error === 'errorSessionNotFound' && !removing" data-remove-record :disabled="busy" @click="removing = true">{{ t('removeHistoryRecord') }}</AppButton>
      </InlineNotice>
      <div class="resume-actions">
        <AppButton @click="open = false">{{ t('cancel') }}</AppButton>
        <AppButton v-if="removing" data-confirm-remove variant="danger" :disabled="busy" @click="removeRecord">{{ t('removeHistoryRecord') }}</AppButton>
        <AppButton v-else-if="selected || direct" data-confirm-resume variant="primary" :disabled="busy || error === 'errorSessionNotFound'" @click="confirm">{{ t('resumeConfirm') }}</AppButton>
      </div>
    </div>
  </AppDialog>
</template>

<style scoped>
.resume-session-dialog { width: min(640px, calc(100vw - 32px)); }
.resume-fields { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
.resume-filters { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
.resume-results { display: flex; flex-direction: column; gap: 4px; max-height: 260px; overflow: auto; min-width: 0; }
.resume-results > button { height: auto; min-height: 40px; justify-content: flex-start; text-align: start; }
.resume-result-copy { display: flex; flex-direction: column; min-width: 0; overflow: hidden; }
.resume-result-copy strong, .resume-result-copy small, .resume-confirm-title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.resume-result-copy small { color: var(--text-secondary); font-size: 11px; }
.resume-actions { display: flex; justify-content: flex-end; gap: 8px; }
</style>
