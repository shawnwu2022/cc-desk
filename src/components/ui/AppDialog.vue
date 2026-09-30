<script lang="ts">
// Only the uppermost shared modal owns document focus when dialogs are nested.
const modalStack: HTMLElement[] = []
let nextId = 0
</script>
<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import AppButton from './AppButton.vue'

defineOptions({ inheritAttrs: false })
const props = withDefaults(defineProps<{
  open?: boolean
  title: string
  description?: string
  showClose?: boolean
  variant?: 'dialog' | 'drawer'
}>(), { open: false, showClose: true, variant: 'dialog' })
const emit = defineEmits<{ 'update:open': [open: boolean]; close: [] }>()
const { t } = useI18n()
const panel = ref<HTMLElement | null>(null)
const titleId = `ui-dialog-title-${++nextId}`
const descriptionId = `${titleId}-description`
let opener: HTMLElement | null = null
let ownedPanel: HTMLElement | null = null
function isTop() { return modalStack[modalStack.length - 1] === ownedPanel }
function focusable() {
  return [...(panel.value?.querySelectorAll<HTMLElement>('button, input, select, textarea, a[href], details > summary:first-of-type, [tabindex]') ?? [])]
    .filter((element) => {
      if (element.matches(':disabled, input[type="hidden"], [tabindex="-1"]') || element.closest('[hidden], [inert], [aria-hidden="true"]')) return false
      for (let ancestor: HTMLElement | null = element; ancestor; ancestor = ancestor.parentElement) {
        const style = getComputedStyle(ancestor)
        if (style.display === 'none' || style.visibility === 'hidden') return false
        if (ancestor === panel.value) break
      }
      return true
    })
}
function danger(element: HTMLElement) { return !!element.closest('[data-danger="true"], .ui-button--danger') }
function initialFocus() {
  const controls = focusable()
  controls.filter(danger).forEach((element) => element.removeAttribute('autofocus'))
  const safe = controls.filter((element) => !danger(element))
  ;(safe.find((element) => element.hasAttribute('autofocus')) ?? safe[0] ?? panel.value)?.focus()
}
function close() {
  emit('update:open', false)
  emit('close')
}
function onKeydown(event: KeyboardEvent) {
  if (!isTop()) return
  if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); return }
  if (event.key !== 'Tab') return
  const controls = focusable()
  const first = controls[0]; const last = controls[controls.length - 1]
  if (!first) { event.preventDefault(); panel.value?.focus(); return }
  if (event.shiftKey && (document.activeElement === first || document.activeElement === panel.value)) {
    event.preventDefault(); last.focus()
  } else if (!event.shiftKey && (document.activeElement === last || document.activeElement === panel.value)) {
    event.preventDefault(); first.focus()
  }
}
function onFocusIn(event: FocusEvent) {
  if (props.open && isTop() && event.target instanceof Node && !panel.value?.contains(event.target)) initialFocus()
}
function releaseFocus() {
  const wasTop = isTop()
  const index = ownedPanel ? modalStack.indexOf(ownedPanel) : -1
  if (index >= 0) modalStack.splice(index, 1)
  ownedPanel = null
  document.removeEventListener('focusin', onFocusIn)
  if (wasTop && opener?.isConnected) opener.focus()
}
watch(() => props.open, async (open) => {
  if (!open) { releaseFocus(); return }
  opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
  await nextTick()
  if (!props.open || !panel.value) return
  ownedPanel = panel.value
  modalStack.push(ownedPanel)
  document.addEventListener('focusin', onFocusIn)
  initialFocus()
}, { immediate: true, flush: 'post' })
onBeforeUnmount(releaseFocus)
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="ui-modal-backdrop" :class="{ 'ui-modal-backdrop--drawer': variant === 'drawer' }">
      <section v-bind="$attrs" ref="panel" class="ui-dialog" :class="{ 'ui-drawer': variant === 'drawer' }" role="dialog"
        aria-modal="true" :aria-labelledby="titleId" :aria-describedby="description ? descriptionId : undefined" tabindex="-1" @keydown="onKeydown">
        <header class="ui-dialog-header">
          <h2 :id="titleId">{{ title }}</h2>
          <AppButton v-if="showClose" variant="ghost" size="compact" class="ui-icon-button" :aria-label="t('close')" @click="close"><span aria-hidden="true">×</span></AppButton>
        </header>
        <div class="ui-dialog-body">
          <p v-if="description" :id="descriptionId" class="ui-description">{{ description }}</p>
          <slot />
        </div>
        <footer v-if="$slots.footer" class="ui-dialog-footer"><slot name="footer" /></footer>
      </section>
    </div>
  </Teleport>
</template>
