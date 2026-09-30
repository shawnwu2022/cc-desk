<script setup lang="ts">
import { computed, useAttrs } from 'vue'

defineOptions({ inheritAttrs: false })
const props = withDefaults(defineProps<{
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger'
  size?: 'compact' | 'normal' | 'primary'
  type?: 'button' | 'submit' | 'reset'
  disabled?: boolean
  loading?: boolean
}>(), { variant: 'secondary', type: 'button', disabled: false, loading: false })
const emit = defineEmits<{ click: [event: MouseEvent] }>()
const attrs = useAttrs()
function safeAttrs() {
  const result = { ...attrs }
  if (props.variant === 'danger') delete result.autofocus
  return result
}
const controlSize = computed(() => props.size ?? (props.variant === 'primary' ? 'primary' : 'normal'))
</script>

<template>
  <button v-bind="safeAttrs()" class="ui-button" :class="[`ui-button--${variant}`, `ui-control--${controlSize}`]"
    :type="type" :disabled="disabled || loading" :aria-busy="loading || undefined"
    :data-danger="variant === 'danger' || undefined" @click="emit('click', $event)">
    <span v-if="loading" class="ui-spinner" aria-hidden="true" />
    <slot />
  </button>
</template>
