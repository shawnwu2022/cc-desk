<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { UnifiedSession } from '@/types/unifiedSession'
import { cliDiscoverPrograms, type ProgramDiscovery } from '@/api/programDiscovery'
import { useCliProfilesStore } from '@/stores/cliProfiles'
import { useWorkspaceStore } from '@/stores/workspace'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
import { useAppStore } from '@/stores/app'
import { useShellStore } from '@/stores/shell'
import type { ConfirmedLaunchProgram } from '@/types/profile'
import { safeUserErrorCode } from '@/utils/userError'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import ErrorDetails from '@/components/ui/ErrorDetails.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppButton from '@/components/ui/AppButton.vue'
const props = defineProps<{ session: UnifiedSession }>()
const emit = defineEmits<{ edit: []; confirmed: [confirmation: ConfirmedLaunchProgram] }>()
const { t } = useI18n()
const profiles = useCliProfilesStore(), workspace = useWorkspaceStore(), sessions = useUnifiedSessionsStore(), app = useAppStore()
const row = props.session, owner = sessions.captureSessionOwnership(row.id)
const shell = useShellStore(), selectionOwner = sessions.captureSelectionOwnership()
const navigation = shell.navigationSequence, request = shell.requestSequence
const receipt = ref<ProgramDiscovery | null>(null), choice = ref('0'), loading = ref(true), busy = ref(false), failure = ref<string | null>(null)
let current = true
let accepted = false
let edited = false
// This guard survives the expected unmount when the confirmed row becomes pending.
const ownsNavigation = () => !edited && shell.section === 'workspace' && shell.navigationSequence === navigation
  && shell.requestSequence === request && !app.isProjectAdmissionBlocked(row.projectPath)
const owns = () => current && owner() && selectionOwner() && ownsNavigation() && sessions.activeSessionId === row.id
function edit() { edited = true; emit('edit') }
const selected = computed(() => receipt.value?.candidates[Number(choice.value)])
const message = computed(() => loading.value ? 'launchDiscoverySearching' : failure.value ? 'launchDiscoveryFailed'
  : receipt.value?.candidates.length ? 'launchDiscoveryConfirm' : 'launchDiscoveryEmpty')
onBeforeUnmount(() => { current = false })
onMounted(async () => {
  try {
    if (!owns() || !row.launchConfigId) return
    const profile = profiles.profile(row.launchConfigId)
    if (!profile || profile.cli !== row.cli) throw new Error('PROFILE_SELECTION_CHANGED')
    const revision = profile.revision
    const projectId = await workspace.ensureRegistered(row.projectPath)
    if (!owns()) return
    await profiles.load()
    if (!owns()) return
    if (profiles.profile(profile.id)?.revision !== revision) throw new Error('PROFILE_SELECTION_CHANGED')
    const result = await cliDiscoverPrograms(profile.id, revision, projectId)
    if (!owns()) return
    if (result.cli !== row.cli || profiles.profile(profile.id)?.revision !== revision || profiles.revision !== result.workspaceRevision) throw new Error('PROFILE_SELECTION_CHANGED')
    receipt.value = result
  } catch (error) { if (owns()) failure.value = safeUserErrorCode(error) }
  finally { if (current) loading.value = false }
})
async function confirm() {
  const snapshot = receipt.value, candidate = selected.value
  if (!owns() || busy.value || accepted || failure.value || !snapshot || !candidate) return
  busy.value = true
  try {
    const saved = await profiles.saveConfiguration({ expectedRevision: snapshot.workspaceRevision,
      source: { id: snapshot.profileId, revision: snapshot.profileRevision },
      patch: { op: 'update', id: snapshot.profileId, changes: { programPath: { mode: 'set', value: candidate.programPath }, launcher: candidate.launcher } } }, owns)
    if (!owns()) return
    const profile = saved.profiles.find(profile => profile.id === snapshot.profileId)
    if (!profile || profile.cli !== snapshot.cli || BigInt(profile.revision) <= BigInt(snapshot.profileRevision)
      || BigInt(saved.revision) <= BigInt(snapshot.workspaceRevision)
      || profiles.profile(profile.id)?.revision !== profile.revision
      || profile.programPath.mode !== 'set' || profile.programPath.value !== candidate.programPath
      || JSON.stringify(profile.launcher) !== JSON.stringify(candidate.launcher)) throw new Error('PROFILE_SELECTION_CHANGED')
    accepted = true
    emit('confirmed', { sessionId: row.id, profileId: profile.id, profileRevision: profile.revision,
      canContinue: () => ownsNavigation() && profiles.profile(profile.id)?.revision === profile.revision })
  } catch (error) { if (owns()) failure.value = safeUserErrorCode(error) }
  finally { if (current) busy.value = accepted }
}
</script>
<template>
  <InlineNotice data-launch-preparation data-program-discovery kind="warning" :message="t(message)" :action-label="t('launchConfigEditAction')" @action="edit">
    <template v-if="receipt?.candidates.length && !failure">
      <AppSelect v-model="choice" :label="t('launchDiscoveryProgram')" :disabled="busy" :options="receipt.candidates.map((candidate, index) => ({ value: String(index), label: candidate.programPath }))" />
      <p v-if="selected?.launcher.kind === 'shim'" class="discovered-runner">{{ t('launchDiscoveryRunner') }}: {{ selected.launcher.runner }}</p>
      <AppButton data-program-confirm :disabled="busy" :loading="busy" @click="confirm">{{ t('launchDiscoveryUse') }}</AppButton>
    </template>
    <ErrorDetails v-if="failure" :code="failure" context="launch" />
  </InlineNotice>
</template>
<style scoped>
.discovered-runner { overflow-wrap: anywhere; }
</style>
