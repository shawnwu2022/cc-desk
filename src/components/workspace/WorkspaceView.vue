<script lang="ts">
export type CliAvailability = 'unknown' | 'available' | 'unavailable'
</script>
<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import WorkspaceHeader from './WorkspaceHeader.vue'
import EmptyState from '@/components/ui/EmptyState.vue'
import InlineNotice from '@/components/ui/InlineNotice.vue'
import type { UnifiedCliKind, UnifiedProjectIdentity, UnifiedSession } from '@/types/unifiedSession'

const props = withDefaults(defineProps<{
  project?: UnifiedProjectIdentity | null
  projectTitle?: string
  activeSession?: UnifiedSession | null
  cliAvailability?: Partial<Record<UnifiedCliKind, CliAvailability>>
  requestPending?: boolean
}>(), { project: null, projectTitle: '', activeSession: null, cliAvailability: () => ({}), requestPending: false })
const emit = defineEmits<{ 'new-session-request': [project: UnifiedProjectIdentity]; 'add-project': [] }>()
const { t } = useI18n()
const clis = ['claude', 'codex'] as const
function requestSession() {
  if (props.project) emit('new-session-request', props.project)
  else emit('add-project')
}
</script>

<template>
  <section class="workspace-view" :aria-label="t('workspace')">
    <WorkspaceHeader :project-title="projectTitle" :session-title="activeSession?.title"
      :has-project="!!project" @new-session-request="requestSession" @add-project="emit('add-project')" />
    <div class="workspace-content">
      <InlineNotice v-for="cli in clis.filter(cli => cliAvailability[cli] === 'unavailable')" :key="cli"
        :data-cli-unavailable="cli" kind="warning" :message="t('cliUnavailable', { cli: cli === 'claude' ? 'Claude Code' : 'Codex CLI' })" />
      <InlineNotice v-if="requestPending" :message="t('workspaceActionPending')" />
      <!-- Task 11 fills this single host slot. Section switching must never unmount it. -->
      <div class="workspace-terminal-host" data-workspace-terminal-host>
        <slot name="terminal">
          <EmptyState :title="t('workspaceWelcome')" :description="t('workspaceWelcomeHint')"
            :action-label="t(project ? 'newSession' : 'addProject')" @action="requestSession" />
        </slot>
      </div>
    </div>
  </section>
</template>

<style scoped>
.workspace-view { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; overflow: hidden; }
.workspace-content { display: flex; flex-direction: column; flex: 1; gap: 8px; padding: 12px; min-width: 0; min-height: 0; overflow: hidden; }
.workspace-terminal-host { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; overflow: hidden; }
</style>
