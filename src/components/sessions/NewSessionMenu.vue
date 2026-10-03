<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import AppMenu from '@/components/ui/AppMenu.vue'
import claudeIcon from '@/assets/icons/cli/claude.svg'
import codexIcon from '@/assets/icons/cli/codex.svg'
import type { NewSessionRequest, UnifiedCliKind } from '@/types/unifiedSession'
const props = withDefaults(defineProps<{ open: boolean; active?: boolean; anchor: { x: number; y: number }; availability?: Partial<Record<UnifiedCliKind, 'unknown' | 'available' | 'unavailable'>> }>(), { active: true, availability: () => ({}) })
const emit = defineEmits<{ 'update:open': [open: boolean]; select: [intent: NonNullable<NewSessionRequest['intent']>] }>()
const { t } = useI18n()
const app = useAppStore()
const menu = ref<InstanceType<typeof AppMenu> | null>(null)
const position = ref({ left: '8px', top: '8px' })
const items = computed(() => [
  { id: 'claude', label: props.availability.claude === 'unavailable' ? t('newSessionToolUnavailable', { cli: 'Claude Code' }) : 'Claude Code', disabled: props.availability.claude === 'unavailable' },
  { id: 'codex', label: props.availability.codex === 'unavailable' ? t('newSessionToolUnavailable', { cli: 'Codex CLI' }) : 'Codex CLI', disabled: props.availability.codex === 'unavailable' },
].sort((left, right) => Number(right.id === app.defaultNewCli) - Number(left.id === app.defaultNewCli)).concat([
  { id: 'restore', label: t('newSessionRestore'), disabled: false }, { id: 'options', label: t('newSessionMoreOptions'), disabled: false },
]))
async function place() {
  await nextTick()
  if (!props.open) return
  const el = menu.value?.$el
  const rect = el instanceof HTMLElement ? el.getBoundingClientRect() : null
  position.value = { left: `${Math.max(8, Math.min(props.anchor.x, window.innerWidth - (rect?.width || 240) - 8))}px`,
    top: `${Math.max(8, Math.min(props.anchor.y, window.innerHeight - (rect?.height || 152) - 8))}px` }
}
watch(() => [props.open, props.anchor, items.value], () => { void place() }, { immediate: true })
watch(() => props.open, open => { if (open) window.addEventListener('resize', place); else window.removeEventListener('resize', place) })
watch(() => props.active, active => { if (!active) emit('update:open', false) }, { flush: 'sync' })
onBeforeUnmount(() => window.removeEventListener('resize', place))
</script>
<template>
  <Teleport to="body">
    <AppMenu ref="menu" :open="open && active" :label="t('newSessionTitle')" :items="items" class="new-session-menu" :style="position"
      @update:open="emit('update:open', $event)" @select="emit('select', $event as NonNullable<NewSessionRequest['intent']>)">
      <template #item="{ item }"><img v-if="item.id === 'claude' || item.id === 'codex'" :src="item.id === 'claude' ? claudeIcon : codexIcon" width="16" height="16" alt="" aria-hidden="true" /><span>{{ item.label }}</span></template>
    </AppMenu>
  </Teleport>
</template>
<style scoped>
.new-session-menu { position: fixed; max-width: calc(100vw - 16px); max-height: calc(100vh - 16px); }
.new-session-menu :deep(.ui-menu-item) { display: flex; align-items: center; gap: 8px; }
.new-session-menu :deep([data-item-id='restore']) { border-top: 1px solid var(--border-color); margin-top: 4px; padding-top: 8px; }
</style>
