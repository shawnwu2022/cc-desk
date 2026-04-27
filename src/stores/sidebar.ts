import { defineStore } from 'pinia'
import { ref } from 'vue'

export type SidebarPanelType = 'sessions' | 'skills' | 'agents' | 'mcp' | 'plugins' | null

export const useSidebarStore = defineStore('sidebar', () => {
  const activePanel = ref<SidebarPanelType>(null)
  const panelVisible = ref(false)

  // 设置模式
  const showSettings = ref(false)
  const activeSettingsSection = ref<string>('appearance')
  const updateAvailable = ref(false)

  // Skills 面板折叠状态（按来源分组）
  const skillsExpandedGroups = ref({
    project: true,
    user: true,
    plugin: true
  })

  // Agents 面板折叠状态
  const agentsExpandedGroups = ref({
    builtin: true,
    plugin: true,
    user: true,
    project: true
  })

  // MCP 面板折叠状态
  const mcpExpandedGroups = ref({
    plugin: true,
    user: true,
    project: true
  })

  // Plugins 面板折叠状态
  const pluginsExpandedGroups = ref({
    user: true,
    project: true
  })

  // 切换面板
  function togglePanel(panel: SidebarPanelType) {
    // 如果设置打开，先关闭设置再打开面板
    if (showSettings.value) {
      showSettings.value = false
      activePanel.value = panel
      panelVisible.value = true
      return
    }

    if (activePanel.value === panel && panelVisible.value) {
      closePanel()
    } else {
      activePanel.value = panel
      panelVisible.value = true
    }
  }

  // 设置模式
  function openSettings(section?: string) {
    panelVisible.value = false
    activePanel.value = null
    showSettings.value = true
    if (section) activeSettingsSection.value = section
  }

  function closeSettings() {
    showSettings.value = false
  }

  function toggleSettings() {
    if (showSettings.value) {
      closeSettings()
    } else {
      openSettings()
    }
  }

  // 关闭面板
  function closePanel() {
    panelVisible.value = false
    setTimeout(() => {
      activePanel.value = null
    }, 250)
  }

  // 切换 Skill 分组折叠
  function toggleSkillGroup(group: keyof typeof skillsExpandedGroups.value) {
    skillsExpandedGroups.value[group] = !skillsExpandedGroups.value[group]
  }

  // 切换 Agent 分组折叠
  function toggleAgentGroup(group: keyof typeof agentsExpandedGroups.value) {
    agentsExpandedGroups.value[group] = !agentsExpandedGroups.value[group]
  }

  // 切换 MCP 分组折叠
  function toggleMcpGroup(group: keyof typeof mcpExpandedGroups.value) {
    mcpExpandedGroups.value[group] = !mcpExpandedGroups.value[group]
  }

  // 切换 Plugin 分组折叠
  function togglePluginGroup(group: keyof typeof pluginsExpandedGroups.value) {
    pluginsExpandedGroups.value[group] = !pluginsExpandedGroups.value[group]
  }

  return {
    activePanel,
    panelVisible,
    showSettings,
    activeSettingsSection,
    updateAvailable,
    skillsExpandedGroups,
    agentsExpandedGroups,
    mcpExpandedGroups,
    pluginsExpandedGroups,
    togglePanel,
    closePanel,
    openSettings,
    closeSettings,
    toggleSettings,
    toggleSkillGroup,
    toggleAgentGroup,
    toggleMcpGroup,
    togglePluginGroup
  }
})