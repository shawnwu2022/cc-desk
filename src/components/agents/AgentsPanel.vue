<template>
  <div class="agents-panel">
    <!-- Header -->
    <PanelHeader title="Agents" @close="$emit('close')">
      <template #actions>
        <button class="action-btn" @click="handleRefresh" title="Refresh agents">
          <img src="@/assets/icons/refresh.svg" alt="Refresh" />
        </button>
      </template>
    </PanelHeader>

    <!-- Loading -->
    <div v-if="loading" class="loading-state">
      <span class="loading-text">Loading agents...</span>
    </div>

    <!-- Error -->
    <div v-else-if="error" class="error-state">
      <span class="error-text">{{ error }}</span>
    </div>

    <!-- Empty -->
    <div v-else-if="allAgents.length === 0" class="empty-state">
      <span class="empty-text">No agents available</span>
    </div>

    <!-- Agents List -->
    <div v-else class="agents-list">
      <!-- Project Agents -->
      <AgentGroup
        v-if="projectAgents.length > 0"
        title="Project"
        :expanded="sidebarStore.agentsExpandedGroups.project"
        :count="projectAgents.length"
        :agents="projectAgents"
        @toggle="sidebarStore.toggleAgentGroup('project')"
      />

      <!-- User Agents -->
      <AgentGroup
        v-if="userAgents.length > 0"
        title="User"
        :expanded="sidebarStore.agentsExpandedGroups.user"
        :count="userAgents.length"
        :agents="userAgents"
        @toggle="sidebarStore.toggleAgentGroup('user')"
      />

      <!-- Plugin Agents -->
      <AgentGroup
        v-if="pluginAgents.length > 0"
        title="Plugin"
        :expanded="sidebarStore.agentsExpandedGroups.plugin"
        :count="pluginAgents.length"
        :agents="pluginAgents"
        @toggle="sidebarStore.toggleAgentGroup('plugin')"
      />

      <!-- Built-in Agents -->
      <AgentGroup
        v-if="builtinAgents.length > 0"
        title="Built-in"
        :expanded="sidebarStore.agentsExpandedGroups.builtin"
        :count="builtinAgents.length"
        :agents="builtinAgents"
        @toggle="sidebarStore.toggleAgentGroup('builtin')"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useSidebarStore } from '@/stores/sidebar'
import { useAppStore } from '@/stores/app'
import { getAllAgents } from '@/api/tauri'
import type { AgentInfo } from '@/types'
import AgentGroup from './AgentGroup.vue'
import PanelHeader from '../sidebar/PanelHeader.vue'

const sidebarStore = useSidebarStore()
const appStore = useAppStore()

const agents = ref<AgentInfo[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const loadedCwd = ref<string | null>(null)

// 所有 agents
const allAgents = computed(() => agents.value)

// 按 sourceType 分组
const builtinAgents = computed(() =>
  agents.value.filter(a => a.sourceType === 'builtin')
)

const pluginAgents = computed(() =>
  agents.value.filter(a => a.sourceType === 'plugin')
)

const userAgents = computed(() =>
  agents.value.filter(a => a.sourceType === 'user')
)

const projectAgents = computed(() =>
  agents.value.filter(a => a.sourceType === 'project')
)

// 加载 Agents（带缓存）
async function loadAgents(projectPath: string, force = false) {
  if (!projectPath) return
  if (!force && loadedCwd.value === projectPath && agents.value.length > 0) return

  loading.value = true
  error.value = null

  try {
    const result = await getAllAgents(projectPath)
    agents.value = result
    loadedCwd.value = projectPath
  } catch (err) {
    error.value = 'Failed to load agents'
    console.error('[AgentsPanel] Failed to load agents:', err)
  } finally {
    loading.value = false
  }
}

function handleRefresh() {
  if (appStore.cwd) {
    loadAgents(appStore.cwd, true)
  }
}

onMounted(() => {
  if (appStore.cwd) {
    loadAgents(appStore.cwd)
  }
})
</script>

<style scoped>
.agents-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-secondary);
}

/* action-btn slot 样式 */
.agents-panel :deep(.action-btn) {
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

.agents-panel :deep(.action-btn img) {
  width: 16px;
  height: 16px;
}

.agents-panel :deep(.action-btn:hover) {
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

.agents-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px 0;
}
</style>
