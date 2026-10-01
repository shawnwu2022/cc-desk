<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppMenu from '@/components/ui/AppMenu.vue'
import AppTooltip from '@/components/ui/AppTooltip.vue'
import IconButton from '@/components/ui/IconButton.vue'
import SessionList from './SessionList.vue'
import NewSessionMenu from './NewSessionMenu.vue'
import { useNewSessionDraftStore } from '@/stores/newSessionDraft'
import type {
  ProjectActionRequest, ProjectMenuAction, SessionMenuAction, SessionPrimaryAction,
  SessionTreeConfirmationRequest, UnifiedProjectGroup, UnifiedProjectIdentity, NewSessionRequest,
} from '@/types/unifiedSession'

const props = withDefaults(defineProps<{
  project: UnifiedProjectGroup
  expanded: boolean
  surfaceActive?: boolean
  selectedId?: string | null
  isCurrent?: boolean
  /** Search expansion is temporary and must not alter explicit user state. */
  disableToggle?: boolean
}>(), { isCurrent: false, disableToggle: false, surfaceActive: true })
const emit = defineEmits<{
  'toggle-expand': [projectKey: string]
  'new-session-request': [project: NewSessionRequest]
  'project-action': [request: ProjectActionRequest]
  activate: [id: string]
  'primary-action': [id: string, action: SessionPrimaryAction]
  'menu-action': [id: string, action: SessionMenuAction]
  'rename-commit': [id: string, title: string]
  'rename-cancel': [id: string]
  'confirmation-request': [request: SessionTreeConfirmationRequest]
}>()
const { t } = useI18n()
const row = ref<HTMLElement | null>(null)
const menu = ref<InstanceType<typeof AppMenu> | null>(null)
const menuOpen = ref(false)
const newMenuOpen = ref(false)
const newMenuAnchor = ref({ x: 8, y: 8 })
const draft = useNewSessionDraftStore()
function openNewMenu(event: MouseEvent) {
  if (!props.surfaceActive) return
  const target = event.currentTarget as HTMLElement
  target.focus()
  const rect = target.getBoundingClientRect()
  newMenuAnchor.value = { x: rect.left, y: rect.bottom + 4 }
  menuOpen.value = false
  newMenuOpen.value = !newMenuOpen.value
}
const anchor = ref({ x: 8, y: 8 })
const menuPosition = ref({ left: '8px', top: '8px' })
const sessions = computed(() => props.project.sessions.filter(session => !session.archived))
const projectIdentity = computed<UnifiedProjectIdentity>(() => ({
  projectKey: props.project.projectKey, projectPath: props.project.projectPath,
}))
const menuItems = computed(() => [
  { id: props.project.pinned ? 'unpin' : 'pin', label: t(props.project.pinned ? 'unpin' : 'pin') },
  { id: 'rename', label: t('rename') },
  { id: 'view-archive', label: t('archivedSessions') },
  { id: 'open-project-directory', label: t('openFolder') },
  { id: 'remove-project', label: t('removeProject'), danger: true },
])
function toggle() {
  if (!props.disableToggle) emit('toggle-expand', props.project.projectKey)
}
function openOverflow(event: MouseEvent) {
  newMenuOpen.value = false
  if (!props.surfaceActive) return
  if (menuOpen.value) { menuOpen.value = false; return }
  const trigger = event.currentTarget as HTMLElement
  trigger.focus()
  const rect = trigger.getBoundingClientRect()
  anchor.value = { x: rect.right - 200, y: rect.bottom + 4 }
  menuOpen.value = true
}
function overflowPointerdown(event: PointerEvent) {
  // An inactive opener must still dismiss another project's/session's menu.
  if (menuOpen.value) event.stopPropagation()
}
function openContext(event: MouseEvent) {
  newMenuOpen.value = false
  if (!props.surfaceActive) return
  event.preventDefault(); event.stopPropagation()
  row.value?.focus()
  anchor.value = { x: event.clientX, y: event.clientY }
  menuOpen.value = true
}
function onRowKeydown(event: KeyboardEvent) {
  if (!props.surfaceActive) return
  if (event.target !== event.currentTarget) return
  if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) {
    newMenuOpen.value = false
    event.preventDefault(); event.stopPropagation()
    const rect = row.value!.getBoundingClientRect()
    anchor.value = { x: rect.right - 200, y: rect.bottom + 4 }
    menuOpen.value = true
  } else if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault(); toggle()
  }
}
function projectAction(id: string) {
  const item = menuItems.value.find(item => item.id === id)
  if (!item) return
  emit('project-action', { ...projectIdentity.value, action: item.id as ProjectMenuAction })
}
function sessionMenuAction(id: string, action: SessionMenuAction) {
  const session = sessions.value.find(session => session.id === id)
  if (!session) return
  if (action === 'archive' && session.processState === 'running') {
    emit('confirmation-request', { kind: 'stop-and-archive', sessionId: id, ...projectIdentity.value })
    return
  }
  // Recheck live state even if a now-stale menu originated in a stopped state.
  if (action === 'archive' && (session.processState === 'unknown' || session.processState === 'starting')) return
  emit('menu-action', id, action)
}
async function placeMenu() {
  await nextTick()
  if (!menuOpen.value) return
  const element = menu.value?.$el
  const rect = element instanceof HTMLElement ? element.getBoundingClientRect() : null
  const width = rect?.width || 200
  const height = rect?.height || menuItems.value.length * 32 + 8
  menuPosition.value = {
    left: `${Math.max(8, Math.min(anchor.value.x, window.innerWidth - width - 8))}px`,
    top: `${Math.max(8, Math.min(anchor.value.y, window.innerHeight - height - 8))}px`,
  }
}
watch(() => [menuOpen.value, anchor.value.x, anchor.value.y, menuItems.value], () => { void placeMenu() }, { immediate: true })
watch(menuOpen, open => {
  if (open) window.addEventListener('resize', placeMenu)
  else window.removeEventListener('resize', placeMenu)
})
watch(() => props.project.projectKey, () => { menuOpen.value = false; newMenuOpen.value = false })
watch(() => props.surfaceActive, active => { if (!active) menuOpen.value = false }, { flush: 'sync' })
onBeforeUnmount(() => { window.removeEventListener('resize', placeMenu) })
</script>

<template>
  <div class="project-node" :class="{ current: isCurrent }" role="treeitem" :aria-expanded="expanded" :aria-label="project.name">
    <div ref="row" :data-project-key="project.projectKey" class="project-row" role="button" :aria-expanded="expanded" :aria-label="project.name" tabindex="0"
      @contextmenu="openContext" @keydown="onRowKeydown">
      <IconButton class="expand-arrow" :class="{ expanded }" :label="expanded ? t('collapse') : t('expand')"
        :aria-expanded="expanded" :disabled="disableToggle" @click.stop="toggle">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><polyline points="9 6 15 12 9 18" /></svg>
      </IconButton>
      <AppTooltip :text="project.projectPath">
        <div class="project-main" role="button" tabindex="0" :aria-label="project.name" :aria-expanded="expanded"
          @click.stop="toggle" @keydown.enter.self.prevent.stop="toggle" @keydown.space.self.prevent.stop="toggle">
          <span class="project-name">{{ project.name }}</span>
          <span v-if="project.pinned" class="pin-mark" :aria-label="t('pinned')">⌖</span>
        </div>
      </AppTooltip>
      <span v-if="!expanded && project.needsUserCount > 0" class="project-attention" data-project-attention
        role="img" :aria-label="t('projectNeedsReplyCount', { count: project.needsUserCount })" />
      <span v-else class="project-attention-slot" aria-hidden="true" />
      <IconButton class="project-new-session" data-project-quick-action="new-session" :label="t('newSessionTitle')"
        aria-haspopup="menu" :aria-expanded="newMenuOpen" @pointerdown="newMenuOpen && $event.stopPropagation()" @click.stop="openNewMenu">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14" /></svg>
      </IconButton>
      <div class="project-overflow-trigger" @pointerdown="overflowPointerdown">
        <IconButton :label="t('projectActionsLabel')" aria-haspopup="menu" :aria-expanded="menuOpen" @click.stop="openOverflow">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" /></svg>
        </IconButton>
      </div>
    </div>
    <div v-if="expanded" class="session-sub">
      <SessionList :sessions="sessions" :selected-id="selectedId" :surface-active="surfaceActive" @activate="emit('activate', $event)"
        @primary-action="(id, action) => emit('primary-action', id, action)" @menu-action="sessionMenuAction"
        @rename-commit="(id, title) => emit('rename-commit', id, title)" @rename-cancel="emit('rename-cancel', $event)" />
      <div v-if="sessions.length === 0" class="empty-hint">{{ t('noHistorySessions') }}</div>
    </div>
    <NewSessionMenu v-model:open="newMenuOpen" :active="surfaceActive" :anchor="newMenuAnchor" :availability="draft.availabilityFor(projectIdentity)"
      @select="emit('new-session-request', { ...projectIdentity, intent: $event })" />
    <Teleport to="body">
      <AppMenu ref="menu" v-model:open="menuOpen" class="project-menu" :style="menuPosition"
        :label="t('projectActionsLabel')" :items="menuItems" @select="projectAction" />
    </Teleport>
  </div>
</template>

<style scoped>
.project-node { min-width: 0; }
.project-row {
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr) 20px 28px 28px;
  column-gap: 4px;
  align-items: center;
  height: 40px;
  min-width: 0;
  padding: 0 4px;
  border-radius: var(--radius-md);
}
.project-row:hover { background: var(--hover-bg); }
.project-row:focus-visible, .project-main:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
.project-row > :deep(.ui-tooltip-anchor) { min-width: 0; }
.project-main { display: flex; align-items: center; gap: 4px; min-width: 0; cursor: pointer; }
.project-name {
  display: block;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 600;
}
.project-node.current .project-name { color: var(--accent-color); }
.pin-mark { flex-shrink: 0; color: var(--text-tertiary); }
.project-attention { width: 8px; height: 8px; justify-self: center; border-radius: 50%; background: var(--accent-gold); }
.project-row :deep(.ui-icon-button) { width: 28px; min-width: 28px; height: 28px; padding: 0; }
.project-row :deep(.expand-arrow) { width: 20px; min-width: 20px; }
.expand-arrow :deep(svg) { transition: transform 0.15s ease; }
.expand-arrow.expanded :deep(svg) { transform: rotate(90deg); }
.session-sub { padding-left: 8px; min-width: 0; }
.empty-hint { padding: 12px 8px; font-size: 12px; color: var(--text-secondary); }
.project-menu { position: fixed; max-height: calc(100vh - 16px); }
@media (prefers-reduced-motion: reduce) { .expand-arrow :deep(svg) { transition: none; } }
</style>
