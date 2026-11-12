<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import AppDrawer from '@/components/ui/AppDrawer.vue'
import SessionList from './SessionList.vue'
import IconButton from '@/components/ui/IconButton.vue'
import NativeSessionDeletePanel from './NativeSessionDeletePanel.vue'
import NativeSessionRecyclePanel from './NativeSessionRecyclePanel.vue'
import OpencodeArchivePanel from './OpencodeArchivePanel.vue'
import { useNativeHistoryStore } from '@/stores/nativeHistory'
import { nativeDeleteEligibility } from '@/utils/nativeSessionDelete'
import { requiresOpencodeNativeRestore } from '@/utils/sessionPresentation'
import { normalizePath } from '@/utils/path'
import { compareSessionOpenOrder } from '@/utils/sessionOpenOrder'
import type { SessionMenuAction, SessionPrimaryAction, UnifiedProjectIdentity, UnifiedSession } from '@/types/unifiedSession'

const props = withDefaults(defineProps<{
  open: boolean
  sessions: UnifiedSession[]
  project?: UnifiedProjectIdentity | null
}>(), { project: null })
const emit = defineEmits<{
  'update:open': [open: boolean]
  'restore-request': [id: string]
  'menu-action': [id: string, action: SessionMenuAction]
  'rename-commit': [id: string, title: string]
  'rename-cancel': [id: string]
}>()
const { t } = useI18n()
const history = useNativeHistoryStore()
const deletePanel = ref<InstanceType<typeof NativeSessionDeletePanel> | null>(null)
const deleteTargetId = ref('')
const opencodePanel = ref<InstanceType<typeof OpencodeArchivePanel> | null>(null)
const archived = computed(() => props.sessions.filter(session => session.archived
  && (!props.project || normalizePath(session.projectPath) === normalizePath(props.project.projectPath)))
  .sort(compareSessionOpenOrder))
const deleteTarget = computed(() => archived.value.find(session => session.id === deleteTargetId.value) ?? null)
const deleteEligibility = computed(() => new Map(archived.value.map(session => [session.id,
  nativeDeleteEligibility(session, session.nativeOrigin ? history.get(session.nativeOrigin) : undefined)])))
function deleteLabel(session: UnifiedSession) {
  const label = t('deletePermanently') + ' · ' + session.title
  const reason = deleteEligibility.value.get(session.id)?.reason
  return reason ? label + ' · ' + t(reason) : label
}
async function reviewDelete(id: string) {
  if (!props.open || !deleteEligibility.value.get(id)?.binding) return
  deleteTargetId.value = id
  await nextTick()
  // 行在渲染间隙被恢复、移除或切换来源时，不沿用旧点击的资格。
  if (props.open && deleteTargetId.value === id && deleteEligibility.value.get(id)?.binding) await deletePanel.value?.review()
}
function restore(id: string) {
  const session = archived.value.find(session => session.id === id)
  if (!props.open || !session) return
  if (requiresOpencodeNativeRestore(session)) opencodePanel.value?.showFor(id)
  else emit('restore-request', id)
}
function primaryAction(id: string, action: SessionPrimaryAction) {
  if (action === 'restore-archive') restore(id)
}
function menuAction(id: string, action: SessionMenuAction) {
  if (!archived.value.some(session => session.id === id)) return
  if (action === 'restore-archive') restore(id)
  else emit('menu-action', id, action)
}
</script>

<template>
  <AppDrawer :open="open" :title="t('archivedSessions')" @update:open="emit('update:open', $event)">
    <template v-if="deletePanel?.hasResults" #headerActions>
      <IconButton data-archive-delete-results :label="t('deleteViewOperation')" @click="deletePanel?.showResults()">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M3 11a9 9 0 1 1 3 7M3 4v7h7M12 7v5l3 2" />
        </svg>
      </IconButton>
    </template>
    <NativeSessionRecyclePanel :sessions="archived" :active="open" :project-path="project?.projectPath" />
    <OpencodeArchivePanel ref="opencodePanel" :sessions="archived" :active="open" :project-path="project?.projectPath" />
    <div class="archived-sessions-content" role="tree" :aria-label="t('archivedSessions')">
      <SessionList v-if="archived.length" :sessions="archived" :surface-active="open" :menu-teleport="false" @primary-action="primaryAction" @menu-action="menuAction"
        @rename-commit="(id, title) => emit('rename-commit', id, title)" @rename-cancel="emit('rename-cancel', $event)">
        <template #trailingAction="{ session }">
          <IconButton v-if="requiresOpencodeNativeRestore(session)" data-session-opencode-archive :label="`${t('opencodeArchiveOpen')} · ${session.title}`" :disabled="!open" @click.stop="opencodePanel?.showFor(session.id)">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 4h18v4H3zM5 8v12h14V8M9 12h6" />
            </svg>
          </IconButton>
          <IconButton v-else data-session-delete :label="deleteLabel(session)" :disabled="!open || !deleteEligibility.get(session.id)?.binding" @click.stop="reviewDelete(session.id)">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 6h18M9 6V4h6v2M5 6l1 14h12l1-14M10 10v6M14 10v6" />
            </svg>
          </IconButton>
        </template>
      </SessionList>
      <p v-else class="archived-empty">{{ t('noArchivedSessions') }}</p>
    </div>
  </AppDrawer>
  <NativeSessionDeletePanel ref="deletePanel" :target="deleteTarget" :active="open" dialog-only />
</template>

<style scoped>
.archived-sessions-content { min-width: 0; }
.archived-empty { color: var(--text-secondary); font-size: 13px; text-align: center; padding: 16px 0; }
</style>
