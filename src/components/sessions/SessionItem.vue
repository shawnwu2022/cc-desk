<script setup lang="ts">
import { inject } from 'vue'
import { SESSION_INTERACTION_OWNER } from '@/session/sessionInteraction'
import { APP_RENAME_SHORTCUT, captureShortcut } from '@/config/appShortcuts'
import { computed, nextTick, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppInput from '@/components/ui/AppInput.vue'
import AppTooltip from '@/components/ui/AppTooltip.vue'
import IconButton from '@/components/ui/IconButton.vue'
import SessionAppIcon from './SessionAppIcon.vue'
import SessionOverflowMenu from './SessionOverflowMenu.vue'
import { deriveSessionVisualState, makeSessionRenameOwnerKey, selectSessionMenuActions, selectSessionArchiveAction, selectSessionPrimaryAction, selectSessionObservationNotice, sessionActionLabelKey } from '@/utils/sessionPresentation'
import { formatRelativeActivity, useRelativeActivityClock } from '@/utils/relativeTime'
import { buildSessionDragPayload } from '@/utils/sessionReference'
import { startSessionReferenceDrag } from '@/session/sessionReferenceDrag'
import type { SessionMenuAction, SessionMenuActionVisibility, SessionPrimaryAction, UnifiedSession } from '@/types/unifiedSession'

const props = withDefaults(defineProps<{
  session: UnifiedSession
  selected?: boolean
  surfaceActive?: boolean
  primaryAction?: SessionPrimaryAction | null
  menuActionVisibility?: SessionMenuActionVisibility
  /** Keep a drawer menu inside the shared modal focus boundary. */
  menuTeleport?: boolean
}>(), { selected: false, menuTeleport: true, surfaceActive: true })
const emit = defineEmits<{
  activate: [id: string]
  'primary-action': [id: string, action: SessionPrimaryAction]
  'menu-action': [id: string, action: SessionMenuAction]
  'rename-commit': [id: string, title: string]
  'rename-cancel': [id: string]
}>()
const { t, locale } = useI18n()
const captureInteractionOwner = inject(SESSION_INTERACTION_OWNER, null)
const renameShortcut = inject(APP_RENAME_SHORTCUT, computed(() => 'F2'))
const now = useRelativeActivityClock()
const row = ref<HTMLElement | null>(null)
const renameInput = ref<InstanceType<typeof AppInput> | null>(null)
const localRename = ref(false)
const renameValue = ref('')
const renameInvalid = ref(false)
const menuOpen = ref(false)
let firstClick: { session: UnifiedSession; owner: string; owns: (() => boolean) | null; selected: boolean } | null = null
const menuAnchor = ref({ x: 8, y: 8 })
let cancelReferenceDrag: (() => void) | undefined
const isRenaming = computed(() => (props.selected || props.session.opened === false) && (localRename.value || props.session.renameState === 'editing' || (props.selected && props.session.renameState === 'saving')))
const isSaving = computed(() => props.session.renameState === 'saving')
const visualState = computed(() => deriveSessionVisualState(props.session, props.session.opened === true))
const observationNotice = computed(() => selectSessionObservationNotice(props.session))
const receiptId = computed(() => {
  const notice = props.session.observationNotice?.unreadReplyEnd
  return notice ? JSON.stringify([notice.runId, notice.generation, notice.eventId]) : undefined
})
const canRename = computed(() => (props.selected || props.session.opened === false) && !props.session.archived && !props.session.preparationState && props.menuActionVisibility?.rename !== false)
const actions = computed(() => selectSessionMenuActions(props.session, { ...props.menuActionVisibility, rename: canRename.value }))
const archive = computed(() => selectSessionArchiveAction(props.session, props.menuActionVisibility))
const requestedPrimary = computed(() => isRenaming.value ? 'save-rename'
  : props.primaryAction === undefined ? selectSessionPrimaryAction({ ...props.session, renameState: 'idle' }) : props.primaryAction)
// History is resumed by its row. Opened terminals remain selection-only, even
// after natural exit; archived/preparation rows keep their explicit controls.
const rowResumes = computed(() => !props.session.archived && props.session.opened !== true
  && !props.session.preparationState && props.session.processState === 'stopped' && props.session.resumable)
const primary = computed(() => rowResumes.value && requestedPrimary.value === 'resume' ? null : requestedPrimary.value)
const launchPrimary = computed(() => !isRenaming.value && (props.session.archived || !props.session.opened) && ['resume', 'retry', 'restore-archive'].includes(primary.value ?? ''))
const primaryDisabled = computed(() => isSaving.value || !!props.session.resumePending || (launchPrimary.value && props.session.preparationState === 'pending'))
const primaryLabel = computed(() => primary.value ? t(sessionActionLabelKey(primary.value, props.session)) : '')
const age = computed(() => formatRelativeActivity(props.session.lastActivityAt, now.value, locale.value.startsWith('zh') ? 'zh' : 'en'))
const fullActivity = computed(() => new Date(props.session.lastActivityAt).toLocaleString(locale.value))

watch(() => makeSessionRenameOwnerKey(props.session), () => {
  cancelReferenceDrag?.()
  localRename.value = false
  renameInvalid.value = false
  renameValue.value = props.session.title
  menuOpen.value = false
})
watch(() => props.session.renameState, (state, previous) => {
  if (state === 'editing' || state === 'saving') {
    if (!localRename.value) renameValue.value = props.session.title
    if (!isSaving.value) { localRename.value = true; void focusRename() }
  } else if (previous === 'editing' || previous === 'saving') {
    localRename.value = false
    renameInvalid.value = false
    renameValue.value = props.session.title
  }
}, { immediate: true })
watch(() => props.selected, selected => { if (!selected) { localRename.value = false; renameInvalid.value = false } }, { flush: 'sync' })
watch(() => props.surfaceActive, active => { if (!active) menuOpen.value = false }, { flush: 'sync' })
watch(() => [props.surfaceActive, isRenaming.value, menuOpen.value], () => { cancelReferenceDrag?.() }, { flush: 'sync' })
onUnmounted(() => { cancelReferenceDrag?.() })
async function focusRename() {
  await nextTick()
  if (!props.surfaceActive || (props.session.opened !== false && !props.selected)) return
  const input = renameInput.value?.$el.querySelector('input') as HTMLInputElement | null
  input?.focus()
  input?.select()
}
function activate() {
  if (!props.surfaceActive || props.session.archived || isRenaming.value || menuOpen.value) return
  if (props.session.opened === true) emit('activate', props.session.id)
  else if (rowResumes.value && !props.session.resumePending) emit('primary-action', props.session.id, 'resume')
}
function onClick(event: MouseEvent) {
  // A browser double click dispatches click(1), click(2), then dblclick. Capture
  // selection before the first click can activate this row asynchronously.
  if (event.detail > 1) return
  firstClick = { session: props.session, owns: captureInteractionOwner?.(props.session.id) ?? null, owner: makeSessionRenameOwnerKey(props.session), selected: props.selected }
  activate()
}
function onDoubleClick(event: MouseEvent) {
  if (!props.surfaceActive || isRenaming.value || menuOpen.value) return
  if (event.target instanceof Element && event.target.closest('button, input, [role="menu"]')) return
  if (firstClick && (firstClick.owner !== makeSessionRenameOwnerKey(props.session)
    || (firstClick.owns ? !firstClick.owns() : firstClick.session !== props.session))) return
  if (firstClick?.selected ?? props.selected) startRename()
  else if (!firstClick) activate()
}
function startRename() {
  if (!props.surfaceActive || isSaving.value || !canRename.value) return
  // The owning catalog admits the exact attempt before the editor is displayed.
  menuOpen.value = false
  emit('menu-action', props.session.id, 'rename')
}
function commitRename() {
  if (!isRenaming.value || isSaving.value) return
  const title = renameValue.value.trim()
  if (!title || /\p{Cc}/u.test(renameValue.value)) { renameInvalid.value = true; return }
  renameInvalid.value = false
  emit('rename-commit', props.session.id, title)
  void nextTick(() => row.value?.focus())
}
function cancelRename() {
  if (!isRenaming.value || isSaving.value) return
  localRename.value = false
  renameInvalid.value = false
  renameValue.value = props.session.title
  emit('rename-cancel', props.session.id)
  void nextTick(() => row.value?.focus())
}
function runPrimary() {
  if (!props.surfaceActive || !primary.value || primaryDisabled.value) return
  if (primary.value === 'save-rename') commitRename()
  else emit('primary-action', props.session.id, primary.value)
}
function runArchive() {
  if (props.surfaceActive && archive.value && !archive.value.disabled && !isRenaming.value) emit('menu-action', props.session.id, 'archive')
}
function openOverflow(event: MouseEvent) {
  if (!props.surfaceActive) return
  const trigger = event.currentTarget as HTMLElement
  if (menuOpen.value) { menuOpen.value = false; return }
  trigger.focus()
  const rect = trigger.getBoundingClientRect()
  menuAnchor.value = { x: rect.right - 200, y: rect.bottom + 4 }
  menuOpen.value = true
}
function onOverflowPointerdown(event: PointerEvent) {
  // Preserve only this open menu for its own toggle click. Another row's
  // opener must still reach AppMenu's outside listener and dismiss this one.
  if (menuOpen.value) event.stopPropagation()
}
function openContext(event: MouseEvent) {
  if (!props.surfaceActive) return
  if (isRenaming.value || !actions.value.length) return
  event.preventDefault()
  event.stopPropagation()
  row.value?.focus()
  menuAnchor.value = { x: event.clientX, y: event.clientY }
  menuOpen.value = true
}
function onKeydown(event: KeyboardEvent) {
  if (!props.surfaceActive) return
  if (event.target instanceof HTMLInputElement) return
  if (renameShortcut.value && captureShortcut(event) === renameShortcut.value) { event.preventDefault(); event.stopPropagation(); startRename() }
  else if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) {
    if (!actions.value.length || isRenaming.value) return
    event.preventDefault(); event.stopPropagation()
    row.value?.focus()
    const rect = row.value!.getBoundingClientRect()
    menuAnchor.value = { x: rect.right - 200, y: rect.bottom + 4 }
    menuOpen.value = true
  } else if (event.target === event.currentTarget && (event.key === 'Enter' || event.key === ' ')) {
    event.preventDefault(); activate()
  }
}
function onMenuAction(action: SessionMenuAction) {
  menuOpen.value = false
  if (action === 'rename') startRename()
  else emit('menu-action', props.session.id, action)
}
// 原生拖放只接受文件；会话引用在同一文档内跟踪指针，不进入 OS 拖放协议。
function onReferencePointerdown(event: PointerEvent) {
  if (!props.surfaceActive || isRenaming.value || menuOpen.value || event.button !== 0 || !event.isPrimary || event.pointerType === 'touch') return
  if (event.target instanceof Element && event.target.closest('button, input, textarea, select, a, [role="menu"], .session-tail')) return
  cancelReferenceDrag?.()
  cancelReferenceDrag = startSessionReferenceDrag(event, buildSessionDragPayload(props.session))
}
function preventReferenceNativeDrag(event: DragEvent) {
  if (!isRenaming.value) event.preventDefault()
}
</script>

<template>
  <div ref="row" class="session-item" :class="{ active: selected, 'has-primary': !!primary, 'has-launch': launchPrimary, 'has-archive': !!archive && !primary && !isRenaming, editing: isRenaming }"
    role="treeitem" :data-session-row="session.id" :aria-selected="selected" :aria-label="session.title" :aria-busy="session.resumePending || undefined" tabindex="0"
    draggable="false" @pointerdown="onReferencePointerdown" @dragstart="preventReferenceNativeDrag"
    @click="onClick" @dblclick="onDoubleClick" @keydown="onKeydown" @contextmenu="openContext">
    <SessionAppIcon :cli="session.cli" :state="visualState" :activity-state="session.activityState" :observation-state="session.observationState" :observer-reason-code="session.observerReasonCode" :archived="session.archived" :notice="observationNotice" :receipt-id="receiptId" />
    <div class="session-name-wrapper">
      <AppTooltip v-if="isRenaming" class="session-title-tooltip" :text="session.renameTarget ? t(session.renameTarget === 'resume-source' ? 'sessionRenameResumeSourceHint' : 'sessionRenameHistoryRecordHint', { id: session.nativeSessionId }) : ''">
        <AppInput ref="renameInput" v-model="renameValue" class="rename-input" size="compact"
          :aria-label="t(session.renameTarget === 'resume-source' ? 'sessionRenameResumeSourceLabel' : session.renameTarget === 'history-record' ? 'sessionRenameHistoryRecordLabel' : 'sessionRenameLabel')" :invalid="renameInvalid" :disabled="isSaving"
          @click.stop @keydown.enter.stop.prevent="commitRename" @keydown.esc.stop.prevent="cancelRename" />
      </AppTooltip>
      <AppTooltip v-else class="session-title-tooltip" :text="session.title">
        <span class="session-name" tabindex="0">{{ session.title }}</span>
      </AppTooltip>
      <AppTooltip v-if="session.historyReadFailure" :text="t('historySessionReadIssue', { code: session.historyReadFailure })">
        <span data-session-history-issue tabindex="0" :aria-label="t('historySessionReadIssue', { code: session.historyReadFailure })">!</span>
      </AppTooltip>
    </div>
    <div class="session-tail" @click.stop @dblclick.stop>
      <AppTooltip :text="fullActivity">
        <span class="session-time" :class="{ 'session-time--date': age.includes('/') }" tabindex="0">{{ age }}</span>
      </AppTooltip>
      <div v-if="primary" class="session-primary-action" :class="{ 'session-launch-action': launchPrimary }" :data-session-launch="launchPrimary ? '' : undefined">
        <IconButton class="session-row-control" :label="primaryLabel" :disabled="primaryDisabled" @click.stop="runPrimary">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path v-if="primary === 'save-rename'" d="m5 12 4 4L19 6" />
            <rect v-else-if="primary === 'stop'" x="6" y="6" width="12" height="12" rx="1" />
            <path v-else-if="primary === 'cancel-start' || primary === 'close'" d="m6 6 12 12M18 6 6 18" />
            <path v-else-if="primary === 'confirm-status'" d="M9 9a3 3 0 1 1 5 2c-1.5 1-2 1.5-2 3m0 3h.01" />
            <path v-else-if="primary === 'resume'" d="m9 5 11 7-11 7Z" />
            <path v-else d="M20 7v5h-5m5 0a8 8 0 1 0-2 6" />
          </svg>
        </IconButton>
      </div>
    </div>
    <div class="session-archive-slot">
      <div v-if="archive && !primary && !isRenaming" class="session-archive-action" data-session-archive>
        <IconButton class="session-row-control" :label="t(archive.labelKey)" :disabled="archive.disabled" @click.stop="runArchive">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <rect x="3" y="3" width="18" height="4" rx="1" /><path d="M5 7v13h14V7M9 11h6" />
          </svg>
        </IconButton>
      </div>
    </div>
    <div class="session-overflow-trigger" :class="{ 'is-open': menuOpen }" @pointerdown="onOverflowPointerdown" @click.stop>
      <IconButton v-if="actions.length && !isRenaming" class="session-row-control" :label="t('sessionActionsLabel')"
        aria-haspopup="menu" :aria-expanded="menuOpen" @click="openOverflow">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" /></svg>
      </IconButton>
    </div>
    <SessionOverflowMenu v-model:open="menuOpen" :actions="actions" :anchor="menuAnchor" :teleport="menuTeleport" @menu-action="onMenuAction" />
  </div>
</template>

<style scoped>
.session-item {
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr) 38px 20px 20px;
  column-gap: 6px;
  align-items: center;
  height: var(--session-row-height);
  width: 100%;
  padding: 0 8px;
  position: relative;
  border-radius: var(--radius-md);
  color: var(--text-primary);
  background: transparent;
  cursor: pointer;
}
.session-item.has-launch { grid-template-columns: 20px minmax(0, 1fr) 64px 20px 20px; }
.session-item.has-launch .session-tail { width: 64px; gap: 6px; }
.session-item.has-launch .session-launch-action { position: static; width: 20px; flex: 0 0 20px; opacity: 1; pointer-events: auto; }
.session-item.has-launch:hover .session-time, .session-item.has-launch:focus-within .session-time { opacity: 1; pointer-events: auto; }
.session-item:hover { background: var(--hover-bg); }
.session-item.active { background: var(--selected-bg); }
.session-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  top: 4px;
  bottom: 4px;
  width: 3px;
  border-radius: 0 2px 2px 0;
  background: var(--accent-gold);
}
.session-item:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
.session-name-wrapper { display: flex; align-items: center; gap: 4px; min-width: 0; }
.session-name-wrapper :deep(.ui-tooltip-anchor) { display: block; min-width: 0; }
.session-title-tooltip { flex: 1; }
.session-name {
  display: block;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--sidebar-title-size);
  font-weight: 500;
}
.session-item.active .session-name { font-weight: 600; }
.session-name:focus-visible, .session-time:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; border-radius: var(--radius-sm); }
.session-name-wrapper :deep(.rename-input) { width: 100%; min-width: 0; padding: 0 4px; font-size: var(--sidebar-title-size); }
.session-tail { position: relative; width: 38px; height: 28px; display: flex; align-items: center; justify-content: flex-end; }
.session-time {
  display: block;
  width: 38px;
  font-size: 11px;
  white-space: nowrap;
  text-align: right;
  font-variant-numeric: tabular-nums;
  color: var(--text-tertiary);
}
.session-time--date { font-size: 9px; }
.session-primary-action {
  position: absolute;
  inset: 0;
  display: flex;
  justify-content: flex-end;
  align-items: center;
  opacity: 0;
  pointer-events: none;
}
.session-archive-slot { width: 20px; height: 28px; }
[data-session-history-issue] { color: var(--status-warning); flex-shrink: 0; font-weight: 600; }
.session-archive-action { display: flex; justify-content: flex-end; align-items: center; opacity: 0; pointer-events: none; }
.session-item:hover .session-archive-action, .session-item:focus-within .session-archive-action { opacity: 1; pointer-events: auto; }
.session-overflow-trigger { width: 20px; height: 28px; opacity: 0; pointer-events: none; }
.session-primary-action :deep(.ui-button), .session-archive-action :deep(.ui-button), .session-overflow-trigger :deep(.ui-button) { width: 20px; min-width: 20px; height: 28px; padding: 0; }
.session-item:hover .session-primary-action, .session-item:focus-within .session-primary-action,
.session-item.editing .session-primary-action { opacity: 1; pointer-events: auto; }
.session-item:hover .session-overflow-trigger, .session-item:focus-within .session-overflow-trigger,
.session-overflow-trigger.is-open { opacity: 1; pointer-events: auto; }
.session-item.has-primary:not(.has-launch):hover .session-time, .session-item.has-primary:not(.has-launch):focus-within .session-time,
.session-item.editing .session-time { opacity: 0; pointer-events: none; }
@media (hover: none), (pointer: coarse) {
  .session-item.has-archive .session-archive-action { opacity: 1; pointer-events: auto; }
}
</style>
