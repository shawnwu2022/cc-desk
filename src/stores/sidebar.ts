import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { getAllAgents, getAllSkills, getAllMcpServers, getAllPlugins } from '@/api/tauri'
import type { AgentInfo, SkillInfo, McpServerInfo, PluginInfo, UpdateInfo } from '@/types'

export type SidebarPanelType = 'sessions' | 'skills' | 'agents' | 'mcp' | 'plugins' | null

export const useSidebarStore = defineStore('sidebar', () => {
  const activePanel = ref<SidebarPanelType>(null)
  const panelVisible = ref(false)

  // 设置模式
  const showSettings = ref(false)
  const activeSettingsSection = ref<string>('appearance')
  const updateInfo = ref<UpdateInfo | null>(null)
  const updateAvailable = computed(() => updateInfo.value?.hasUpdate ?? false)

  function setUpdateInfo(info: UpdateInfo) {
    updateInfo.value = info
  }

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

  // ========== 预加载数据 ==========

  // 数据
  const skills = ref<SkillInfo[]>([])
  const agents = ref<AgentInfo[]>([])
  const mcpServers = ref<McpServerInfo[]>([])
  const plugins = ref<PluginInfo[]>([])

  // 加载状态
  const skillsLoading = ref(false)
  const agentsLoading = ref(false)
  const mcpServersLoading = ref(false)
  const pluginsLoading = ref(false)

  // 已加载的 cwd（用于判断是否需要重新加载）
  const loadedCwd = ref<string | null>(null)

  // 加载所有 sidebar 数据
  async function loadAllSidebarData(cwd: string) {
    if (loadedCwd.value === cwd) return // 已加载过

    loadedCwd.value = cwd

    // 并行加载所有数据
    await Promise.all([
      loadSkills(cwd),
      loadAgents(cwd),
      loadMcpServers(cwd),
      loadPlugins(cwd),
    ])
  }

  async function loadSkills(cwd: string) {
    skillsLoading.value = true
    try {
      skills.value = await getAllSkills(cwd)
    } catch (err) {
      console.error('[SidebarStore] Failed to load skills:', err)
    } finally {
      skillsLoading.value = false
    }
  }

  async function loadAgents(cwd: string) {
    agentsLoading.value = true
    try {
      agents.value = await getAllAgents(cwd)
    } catch (err) {
      console.error('[SidebarStore] Failed to load agents:', err)
    } finally {
      agentsLoading.value = false
    }
  }

  async function loadMcpServers(cwd: string) {
    mcpServersLoading.value = true
    try {
      mcpServers.value = await getAllMcpServers(cwd)
    } catch (err) {
      console.error('[SidebarStore] Failed to load mcp servers:', err)
    } finally {
      mcpServersLoading.value = false
    }
  }

  async function loadPlugins(cwd: string) {
    pluginsLoading.value = true
    try {
      plugins.value = await getAllPlugins(cwd)
    } catch (err) {
      console.error('[SidebarStore] Failed to load plugins:', err)
    } finally {
      pluginsLoading.value = false
    }
  }

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
    updateInfo,
    updateAvailable,
    setUpdateInfo,
    skillsExpandedGroups,
    agentsExpandedGroups,
    mcpExpandedGroups,
    pluginsExpandedGroups,
    // 预加载数据
    skills,
    agents,
    mcpServers,
    plugins,
    skillsLoading,
    agentsLoading,
    mcpServersLoading,
    pluginsLoading,
    loadedCwd,
    loadAllSidebarData,
    loadSkills,
    loadAgents,
    loadMcpServers,
    loadPlugins,
    // 操作函数
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