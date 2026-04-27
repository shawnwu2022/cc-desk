<template>
  <aside class="icon-bar">
    <!-- 顶部图标组 -->
    <div class="icon-group top">
      <!-- Sessions -->
      <button
        class="icon-btn"
        :class="{ active: activePanel === 'sessions' }"
        @click="$emit('toggle', 'sessions')"
        title="Sessions"
      >
        <img src="@/assets/icons/sessions.svg" alt="Sessions" />
      </button>

      <!-- Skills -->
      <button
        class="icon-btn"
        :class="{ active: activePanel === 'skills' }"
        @click="$emit('toggle', 'skills')"
        title="Skills"
      >
        <img src="@/assets/icons/skills.svg" alt="Skills" />
      </button>

      <!-- Agents -->
      <button
        class="icon-btn"
        :class="{ active: activePanel === 'agents' }"
        @click="$emit('toggle', 'agents')"
        title="Agents"
      >
        <img src="@/assets/icons/agents.svg" alt="Agents" />
      </button>

      <!-- MCP -->
      <button
          class="icon-btn"
          :class="{ active: activePanel === 'mcp' }"
          @click="$emit('toggle', 'mcp')"
          title="MCP Servers"
      >
        <img src="@/assets/icons/mcp.svg" alt="MCP" />
      </button>

      <!-- Plugins -->
      <button
        class="icon-btn"
        :class="{ active: activePanel === 'plugins' }"
        @click="$emit('toggle', 'plugins')"
        title="Plugins"
      >
        <img src="@/assets/icons/plugins.svg" alt="Plugins" />
      </button>

    </div>

    <!-- 分隔线 -->
    <div class="separator"></div>

    <!-- 文件夹按钮 -->
    <button
      class="icon-btn folder-btn"
      @click="$emit('openFolder')"
      title="Open folder"
    >
      <img src="@/assets/icons/folder.svg" alt="Folder" />
    </button>
  </aside>
</template>

<script setup lang="ts">
import type { SidebarPanelType } from '@/stores/sidebar'

defineProps<{
  activePanel: SidebarPanelType
}>()

defineEmits<{
  toggle: [panel: SidebarPanelType]
  openFolder: []
}>()
</script>

<style scoped>
.icon-bar {
  width: 48px;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  padding: 8px 4px;
  gap: 4px;
  flex-shrink: 0;
}

.icon-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.icon-btn {
  position: relative;
  width: 40px;
  height: 40px;
  margin: 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 8px;
  transition: all 0.15s ease;
}

.icon-btn img {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
}

.icon-btn:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.icon-btn.active {
  background: var(--selected-bg);
  color: var(--accent-color);
}

.icon-btn.active::before {
  content: '';
  position: absolute;
  left: -4px;
  top: 8px;
  bottom: 8px;
  width: 3px;
  background: var(--accent-color);
  border-radius: 0 2px 2px 0;
}

.separator {
  height: 1px;
  background: var(--border-color);
  margin: 8px 8px;
}

.folder-btn {
  margin-top: auto;
}
</style>