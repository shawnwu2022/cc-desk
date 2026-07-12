import type { ProjectStartupState } from '@/types/app'

/** 启动决策结果 */
export interface StartupDecision {
  view: 'welcome' | 'projects' | 'terminal'
  /** 是否自动打开 Sessions 面板（首次/lastOpened 无效/隐藏时引导） */
  openSessionsPanel: boolean
  /** 需 setCurrentProject(persist:false) 恢复的项目路径；null=不恢复 */
  restoreProject: string | null
}

/**
 * 启动视图决策纯函数（spec §4.3 五场景）。
 * - hasAnyProject==false -> welcome
 * - hasVisibleProject==false（全隐藏）-> projects（管理页，可「显示」恢复）
 * - 有可见 + lastOpened 有效且未隐藏 -> terminal + restore
 * - 有可见 + 无 lastOpened（首次）/ lastOpened 隐藏 / 无效 / 无 info -> terminal + 自动开 Sessions（不 setCwd）
 */
export function decideStartupView(
  state: ProjectStartupState,
  lastOpened: string,
  isHidden: (path: string) => boolean,
): StartupDecision {
  if (!state.hasAnyProject) {
    return { view: 'welcome', openSessionsPanel: false, restoreProject: null }
  }
  if (!state.hasVisibleProject) {
    return { view: 'projects', openSessionsPanel: false, restoreProject: null }
  }
  // 有可见项目
  const info = state.lastOpenedProjectInfo
  const lastOpenedHidden = lastOpened ? isHidden(lastOpened) : false
  if (info && info.exists && !lastOpenedHidden) {
    return { view: 'terminal', openSessionsPanel: false, restoreProject: lastOpened }
  }
  // 首次 / lastOpened 隐藏 / 无效 / 无 info -> terminal + 自动开 Sessions
  return { view: 'terminal', openSessionsPanel: true, restoreProject: null }
}
