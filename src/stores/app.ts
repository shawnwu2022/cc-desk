import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import {
  getAppConfig,
  updateAppConfig,
  saveLastProject,
  getDefaultClaudeOptions,
  saveDefaultClaudeOptions
} from '@/api/tauri'

// 从统一类型导入
import type { ClaudeOptions } from '@/types'

export const useAppStore = defineStore('app', () => {
  const cwd = ref<string>('')
  const theme = ref<string>('light')
  const fontSize = ref<number>(10)

  // Claude 启动选项（初始值，会被应用配置覆盖）
  const claudeOptions = ref<ClaudeOptions>({
    continue: true,
    resume: '',
    skipPermissions: false,
    customArgs: ''
  })

  const currentProject = computed(() => {
    if (!cwd.value) return null
    const parts = cwd.value.replace(/\\/g, '/').split('/')
    return parts[parts.length - 1] || cwd.value
  })

  // 从应用配置加载默认值
  async function loadAppConfig() {
    try {
      const config = await getAppConfig()
      theme.value = config.theme || 'light'
      fontSize.value = config.fontSize || 10

      // 设置启动选项默认值
      claudeOptions.value = {
        continue: config.defaultContinue ?? true,
        resume: '',
        skipPermissions: config.defaultSkipPermissions ?? false,
        customArgs: config.defaultCustomArgs ?? ''
      }

      // 如果有上次打开的项目，可以自动恢复
      if (config.lastOpenedProject) {
        cwd.value = config.lastOpenedProject
      }
    } catch (err) {
      console.error('Failed to load app config:', err)
    }
  }

  function setCwd(path: string) {
    cwd.value = path
    // 保存到应用配置
    saveLastProject(path)
  }

  function setTheme(newTheme: string) {
    theme.value = newTheme
    updateAppConfig({ theme: newTheme })
  }

  function setFontSize(size: number) {
    fontSize.value = Math.max(10, Math.min(24, size))
    updateAppConfig({ fontSize: size })
  }

  function setClaudeOptions(options: Partial<ClaudeOptions>) {
    claudeOptions.value = { ...claudeOptions.value, ...options }
  }

  function resetClaudeOptions() {
    // 重置为应用默认值
    getDefaultClaudeOptions().then(defaults => {
      claudeOptions.value = {
        continue: defaults.continue ?? true,
        resume: '',
        skipPermissions: defaults.skipPermissions ?? false,
        customArgs: defaults.customArgs ?? ''
      }
    })
  }

  // 保存当前启动选项为默认值
  async function saveAsDefault(): Promise<boolean> {
    try {
      await saveDefaultClaudeOptions({
        continue: claudeOptions.value.continue,
        skipPermissions: claudeOptions.value.skipPermissions,
        customArgs: claudeOptions.value.customArgs
      })
      return true
    } catch (err) {
      console.error('Failed to save default options:', err)
      return false
    }
  }

  // 将选项转换为 CLI 参数
  // 所有选项如实地传递给 CLI，不做任何转换
  function getClaudeArgs(): string[] {
    const opts = claudeOptions.value
    const args: string[] = []

    // 如实传递所有选项
    if (opts.continue) args.push('--continue')
    if (opts.resume) args.push('--resume', opts.resume)
    if (opts.skipPermissions) args.push('--dangerously-skip-permissions')
    if (opts.customArgs) {
      const custom = opts.customArgs.trim().split(/\s+/).filter(Boolean)
      args.push(...custom)
    }

    return args
  }

  // 初始化时加载配置
  loadAppConfig()

  return {
    cwd,
    theme,
    fontSize,
    claudeOptions,
    currentProject,
    loadAppConfig,
    setCwd,
    setTheme,
    setFontSize,
    setClaudeOptions,
    resetClaudeOptions,
    saveAsDefault,
    getClaudeArgs
  }
})