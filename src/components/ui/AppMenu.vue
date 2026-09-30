<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'

export interface MenuItem { id: string; label: string; disabled?: boolean; hidden?: boolean; danger?: boolean }
const props = withDefaults(defineProps<{ open?: boolean; label: string; items: MenuItem[] }>(), { open: false })
const emit = defineEmits<{ 'update:open': [open: boolean]; close: []; select: [id: string] }>()
const menu = ref<HTMLElement | null>(null)
const items = computed(() => props.items.filter((item) => !item.hidden).sort((a, b) => Number(!!a.danger) - Number(!!b.danger)))
const activeId = ref<string | null>(null)
let opener: HTMLElement | null = null
let returnFocus = true

function enabledItems() { return items.value.filter((item) => !item.disabled) }
function focusItem(id: string | null) {
  activeId.value = id
  const target = [...(menu.value?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])].find((element) => element.dataset.itemId === id)
  ;(target ?? menu.value)?.focus()
}
function close(restore = true) {
  returnFocus = restore
  emit('update:open', false)
  emit('close')
}
function select(item: MenuItem) {
  if (item.disabled || item.hidden) return
  emit('select', item.id)
  close()
}
function onKeydown(event: KeyboardEvent) {
  const enabled = enabledItems()
  if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); return }
  if (event.key === 'Tab') { close(false); return }
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault(); event.stopPropagation()
    const item = enabled.find((item) => item.id === activeId.value)
    if (item) select(item)
    return
  }
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
  event.preventDefault(); event.stopPropagation()
  if (!enabled.length) return
  const index = enabled.findIndex((item) => item.id === activeId.value)
  const next = event.key === 'Home' ? 0 : event.key === 'End' ? enabled.length - 1
    : index < 0 ? (event.key === 'ArrowDown' ? 0 : enabled.length - 1)
    : (index + (event.key === 'ArrowDown' ? 1 : -1) + enabled.length) % enabled.length
  focusItem(enabled[next].id)
}
function onOutsidePointer(event: PointerEvent) {
  if (props.open && event.target instanceof Node && !menu.value?.contains(event.target)) close(false)
}
watch(() => props.open, async (open) => {
  if (open) {
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    returnFocus = true
    document.addEventListener('pointerdown', onOutsidePointer)
    await nextTick()
    if (props.open) focusItem(enabledItems().find((item) => !item.danger)?.id ?? null)
  } else {
    document.removeEventListener('pointerdown', onOutsidePointer)
    if (returnFocus && opener?.isConnected) opener.focus()
  }
}, { immediate: true, flush: 'post' })
watch(items, async () => {
  if (props.open && !enabledItems().some((item) => item.id === activeId.value)) {
    await nextTick(); focusItem(enabledItems().find((item) => !item.danger)?.id ?? null)
  }
})
onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onOutsidePointer)
  if (props.open && returnFocus && opener?.isConnected && menu.value?.contains(document.activeElement)) opener.focus()
})
</script>

<template>
  <div v-if="open" ref="menu" class="ui-menu" role="menu" :aria-label="label" tabindex="-1" @keydown="onKeydown">
    <button v-for="item in items" :key="item.id" role="menuitem" type="button" class="ui-menu-item"
      :class="{ 'ui-menu-item--danger': item.danger }" :data-item-id="item.id" :disabled="item.disabled"
      :tabindex="item.id === activeId ? 0 : -1" @focus="activeId = item.id" @click="select(item)"><slot name="item" :item="item">{{ item.label }}</slot></button>
  </div>
</template>
