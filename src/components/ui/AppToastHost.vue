<script setup lang="ts">
import { onBeforeUnmount, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useNotificationsStore } from '@/stores/notifications'
import AppButton from './AppButton.vue'
const { t } = useI18n()
const notifications = useNotificationsStore()
const hovered = new Set<string>()
const focused = new Set<string>()
function hold(id: string, source: 'pointer' | 'focus') {
  ;(source === 'pointer' ? hovered : focused).add(id)
  notifications.pauseToast(id)
}
function release(id: string, source: 'pointer' | 'focus', event?: FocusEvent) {
  if (source === 'focus' && event?.currentTarget instanceof Node && event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return
  ;(source === 'pointer' ? hovered : focused).delete(id)
  if (!hovered.has(id) && !focused.has(id)) notifications.resumeToast(id)
}
watch(() => notifications.toasts.map((toast) => toast.id), (ids) => {
  for (const id of hovered) if (!ids.includes(id)) hovered.delete(id)
  for (const id of focused) if (!ids.includes(id)) focused.delete(id)
})
onBeforeUnmount(() => {
  for (const id of new Set([...hovered, ...focused])) notifications.resumeToast(id)
})
</script>

<template>
  <div class="ui-toast-host" role="status" aria-live="polite" aria-atomic="false" aria-relevant="additions">
    <div v-for="toast in notifications.toasts" :key="toast.id" class="ui-toast" :class="`ui-toast--${toast.kind}`"
      @mouseenter="hold(toast.id, 'pointer')" @mouseleave="release(toast.id, 'pointer')"
      @focusin="hold(toast.id, 'focus')" @focusout="release(toast.id, 'focus', $event)">
      <span class="ui-notice-mark" aria-hidden="true">{{ toast.kind === 'error' ? '!' : toast.kind === 'warning' ? '△' : toast.kind === 'success' ? '✓' : 'i' }}</span>
      <p>{{ t(toast.messageKey) }}</p>
      <AppButton variant="ghost" size="compact" class="ui-icon-button" :aria-label="t('close')" @click="notifications.dismissToast(toast.id)"><span aria-hidden="true">×</span></AppButton>
    </div>
  </div>
</template>
