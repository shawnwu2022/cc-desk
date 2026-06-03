<template>
  <div class="plugin-item" :class="{ expanded: isExpanded }">
    <!-- Plugin Header -->
    <div class="plugin-header" @click="toggleExpand">
      <img
        class="expand-icon"
        :class="{ expanded: isExpanded }"
        src="@/assets/icons/chevron.svg"
        alt="Toggle"
      />
      <span class="plugin-name">{{ plugin.name }}</span>
      <span class="plugin-version">v{{ plugin.version }}</span>
    </div>

    <!-- Plugin ID -->
    <div class="plugin-id">{{ plugin.id }}</div>

    <!-- Plugin Components Tags (collapsed view) -->
    <div v-if="!isExpanded && hasComponents" class="plugin-components">
      <span v-if="plugin.skills?.length" class="component-tag skills">
        {{ plugin.skills.length }} Skills
      </span>
      <span v-if="plugin.agents?.length" class="component-tag agents">
        {{ plugin.agents.length }} Agents
      </span>
      <span v-if="plugin.mcpServers && Object.keys(plugin.mcpServers).length > 0" class="component-tag mcp">
        {{ Object.keys(plugin.mcpServers).length }} MCP
      </span>
    </div>

    <!-- Expanded Components List -->
    <div v-if="isExpanded" class="plugin-expanded">
      <!-- Skills -->
      <div v-if="plugin.skills?.length" class="component-section">
        <div class="section-title">{{ t('skills') }}</div>
        <div v-for="skill in plugin.skills" :key="skill.name" class="component-item" :class="{ expanded: expandedSkills[skill.name] }">
          <div class="item-header" @click="toggleSkillDetail(skill.name)">
            <img class="item-expand-icon" :class="{ expanded: expandedSkills[skill.name] }" src="@/assets/icons/chevron.svg" alt="Toggle" />
            <span class="item-name">{{ skill.name }}</span>
            <button class="item-use-btn" @click.stop="useSkill(skill.invokeFormat)" :title="t('useThisSkill')">
              <img src="@/assets/icons/skills.svg" :alt="t('skills')" class="item-icon" />
            </button>
          </div>
          <div v-if="expandedSkills[skill.name]" class="item-detail">
            <div v-if="skill.description" class="item-desc-full">{{ skill.description }}</div>
            <div v-else class="item-desc-empty">{{ t('noDescription') }}</div>
            <div class="item-invoke-format">
              <span class="invoke-label">Invoke:</span>
              <span class="invoke-value">{{ skill.invokeFormat }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Agents -->
      <div v-if="plugin.agents?.length" class="component-section">
        <div class="section-title">{{ t('agents') }}</div>
        <div v-for="agent in plugin.agents" :key="agent.name" class="component-item" :class="{ expanded: expandedAgents[agent.name] }">
          <div class="item-header" @click="toggleAgentDetail(agent.name)">
            <img class="item-expand-icon" :class="{ expanded: expandedAgents[agent.name] }" src="@/assets/icons/chevron.svg" alt="Toggle" />
            <span class="item-name">{{ agent.name }}</span>
            <button class="item-use-btn" @click.stop="useAgent(agent.invokeFormat)" :title="t('useThisAgent')">
              <img src="@/assets/icons/agents.svg" :alt="t('agents')" class="item-icon" />
            </button>
          </div>
          <div v-if="expandedAgents[agent.name]" class="item-detail">
            <div v-if="agent.description" class="item-desc-full">{{ agent.description }}</div>
            <div v-else class="item-desc-empty">{{ t('noDescription') }}</div>
            <div class="item-invoke-format">
              <span class="invoke-label">Invoke:</span>
              <span class="invoke-value">{{ agent.invokeFormat }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- MCP Servers -->
      <div v-if="plugin.mcpServers && Object.keys(plugin.mcpServers).length > 0" class="component-section">
        <div class="section-title">{{ t('mcpServers') }}</div>
        <div v-for="(serverConfig, serverName) in plugin.mcpServers" :key="serverName" class="component-item mcp-server">
          <span class="item-name">{{ serverName }}</span>
          <span v-if="serverConfig.type" class="item-type">{{ serverConfig.type }}</span>
        </div>
      </div>

      <!-- Empty state -->
      <div v-if="!hasComponents" class="no-components">
        {{ t('noDescription') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { PluginInfo } from '@/types'
import { sendTerminalCommand } from '@/composables/useTerminalCommand'

const { t } = useI18n()
const props = defineProps<{
  plugin: PluginInfo
}>()

const isExpanded = ref(false)
const expandedSkills = ref<Record<string, boolean>>({})
const expandedAgents = ref<Record<string, boolean>>({})

const hasComponents = computed(() => {
  return (props.plugin.skills && props.plugin.skills.length > 0) ||
    (props.plugin.agents && props.plugin.agents.length > 0) ||
    (props.plugin.mcpServers && Object.keys(props.plugin.mcpServers).length > 0)
})

function toggleExpand() {
  isExpanded.value = !isExpanded.value
}

function toggleSkillDetail(name: string) {
  expandedSkills.value[name] = !expandedSkills.value[name]
}

function toggleAgentDetail(name: string) {
  expandedAgents.value[name] = !expandedAgents.value[name]
}

function useSkill(invokeFormat: string) {
  sendTerminalCommand(invokeFormat)
}

function useAgent(invokeFormat: string) {
  sendTerminalCommand(invokeFormat)
}
</script>

<style scoped>
.plugin-item {
  background: var(--bg-primary);
  border-radius: 8px;
  padding: 10px 12px;
  transition: background 0.15s ease;
}

.plugin-item:hover {
  background: var(--hover-bg);
}

.plugin-header {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
}

.expand-icon {
  width: 14px;
  height: 14px;
  color: var(--text-secondary);
  flex-shrink: 0;
  transition: transform 0.15s ease;
}

.expand-icon.expanded {
  transform: rotate(90deg);
}

.plugin-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.plugin-version {
  font-size: 11px;
  color: var(--text-tertiary);
}

.plugin-id {
  margin-top: 4px;
  font-size: 11px;
  color: var(--text-tertiary);
  font-family: var(--font-mono);
}

.plugin-components {
  margin-top: 8px;
  display: flex;
  gap: 6px;
}

.component-tag {
  font-size: 10px;
  padding: 2px 6px;
  border-radius: 4px;
}

.component-tag.mcp {
  background: #e3f2fd;
  color: #1565c0;
}

.component-tag.skills {
  background: #fff3e0;
  color: #e65100;
}

.component-tag.agents {
  background: #f3e5f5;
  color: #7b1fa2;
}

/* Expanded styles */
.plugin-expanded {
  margin-top: 12px;
  padding-top: 8px;
  border-top: 1px solid var(--border-color);
}

.component-section {
  margin-bottom: 12px;
}

.section-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 6px;
}

.component-item {
  background: var(--bg-secondary);
  border-radius: 6px;
  margin-bottom: 4px;
}

.item-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px;
  cursor: pointer;
  user-select: none;
}

.item-expand-icon {
  width: 10px;
  height: 10px;
  color: var(--text-tertiary);
  flex-shrink: 0;
  transition: transform 0.15s ease;
}

.item-expand-icon.expanded {
  transform: rotate(90deg);
}

.item-name {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  flex: 1;
}

.item-use-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  border-radius: 4px;
  flex-shrink: 0;
}

.item-use-btn:hover {
  color: var(--accent-color);
  background: var(--bg-tertiary);
}

.item-icon {
  width: 14px;
  height: 14px;
}

.item-detail {
  padding: 8px;
  padding-top: 0;
  margin-top: 6px;
  border-top: 1px solid var(--border-color);
}

.item-desc-full {
  font-size: 11px;
  color: var(--text-secondary);
  line-height: 1.5;
  white-space: pre-wrap;
}

.item-desc-empty {
  font-size: 11px;
  color: var(--text-tertiary);
  font-style: italic;
}

.item-invoke-format {
  margin-top: 6px;
  display: flex;
  gap: 4px;
  font-size: 10px;
}

.invoke-label {
  color: var(--text-tertiary);
}

.invoke-value {
  font-family: var(--font-mono);
  color: var(--text-primary);
  background: var(--bg-tertiary);
  padding: 1px 4px;
  border-radius: 3px;
}

/* MCP Server styles */
.component-item.mcp-server {
  padding: 8px;
  cursor: default;
}

.component-item.mcp-server:hover {
  background: var(--bg-secondary);
}

.item-type {
  font-size: 10px;
  color: var(--text-tertiary);
  margin-left: 8px;
}

.no-components {
  font-size: 12px;
  color: var(--text-tertiary);
  text-align: center;
  padding: 12px;
}
</style>