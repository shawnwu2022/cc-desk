<template>
  <div class="mcp-panel">
    <PanelHeader :title="t('mcpServers')" @close="$emit('close')">
      <template #actions>
        <button class="action-btn" :title="t('refreshMcp')" @click="handleRefresh">
          <img src="@/assets/icons/refresh.svg" :alt="t('refreshMcp')" />
        </button>
      </template>
    </PanelHeader>

    <div class="panel-content">
      <div class="panel-desc">{{ t('mcpDesc') }}</div>

      <div v-if="loading" class="loading-state">
        <span class="loading-text">{{ t('loadingMcp') }}</span>
      </div>

      <div v-else-if="allServers.length === 0" class="empty-state">
        <span class="empty-text">{{ t('noMcpConfigured') }}</span>
        <span class="empty-hint">{{ t('addMcpHint') }}</span>
      </div>

      <div v-else class="servers-list">
        <McpGroup
          v-if="projectServers.length > 0"
          :title="t('project')"
          :expanded="sidebarStore.mcpExpandedGroups.project"
          :count="projectServers.length"
          :servers="projectServers"
          @toggle="sidebarStore.toggleMcpGroup('project')"
        />
        <McpGroup
          v-if="userServers.length > 0"
          :title="t('user')"
          :expanded="sidebarStore.mcpExpandedGroups.user"
          :count="userServers.length"
          :servers="userServers"
          @toggle="sidebarStore.toggleMcpGroup('user')"
        />
        <McpGroup
          v-if="pluginServers.length > 0"
          :title="t('plugin')"
          :expanded="sidebarStore.mcpExpandedGroups.plugin"
          :count="pluginServers.length"
          :servers="pluginServers"
          @toggle="sidebarStore.toggleMcpGroup('plugin')"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAppStore } from '@/stores/app'
import { useSidebarStore } from '@/stores/sidebar'
import McpGroup from './McpGroup.vue'
import PanelHeader from '../sidebar/PanelHeader.vue'

const { t } = useI18n()
const sidebarStore = useSidebarStore()
const appStore = useAppStore()

const servers = computed(() => sidebarStore.mcpServers)
const loading = computed(() => sidebarStore.mcpServersLoading)
const allServers = computed(() => servers.value)
const pluginServers = computed(() => servers.value.filter(server => server.sourceType === 'plugin'))
const userServers = computed(() => servers.value.filter(server => server.sourceType === 'user'))
const projectServers = computed(() => servers.value.filter(server => server.sourceType === 'project'))

function handleRefresh() {
  if (appStore.cwd) void sidebarStore.loadMcpServers(appStore.cwd)
}

onMounted(() => {
  if (appStore.cwd && sidebarStore.mcpServers.length === 0) {
    void sidebarStore.loadMcpServers(appStore.cwd)
  }
})
</script>

<style scoped>
.mcp-panel {
  display: flex;
  height: 100%;
  flex-direction: column;
  background: var(--bg-secondary);
}

.mcp-panel :deep(.action-btn) {
  display: flex;
  width: 24px;
  height: 24px;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}

.mcp-panel :deep(.action-btn img) {
  width: 16px;
  height: 16px;
}

.mcp-panel :deep(.action-btn:hover) {
  color: var(--text-primary);
}

.panel-content {
  display: flex;
  overflow-y: auto;
  flex: 1;
  flex-direction: column;
}

.panel-desc {
  padding: 8px 12px;
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.5;
}

.loading-state,
.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  flex-direction: column;
  gap: 8px;
  padding: 24px;
}

.loading-text,
.empty-text {
  color: var(--text-secondary);
  font-size: 13px;
}

.empty-hint {
  color: var(--text-tertiary);
  font-size: 12px;
}

.servers-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px 0;
}
</style>
