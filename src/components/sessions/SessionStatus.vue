<template>
  <div class="session-status" v-if="tab">
    <button
      class="restart-btn"
      @click="$emit('restart')"
    >
      <img src="@/assets/icons/refresh.svg" alt="Restart" />
      {{ tab.status === 'stopped' ? 'Restart' : 'Restart' }}
    </button>
  </div>
</template>

<script setup lang="ts">
import type { TerminalTab } from '@/stores/session'

defineProps<{
  tab: TerminalTab | null
  projectPath: string
}>()

defineEmits<{
  restart: []
}>()
</script>

<style scoped>
.session-status {
  border-top: 1px solid var(--border-color);
  padding: 8px;
}

.restart-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 6px;
  font-size: 13px;
  transition: all 0.15s ease;
}

.restart-btn img {
  width: 14px;
  height: 14px;
}

.restart-btn:hover {
  border-color: var(--accent-color);
  color: var(--accent-color);
}
</style>
