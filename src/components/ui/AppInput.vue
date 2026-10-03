<script lang="ts">
let nextId = 0
</script>
<script setup lang="ts">
import { computed } from 'vue'

defineOptions({ inheritAttrs: false })
const props = withDefaults(defineProps<{
  modelValue: string
  label?: string
  id?: string
  type?: string
  size?: 'compact' | 'normal' | 'primary'
  invalid?: boolean
  describedBy?: string
  disabled?: boolean
  readonly?: boolean
}>(), { type: 'text', size: 'normal', invalid: false, disabled: false, readonly: false })
const emit = defineEmits<{ 'update:modelValue': [value: string] }>()
const generatedId = `ui-input-${++nextId}`
const inputId = computed(() => props.id ?? generatedId)
</script>

<template>
  <div class="ui-field">
    <label v-if="label" :for="inputId" class="ui-field-label">{{ label }}</label>
    <input v-bind="$attrs" :id="inputId" class="ui-input" :class="`ui-control--${size}`" :type="type"
      :value="modelValue" :disabled="disabled" :readonly="readonly" :aria-invalid="invalid || undefined"
      :aria-describedby="describedBy ?? $attrs['aria-describedby'] as string | undefined"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)" />
  </div>
</template>
