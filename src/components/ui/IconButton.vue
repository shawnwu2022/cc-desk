<script setup lang="ts">
import AppButton from './AppButton.vue'
import AppTooltip from './AppTooltip.vue'

defineOptions({ inheritAttrs: false })
withDefaults(defineProps<{
  label: string
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger'
  size?: 'compact' | 'normal' | 'primary'
  disabled?: boolean
  loading?: boolean
}>(), { variant: 'ghost', size: 'compact', disabled: false, loading: false })
const emit = defineEmits<{ click: [event: MouseEvent] }>()
</script>

<template>
  <AppTooltip :text="label">
    <AppButton v-bind="$attrs" class="ui-icon-button" :aria-label="label" :variant="variant" :size="size"
      :disabled="disabled" :loading="loading" @click="emit('click', $event)">
      <span aria-hidden="true"><slot /></span>
    </AppButton>
  </AppTooltip>
</template>
