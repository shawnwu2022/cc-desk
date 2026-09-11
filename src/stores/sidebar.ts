import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { getAllAgents, getAllMcpServers, getAllPlugins, getAllSkills } from '@/api/tauri'
import type { AgentInfo, McpServerInfo, PluginInfo, SkillInfo, UpdateInfo } from '@/types'

export type SidebarPanelType = 'sessions' | 'skills' | 'agents' | 'mcp' | 'plugins' | null
export type SettingsSection = 'appearance' | 'startup' | 'shortcuts' | 'update' | 'about'

const SETTINGS_SECTIONS: readonly SettingsSection[] = [
  'appearance',
  'startup',
  'shortcuts',
  'update',
  'about',
]

function isSettingsSection(value: string): value is SettingsSection {
  return SETTINGS_SECTIONS.includes(value as SettingsSection)
}

export const useSidebarStore = defineStore('sidebar', () => {
  const activePanel = ref<SidebarPanelType>(null)
  const panelVisible = ref(false)

  const showSettings = ref(false)
  const activeSettingsSection = ref<SettingsSection>('appearance')
  const updateInfo = ref<UpdateInfo | null>(null)
  const updateAvailable = computed(() => updateInfo.value?.hasUpdate ?? false)

  function setUpdateInfo(info: UpdateInfo) {
    updateInfo.value = info
  }

  const skillsExpandedGroups = ref({
    project: true,
    user: true,
    plugin: true,
  })

  const agentsExpandedGroups = ref({
    builtin: true,
    plugin: true,
    user: true,
    project: true,
  })

  const mcpExpandedGroups = ref({
    plugin: true,
    user: true,
    project: true,
  })

  const pluginsExpandedGroups = ref({
    user: true,
    project: true,
  })

  const skills = ref<SkillInfo[]>([])
  const agents = ref<AgentInfo[]>([])
  const mcpServers = ref<McpServerInfo[]>([])
  const plugins = ref<PluginInfo[]>([])

  const skillsLoading = ref(false)
  const agentsLoading = ref(false)
  const mcpServersLoading = ref(false)
  const pluginsLoading = ref(false)
  const loadedCwd = ref<string | null>(null)

  async function loadAllSidebarData(cwd: string) {
    if (loadedCwd.value === cwd) return

    loadedCwd.value = cwd
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
    } catch (error) {
      console.error('[SidebarStore] Failed to load skills:', error)
    } finally {
      skillsLoading.value = false
    }
  }

  async function loadAgents(cwd: string) {
    agentsLoading.value = true
    try {
      agents.value = await getAllAgents(cwd)
    } catch (error) {
      console.error('[SidebarStore] Failed to load agents:', error)
    } finally {
      agentsLoading.value = false
    }
  }

  async function loadMcpServers(cwd: string) {
    mcpServersLoading.value = true
    try {
      mcpServers.value = await getAllMcpServers(cwd)
    } catch (error) {
      console.error('[SidebarStore] Failed to load MCP servers:', error)
    } finally {
      mcpServersLoading.value = false
    }
  }

  async function loadPlugins(cwd: string) {
    pluginsLoading.value = true
    try {
      plugins.value = await getAllPlugins(cwd)
    } catch (error) {
      console.error('[SidebarStore] Failed to load plugins:', error)
    } finally {
      pluginsLoading.value = false
    }
  }

  function togglePanel(panel: SidebarPanelType) {
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

  function openSettings(section?: string) {
    panelVisible.value = false
    activePanel.value = null
    showSettings.value = true
    if (section) {
      activeSettingsSection.value = isSettingsSection(section) ? section : 'appearance'
    }
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

  function closePanel() {
    panelVisible.value = false
    setTimeout(() => {
      activePanel.value = null
    }, 250)
  }

  function toggleSkillGroup(group: keyof typeof skillsExpandedGroups.value) {
    skillsExpandedGroups.value[group] = !skillsExpandedGroups.value[group]
  }

  function toggleAgentGroup(group: keyof typeof agentsExpandedGroups.value) {
    agentsExpandedGroups.value[group] = !agentsExpandedGroups.value[group]
  }

  function toggleMcpGroup(group: keyof typeof mcpExpandedGroups.value) {
    mcpExpandedGroups.value[group] = !mcpExpandedGroups.value[group]
  }

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
    togglePanel,
    closePanel,
    openSettings,
    closeSettings,
    toggleSettings,
    toggleSkillGroup,
    toggleAgentGroup,
    toggleMcpGroup,
    togglePluginGroup,
  }
})
