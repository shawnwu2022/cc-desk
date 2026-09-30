<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppMenu from '@/components/ui/AppMenu.vue'
import type { SessionMenuAction, SessionMenuActionDefinition } from '@/types/unifiedSession'

const props = defineProps<{ open: boolean; actions: SessionMenuActionDefinition[]; anchor: { x: number; y: number } }>()
const emit = defineEmits<{ 'update:open': [open: boolean]; 'menu-action': [action: SessionMenuAction] }>()
const { t } = useI18n()
const menu = ref<InstanceType<typeof AppMenu> | null>(null)
const position = ref({ left: '8px', top: '8px' })
const items = computed(() => props.actions.map((action) => ({ ...action, label: t(action.labelKey) })))
async function placeMenu() {
  await nextTick()
  if (!props.open) return
  const element = menu.value?.$el
  const rect = element instanceof HTMLElement ? element.getBoundingClientRect() : null
  const width = rect?.width || 200
  const height = rect?.height || Math.min(items.value.length * 32 + 8, window.innerHeight - 16)
  position.value = {
    left: `${Math.max(8, Math.min(props.anchor.x, window.innerWidth - width - 8))}px`,
    top: `${Math.max(8, Math.min(props.anchor.y, window.innerHeight - height - 8))}px`,
  }
}
watch(() => [props.open, props.anchor.x, props.anchor.y, items.value], () => { void placeMenu() }, { immediate: true })
watch(() => props.open, (open) => {
  if (open) window.addEventListener('resize', placeMenu)
  else window.removeEventListener('resize', placeMenu)
}, { immediate: true })
onBeforeUnmount(() => { window.removeEventListener('resize', placeMenu) })
function selectAction(id: string) {
  const action = props.actions.find((item) => item.id === id && !item.disabled)
  if (action) emit('menu-action', action.id)
}
</script>

<template>
  <Teleport to="body">
    <AppMenu ref="menu" class="session-overflow-menu" :style="position" :open="open"
      :label="t('sessionActionsLabel')" :items="items" @update:open="emit('update:open', $event)" @select="selectAction" />
  </Teleport>
</template>

<style scoped>
.session-overflow-menu { position: fixed; max-height: calc(100vh - 16px); }
</style>
