<template>
  <div class="mcp-item" :class="{ expanded: isExpanded, disabled: isDisabled }">
    <div
      class="server-header"
      role="button"
      tabindex="0"
      @click="toggleExpand"
      @keydown.enter.self.prevent="toggleExpand"
      @keydown.space.self.prevent="toggleExpand"
    >
      <span class="server-name">{{ server.displayName }}</span>
      <span v-if="server.status" class="server-status">{{ server.status }}</span>
      <button class="expand-btn" :class="{ rotated: isExpanded }" :title="t('toggleDetails')">
        <img src="@/assets/icons/chevron.svg" alt="Toggle" />
      </button>
    </div>

    <div v-if="server.sourceType === 'plugin'" class="server-full-name">
      {{ server.name }}
    </div>

    <div v-if="server.serverType" class="server-meta">
      <span class="server-type">{{ server.serverType }}</span>
    </div>

    <div v-if="isExpanded" class="detail-section">
      <div class="info-row">
        <span class="info-label">Source</span>
        <span class="info-value">{{ server.sourceLabel }}</span>
      </div>
      <div v-if="server.url" class="info-row">
        <span class="info-label">URL</span>
        <span class="info-value mono">{{ server.url }}</span>
      </div>
      <div v-if="server.command" class="info-row">
        <span class="info-label">Command</span>
        <span class="info-value mono">{{ server.command }}</span>
      </div>
      <div v-if="server.args?.length" class="info-row">
        <span class="info-label">Arguments</span>
        <span class="info-value mono">{{ server.args.join(' ') }}</span>
      </div>
      <div v-if="server.description" class="server-description">
        {{ server.description }}
      </div>
      <div v-if="!hasStaticDetails" class="no-details">
        {{ t('noDescription') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { McpServerInfo } from '@/types'

const { t } = useI18n()
const props = defineProps<{
  server: McpServerInfo
}>()

const isExpanded = ref(false)
const isDisabled = computed(() => props.server.enabled === false)
const hasStaticDetails = computed(() => Boolean(
  props.server.description ||
  props.server.sourceLabel ||
  props.server.url ||
  props.server.command ||
  props.server.args?.length
))

function toggleExpand() {
  isExpanded.value = !isExpanded.value
}
</script>

<style scoped>
.mcp-item {
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--bg-primary);
  transition: background 0.15s ease;
}

.mcp-item:hover,
.mcp-item.expanded {
  background: var(--hover-bg);
}

.mcp-item.disabled {
  opacity: 0.55;
}

.mcp-item.disabled .server-name {
  color: var(--text-tertiary);
  text-decoration: line-through;
}

.server-header {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
}

.server-name {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.server-status,
.server-full-name,
.server-meta {
  color: var(--text-tertiary);
  font-size: 11px;
}

.server-status {
  flex-shrink: 0;
}

.server-full-name {
  margin-top: 4px;
  font-family: var(--font-mono);
}

.server-meta {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}

.server-type {
  padding: 1px 4px;
  border-radius: 3px;
  background: var(--bg-tertiary);
  color: var(--text-secondary);
}

.expand-btn {
  display: flex;
  width: 20px;
  height: 20px;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 4px;
  background: transparent;
  cursor: pointer;
  transition: transform 0.2s ease;
}

.expand-btn.rotated {
  transform: rotate(90deg);
}

.expand-btn img {
  width: 12px;
  height: 12px;
}

.detail-section {
  display: grid;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border-color);
}

.info-row {
  display: grid;
  grid-template-columns: 72px minmax(0, 1fr);
  gap: 8px;
  font-size: 12px;
}

.info-label {
  color: var(--text-tertiary);
}

.info-value {
  overflow-wrap: anywhere;
  color: var(--text-primary);
}

.mono {
  font-family: var(--font-mono);
}

.server-description,
.no-details {
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
}

.no-details {
  color: var(--text-tertiary);
  font-style: italic;
}
</style>
