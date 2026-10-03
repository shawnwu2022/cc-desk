<script setup lang="ts">
import AppButton from './AppButton.vue'
defineProps<{ title: string; description: string; actionLabel?: string }>()
const emit = defineEmits<{ action: [] }>()
</script>

<template>
  <section class="ui-empty-state">
    <h3>{{ title }}</h3>
    <p class="ui-description">{{ description }}</p>
    <AppButton v-if="actionLabel" @click="emit('action')">{{ actionLabel }}</AppButton>
  </section>
</template>
