<script setup lang="ts">
import AppButton from './AppButton.vue'
withDefaults(defineProps<{ kind?: 'info' | 'success' | 'warning' | 'error'; message: string; actionLabel?: string }>(), { kind: 'info' })
const emit = defineEmits<{ action: [] }>()
</script>

<template>
  <div class="ui-notice" :class="`ui-notice--${kind}`" role="status">
    <span class="ui-notice-mark" aria-hidden="true">{{ kind === 'error' ? '!' : kind === 'warning' ? '△' : kind === 'success' ? '✓' : 'i' }}</span>
    <p>{{ message }}</p>
    <AppButton v-if="actionLabel" variant="ghost" size="compact" @click="emit('action')">{{ actionLabel }}</AppButton>
    <slot />
  </div>
</template>
