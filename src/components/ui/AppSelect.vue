<script lang="ts">
let nextId = 0
</script>
<script setup lang="ts">
import { computed } from 'vue'

defineOptions({ inheritAttrs: false })
const props = withDefaults(defineProps<{
  modelValue: string
  options: { value: string; label: string; disabled?: boolean }[]
  label?: string
  id?: string
  size?: 'compact' | 'normal' | 'primary'
  invalid?: boolean
  describedBy?: string
  disabled?: boolean
}>(), { size: 'normal', invalid: false, disabled: false })
const emit = defineEmits<{ 'update:modelValue': [value: string] }>()
const generatedId = `ui-select-${++nextId}`
const selectId = computed(() => props.id ?? generatedId)
</script>

<template>
  <div class="ui-field">
    <label v-if="label" :for="selectId" class="ui-field-label">{{ label }}</label>
    <select v-bind="$attrs" :id="selectId" class="ui-select" :class="`ui-control--${size}`" :value="modelValue"
      :disabled="disabled" :aria-invalid="invalid || undefined"
      :aria-describedby="describedBy ?? $attrs['aria-describedby'] as string | undefined"
      @change="emit('update:modelValue', ($event.target as HTMLSelectElement).value)">
      <option v-for="option in options" :key="option.value" :value="option.value" :disabled="option.disabled">{{ option.label }}</option>
    </select>
  </div>
</template>
