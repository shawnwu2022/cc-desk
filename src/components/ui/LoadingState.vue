<script setup lang="ts">
import { computed } from 'vue'
const props = withDefaults(defineProps<{ label: string; rows?: number }>(), { rows: 3 })
const count = computed(() => Number.isFinite(props.rows) ? Math.min(12, Math.max(1, Math.floor(props.rows))) : 3)
</script>

<template>
  <div class="ui-loading-state" role="status" aria-busy="true">
    <span class="ui-sr-only">{{ label }}</span>
    <div v-for="row in count" :key="row" class="ui-skeleton" aria-hidden="true" />
  </div>
</template>
