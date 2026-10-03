<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from '@/components/ui/AppButton.vue'
import IconButton from '@/components/ui/IconButton.vue'
import AppMenu from '@/components/ui/AppMenu.vue'
import type { UnifiedProjectGroup, ProjectMenuAction } from '@/types/unifiedSession'
import { formatRelativeActivity, useRelativeActivityClock } from '@/utils/relativeTime'
const props = withDefaults(defineProps<{ project: UnifiedProjectGroup; active?: boolean; busy?: boolean; openCount?: number }>(), { active: true, busy: false, openCount: 0 })
const emit = defineEmits<{ open: []; 'new-session': []; action: [action: ProjectMenuAction | 'hide' | 'show'] }>()
const { t, locale } = useI18n()
const now = useRelativeActivityClock()
const menuOpen = ref(false)
const menuRef = ref<InstanceType<typeof AppMenu> | null>(null)
const anchor = ref({ x: 8, y: 8 })
const menuPosition = ref({ position: 'fixed' as const, left: '8px', top: '8px' })
watch(() => props.active, active => { if (!active) menuOpen.value = false }, { flush: 'sync' })
const pathDisplay = computed(() => {
  const path = props.project.projectPath
  if (path.length <= 72) return path
  // Retain both drive/root and final folder, never let a long path push controls.
  return `${path.slice(0, 28)}…${path.slice(-40)}`
})
const age = computed(() => props.project.lastActivityAt > 0 ? formatRelativeActivity(props.project.lastActivityAt, now.value, locale.value === 'zh' ? 'zh' : 'en') : '–')
const menu = computed(() => [
  { id: 'new-session', label: t('newSession'), disabled: props.project.hidden },
  { id: props.project.pinned ? 'unpin' : 'pin', label: t(props.project.pinned ? 'unpin' : 'pin') },
  { id: 'rename', label: t('rename') },
  { id: 'open-project-directory', label: t('openFolder') },
  { id: props.project.hidden ? 'show' : 'hide', label: t(props.project.hidden ? 'show' : 'hide'), disabled: !props.project.hidden && props.openCount > 0 },
  { id: 'remove-project', label: t('removeProject'), danger: true, disabled: props.openCount > 0 },
].map(item => ({ ...item, disabled: item.disabled || props.busy })))
function openMenu(event: MouseEvent) {
  if (!props.active || props.busy) return
  const trigger = event.currentTarget as HTMLElement
  trigger.focus()
  const rect = trigger.getBoundingClientRect()
  anchor.value = { x: rect.right - 200, y: rect.bottom + 4 }
  menuOpen.value = !menuOpen.value
}
function openContext(event: MouseEvent) {
  if (!props.active || props.busy) return
  event.preventDefault()
  anchor.value = { x: event.clientX, y: event.clientY }
  menuOpen.value = true
}
async function placeMenu() {
  await nextTick()
  if (!menuOpen.value) return
  const element = menuRef.value?.$el
  const rect = element instanceof HTMLElement ? element.getBoundingClientRect() : null
  const width = rect?.width || 200
  const height = rect?.height || menu.value.length * 32 + 8
  menuPosition.value = { position: 'fixed',
    left: `${Math.max(8, Math.min(anchor.value.x, window.innerWidth - width - 8))}px`,
    top: `${Math.max(8, Math.min(anchor.value.y, window.innerHeight - height - 8))}px` }
}
function closeOnScroll() { menuOpen.value = false }
watch(() => [menuOpen.value, anchor.value, menu.value], () => { void placeMenu() })
watch(menuOpen, open => {
  if (open) { window.addEventListener('resize', placeMenu); window.addEventListener('scroll', closeOnScroll, true) }
  else { window.removeEventListener('resize', placeMenu); window.removeEventListener('scroll', closeOnScroll, true) }
})
onBeforeUnmount(() => { window.removeEventListener('resize', placeMenu); window.removeEventListener('scroll', closeOnScroll, true) })
function select(id: string) {
  if (!props.active || props.busy) return
  if (id === 'new-session') emit('new-session')
  else emit('action', id as ProjectMenuAction | 'hide' | 'show')
}
</script>
<template>
  <li class="project-row" data-project-row :data-project-path="project.projectPath" :class="{ 'project-row--hidden': project.hidden }" @contextmenu="openContext">
    <div class="project-identity">
      <AppButton variant="ghost" size="compact" class="project-name" :title="project.name" :disabled="busy || project.hidden" @click="emit('open')">{{ project.name }}</AppButton>
      <span data-project-path-display class="project-path" :title="project.projectPath">{{ pathDisplay }}</span>
    </div>
    <span v-if="project.pinned" class="project-pinned" :aria-label="t('pinned')" title="">⌖</span>
    <time class="project-activity" :title="t('projectRecentActivity')">{{ age }}</time>
    <span class="project-sessions" :aria-label="t('projectActiveSessions', { count: project.runningCount })">{{ project.runningCount }}</span>
    <div class="project-menu-anchor">
      <IconButton data-project-overflow :label="t('more')" :disabled="busy" aria-haspopup="menu" :aria-expanded="menuOpen" @click="openMenu"><span aria-hidden="true">⋯</span></IconButton>

    </div>
    <Teleport to="body"><AppMenu ref="menuRef" v-model:open="menuOpen" :style="menuPosition" :label="t('projectActionsLabel')" :items="menu" @select="select" /></Teleport>
  </li>
</template>
<style scoped>
.project-row { display: grid; grid-template-columns: minmax(0, 1fr) 18px 54px 32px 28px; align-items: center; gap: 8px; min-width: 0; padding: 8px 10px; border-bottom: 1px solid var(--border-color); }
.project-row--hidden .project-identity { opacity: .65; }
.project-identity { display: flex; flex-direction: column; align-items: flex-start; min-width: 0; overflow: hidden; gap: 2px; }
.project-name { display: block; max-width: 100%; min-width: 0; text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-primary); }
.project-path { display: block; max-width: 100%; font-size: 11px; color: var(--text-secondary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.project-pinned { grid-column: 2; color: var(--text-secondary); text-align: center; }
.project-activity { grid-column: 3; font-size: 11px; color: var(--text-secondary); text-align: right; white-space: nowrap; }
.project-sessions { grid-column: 4; text-align: center; font-size: 12px; color: var(--text-secondary); }
.project-menu-anchor { grid-column: 5; position: relative; }

</style>
