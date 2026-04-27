<template>
  <div class="panel-header">
    <span class="panel-title">{{ title }}</span>
    <div class="header-actions">
      <slot name="actions" />
      <button class="close-btn" @click="$emit('close')">
        <img src="@/assets/icons/close.svg" alt="Close" />
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  title: string
}>()

defineEmits<{
  close: []
}>()
</script>

<style scoped>
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color);
}

.panel-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.header-actions {
  display: flex;
  gap: 4px;
}

.close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  padding: 0;
}

.close-btn img {
  width: 16px;
  height: 16px;
}

.close-btn:hover {
  color: var(--text-primary);
}
</style>
