<template>
  <footer class="info-bar">
    <span class="status-text">{{ status }}</span>
  </footer>
</template>

<script setup lang="ts">
defineProps<{
  status: string
}>()
</script>

<style scoped>
.info-bar {
  display: flex;
  align-items: center;
  height: 28px;
  padding: 0 12px;
  background: var(--bg-secondary);
  border-top: 1px solid var(--border-color);
}

.status-text {
  font-size: 12px;
  color: var(--text-secondary);
}
</style>