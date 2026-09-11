<template>
  <div class="plugin-item" :class="{ expanded: isExpanded, disabled: isDisabled }">
    <div
      class="plugin-header"
      role="button"
      tabindex="0"
      @click="toggleExpand"
      @keydown.enter.self.prevent="toggleExpand"
      @keydown.space.self.prevent="toggleExpand"
    >
      <img
        class="expand-icon"
        :class="{ expanded: isExpanded }"
        src="@/assets/icons/chevron.svg"
        alt="Toggle"
      />
      <span class="plugin-name">{{ plugin.name }}</span>
      <span class="plugin-state">{{ isDisabled ? t('disabled') : t('enabled') }}</span>
    </div>

    <div class="plugin-version">v{{ plugin.version }}</div>
    <div class="plugin-id">{{ plugin.id }}</div>

    <div v-if="!isExpanded && hasComponents" class="plugin-components">
      <span v-if="plugin.skills?.length" class="component-tag skills">
        {{ plugin.skills.length }} Skills
      </span>
      <span v-if="plugin.agents?.length" class="component-tag agents">
        {{ plugin.agents.length }} Agents
      </span>
      <span v-if="mcpCount > 0" class="component-tag mcp">
        {{ mcpCount }} MCP
      </span>
    </div>

    <div v-if="isExpanded" class="plugin-expanded">
      <div v-if="plugin.skills?.length" class="component-section">
        <div class="section-title">{{ t('skills') }}</div>
        <div v-for="skill in plugin.skills" :key="skill.name" class="component-item">
          <div
            class="item-header"
            role="button"
            tabindex="0"
            @click="toggleSkillDetail(skill.name)"
            @keydown.enter.self.prevent="toggleSkillDetail(skill.name)"
            @keydown.space.self.prevent="toggleSkillDetail(skill.name)"
          >
            <img
              class="item-expand-icon"
              :class="{ expanded: expandedSkills[skill.name] }"
              src="@/assets/icons/chevron.svg"
              alt="Toggle"
            />
            <span class="item-name">{{ skill.name }}</span>
            <button
              class="item-use-btn"
              :disabled="isDisabled"
              :title="t('useThisSkill')"
              @click.stop="useSkill(skill.invokeFormat)"
            >
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

      <div v-if="plugin.agents?.length" class="component-section">
        <div class="section-title">{{ t('agents') }}</div>
        <div v-for="agent in plugin.agents" :key="agent.name" class="component-item">
          <div
            class="item-header"
            role="button"
            tabindex="0"
            @click="toggleAgentDetail(agent.name)"
            @keydown.enter.self.prevent="toggleAgentDetail(agent.name)"
            @keydown.space.self.prevent="toggleAgentDetail(agent.name)"
          >
            <img
              class="item-expand-icon"
              :class="{ expanded: expandedAgents[agent.name] }"
              src="@/assets/icons/chevron.svg"
              alt="Toggle"
            />
            <span class="item-name">{{ agent.name }}</span>
            <button
              class="item-use-btn"
              :disabled="isDisabled"
              :title="t('useThisAgent')"
              @click.stop="useAgent(agent.invokeFormat)"
            >
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

      <div v-if="mcpCount > 0" class="component-section">
        <div class="section-title">{{ t('mcpServers') }}</div>
        <div
          v-for="(serverConfig, serverName) in plugin.mcpServers"
          :key="serverName"
          class="component-item mcp-server"
        >
          <span class="item-name">{{ serverName }}</span>
          <span v-if="serverConfig.type" class="item-type">{{ serverConfig.type }}</span>
        </div>
      </div>

      <div v-if="!hasComponents" class="no-components">
        {{ t('noDescription') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
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

const isDisabled = computed(() => props.plugin.enabled === false)
const mcpCount = computed(() => Object.keys(props.plugin.mcpServers ?? {}).length)
const hasComponents = computed(() =>
  Boolean(props.plugin.skills?.length || props.plugin.agents?.length || mcpCount.value)
)

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
  if (!isDisabled.value) sendTerminalCommand(invokeFormat)
}

function useAgent(invokeFormat: string) {
  if (!isDisabled.value) sendTerminalCommand(invokeFormat)
}
</script>

<style scoped>
.plugin-item {
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--bg-primary);
  transition: background 0.15s ease;
}

.plugin-item:hover {
  background: var(--hover-bg);
}

.plugin-item.disabled {
  opacity: 0.55;
}

.plugin-item.disabled .plugin-name {
  color: var(--text-tertiary);
  text-decoration: line-through;
}

.plugin-header,
.item-header,
.plugin-components,
.item-invoke-format {
  display: flex;
  align-items: center;
  gap: 8px;
}

.plugin-header,
.item-header {
  cursor: pointer;
  user-select: none;
}

.expand-icon,
.item-expand-icon {
  flex-shrink: 0;
  transition: transform 0.15s ease;
}

.expand-icon {
  width: 14px;
  height: 14px;
}

.item-expand-icon {
  width: 10px;
  height: 10px;
}

.expand-icon.expanded,
.item-expand-icon.expanded {
  transform: rotate(90deg);
}

.plugin-name,
.item-name {
  min-width: 0;
  flex: 1;
  color: var(--text-primary);
  font-weight: 500;
}

.plugin-name {
  font-size: 13px;
}

.item-name {
  font-size: 12px;
}

.plugin-state,
.plugin-version,
.plugin-id,
.item-type {
  color: var(--text-tertiary);
  font-size: 11px;
}

.plugin-state {
  flex-shrink: 0;
}

.plugin-version,
.plugin-id {
  margin-top: 4px;
}

.plugin-id,
.invoke-value {
  font-family: var(--font-mono);
}

.plugin-components {
  margin-top: 8px;
  gap: 6px;
}

.component-tag {
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 10px;
}

.component-tag.skills {
  background: var(--tag-skill-bg);
  color: var(--tag-skill-text);
}

.component-tag.agents {
  background: var(--tag-agent-bg);
  color: var(--tag-agent-text);
}

.component-tag.mcp {
  background: var(--tag-mcp-bg);
  color: var(--tag-mcp-text);
}

.plugin-expanded {
  margin-top: 12px;
  padding-top: 8px;
  border-top: 1px solid var(--border-color);
}

.component-section {
  margin-bottom: 12px;
}

.section-title {
  margin-bottom: 6px;
  color: var(--text-secondary);
  font-size: 11px;
  font-weight: 600;
}

.component-item {
  margin-bottom: 4px;
  border-radius: 6px;
  background: var(--bg-secondary);
}

.item-header,
.component-item.mcp-server {
  padding: 8px;
}

.item-use-btn {
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
}

.item-use-btn:disabled {
  cursor: default;
  opacity: 0.4;
}

.item-use-btn:not(:disabled):hover {
  background: var(--bg-tertiary);
}

.item-icon {
  width: 14px;
  height: 14px;
}

.item-detail {
  margin-top: 6px;
  padding: 0 8px 8px;
  border-top: 1px solid var(--border-color);
}

.item-desc-full,
.item-desc-empty {
  padding-top: 8px;
  color: var(--text-secondary);
  font-size: 11px;
  line-height: 1.5;
  white-space: pre-wrap;
}

.item-desc-empty {
  color: var(--text-tertiary);
  font-style: italic;
}

.item-invoke-format {
  margin-top: 6px;
  gap: 4px;
  font-size: 10px;
}

.invoke-label {
  color: var(--text-tertiary);
}

.invoke-value {
  padding: 1px 4px;
  border-radius: 3px;
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.component-item.mcp-server {
  display: flex;
  align-items: center;
}

.no-components {
  padding: 12px;
  color: var(--text-tertiary);
  font-size: 12px;
  text-align: center;
}
</style>
