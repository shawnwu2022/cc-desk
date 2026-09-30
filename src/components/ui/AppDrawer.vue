<script setup lang="ts">
import AppDialog from './AppDialog.vue'
withDefaults(defineProps<{ open?: boolean; title: string; description?: string; showClose?: boolean }>(), { open: false, showClose: true })
const emit = defineEmits<{ 'update:open': [open: boolean]; close: [] }>()
</script>

<template>
  <AppDialog :open="open" :title="title" :description="description" :show-close="showClose" variant="drawer"
    @update:open="emit('update:open', $event)" @close="emit('close')">
    <slot />
    <template v-if="$slots.footer" #footer><slot name="footer" /></template>
  </AppDialog>
</template>
