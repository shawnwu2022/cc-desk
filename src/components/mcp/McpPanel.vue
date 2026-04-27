<template>
  <div class="mcp-panel">
    <!-- Header -->
    <PanelHeader title="MCP Servers" @close="$emit('close')">
      <template #actions>
        <button class="action-btn" @click="handleRefresh" title="Refresh MCP servers">
          <img src="@/assets/icons/refresh.svg" alt="Refresh" />
        </button>
      </template>
    </PanelHeader>

    <!-- Loading -->
    <div v-if="loading" class="loading-state">
      <span class="loading-text">Loading MCP servers...</span>
    </div>

    <!-- Error -->
    <div v-else-if="error" class="error-state">
      <span class="error-text">{{ error }}</span>
    </div>

    <!-- Empty -->
    <div v-else-if="allServers.length === 0" class="empty-state">
      <span class="empty-text">No MCP servers configured</span>
      <span class="empty-hint">Add MCP servers via claude mcp add</span>
    </div>

    <!-- Servers List -->
    <div v-else class="servers-list">
      <!-- Project Servers -->
      <McpGroup
        v-if="projectServers.length > 0"
        title="Project"
        :expanded="sidebarStore.mcpExpandedGroups.project"
        :count="projectServers.length"
        :servers="projectServers"
        @toggle="sidebarStore.toggleMcpGroup('project')"
      />

      <!-- User Servers -->
      <McpGroup
        v-if="userServers.length > 0"
        title="User"
        :expanded="sidebarStore.mcpExpandedGroups.user"
        :count="userServers.length"
        :servers="userServers"
        @toggle="sidebarStore.toggleMcpGroup('user')"
      />

      <!-- Plugin Servers -->
      <McpGroup
        v-if="pluginServers.length > 0"
        title="Plugin"
        :expanded="sidebarStore.mcpExpandedGroups.plugin"
        :count="pluginServers.length"
        :servers="pluginServers"
        @toggle="sidebarStore.toggleMcpGroup('plugin')"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, provide } from 'vue'
import { useSidebarStore } from '@/stores/sidebar'
import { useAppStore } from '@/stores/app'
import { getAllMcpServers } from '@/api/tauri'
import type { McpServerInfo, McpServerDetail } from '@/types'
import McpGroup from './McpGroup.vue'
import PanelHeader from '../sidebar/PanelHeader.vue'

const sidebarStore = useSidebarStore()
const appStore = useAppStore()

const servers = ref<McpServerInfo[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const loadedCwd = ref<string | null>(null)

// MCP Detail 缓存（提供给子组件）
const detailCache = new Map<string, McpServerDetail>()
provide('mcpDetailCache', detailCache)

// 所有 servers
const allServers = computed(() => servers.value)

// 按 sourceType 分组
const pluginServers = computed(() =>
  servers.value.filter(s => s.sourceType === 'plugin')
)

const userServers = computed(() =>
  servers.value.filter(s => s.sourceType === 'user')
)

const projectServers = computed(() =>
  servers.value.filter(s => s.sourceType === 'project')
)

// 加载 MCP Servers（带缓存）
async function loadMcpServers(projectPath: string, force = false) {
  if (!projectPath) return
  if (!force && loadedCwd.value === projectPath && servers.value.length > 0) return

  loading.value = true
  error.value = null

  try {
    const result = await getAllMcpServers(projectPath)
    servers.value = result
    loadedCwd.value = projectPath
  } catch (err) {
    error.value = 'Failed to load MCP servers'
    console.error('[McpPanel] Failed to load MCP servers:', err)
  } finally {
    loading.value = false
  }
}

function handleRefresh() {
  if (appStore.cwd) {
    loadMcpServers(appStore.cwd, true)
  }
}

onMounted(() => {
  if (appStore.cwd) {
    loadMcpServers(appStore.cwd)
  }
})
</script>

<style scoped>
.mcp-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-secondary);
}

/* action-btn slot 样式 */
.mcp-panel :deep(.action-btn) {
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

.mcp-panel :deep(.action-btn img) {
  width: 16px;
  height: 16px;
}

.mcp-panel :deep(.action-btn:hover) {
  color: var(--text-primary);
}

.loading-state,
.error-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 24px;
  gap: 8px;
}

.loading-text,
.error-text,
.empty-text {
  font-size: 13px;
  color: var(--text-secondary);
}

.error-text {
  color: var(--error-color);
}

.empty-hint {
  font-size: 12px;
  color: var(--text-tertiary);
}

.servers-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px 0;
}
</style>
