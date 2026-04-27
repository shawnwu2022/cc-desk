<template>
  <div class="agent-item" :class="{ expanded: isExpanded }">
    <!-- Agent Header -->
    <div class="agent-header" @click="toggleExpand">
      <img
        class="expand-icon"
        :class="{ expanded: isExpanded }"
        src="@/assets/icons/chevron.svg"
        alt="Toggle"
      />
      <div class="agent-info">
        <span class="agent-name">{{ agent.displayName }}</span>
        <span v-if="agent.sourceType === 'plugin'" class="agent-full-name">{{ agent.name }}</span>
      </div>
      <span v-if="agent.model" class="agent-model">{{ agent.model }}</span>
      <button class="use-btn" @click.stop="emitUseAgent" title="Use this agent">
        <img src="@/assets/icons/use.svg" alt="Use" />
      </button>
    </div>

    <!-- Agent Details (expanded) -->
    <div v-if="isExpanded" class="agent-details">
      <div v-if="agent.description" class="agent-description-full">
        {{ agent.description }}
      </div>
      <div v-else class="agent-description-empty">
        No description available
      </div>
      <div class="agent-invoke-format">
        <span class="invoke-label">Invoke:</span>
        <span class="invoke-value">{{ agent.invokeFormat }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import type { AgentInfo } from '@/types'
import { sendTerminalCommand } from '@/composables/useTerminalCommand'

const props = defineProps<{
  agent: AgentInfo
}>()

const isExpanded = ref(false)

function toggleExpand() {
  isExpanded.value = !isExpanded.value
}

function emitUseAgent() {
  sendTerminalCommand(props.agent.invokeFormat)
}
</script>

<style scoped>
.agent-item {
  background: var(--bg-primary);
  border-radius: 8px;
  padding: 10px 12px;
  transition: background 0.15s ease;
}

.agent-item:hover {
  background: var(--hover-bg);
}

.agent-header {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
}

.expand-icon {
  width: 12px;
  height: 12px;
  color: var(--text-secondary);
  flex-shrink: 0;
  transition: transform 0.15s ease;
}

.expand-icon.expanded {
  transform: rotate(180deg);
}

.agent-info {
  flex: 1;
  min-width: 0;
}

.agent-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.agent-full-name {
  display: block;
  font-size: 11px;
  color: var(--text-tertiary);
  font-family: var(--font-mono);
  margin-top: 2px;
}

.agent-model {
  font-size: 10px;
  padding: 2px 6px;
  border-radius: 4px;
  background: #e3f2fd;
  color: #1565c0;
  flex-shrink: 0;
}

.use-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  border-radius: 4px;
  flex-shrink: 0;
}

.use-btn img {
  width: 14px;
  height: 14px;
}

.use-btn:hover {
  color: var(--accent-color);
  background: var(--bg-tertiary);
}

.agent-details {
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--border-color);
}

.agent-description-full {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.5;
  white-space: pre-wrap;
}

.agent-description-empty {
  font-size: 12px;
  color: var(--text-tertiary);
  font-style: italic;
}

.agent-invoke-format {
  margin-top: 8px;
  display: flex;
  gap: 6px;
  font-size: 11px;
}

.invoke-label {
  color: var(--text-tertiary);
}

.invoke-value {
  font-family: var(--font-mono);
  color: var(--text-primary);
  background: var(--bg-tertiary);
  padding: 2px 6px;
  border-radius: 4px;
}
</style>