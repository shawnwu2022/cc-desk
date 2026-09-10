import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

// 从统一类型目录导入
import type {
  PtySpawnResult,
  PtyOutputPayload,
  PtyExitPayload,
  Project,
  SessionInfo,
  SessionDetails,
  SessionSearchResult,
  AppConfig,
  ProjectsState,
  DefaultClaudeOptions,
  ProjectConfigResult,
  AgentInfo,
  McpServerInfo,
  PluginInfo,
  SkillInfo,
  UpdateInfo,
  DownloadProgress,
  HomeData,
  CheckResult,
  HookEventPayload,
  ProjectInfo,
  ProjectStartupState,
} from '@/types';

// 重新导出类型（保持兼容性）
export type {
  PtySpawnResult,
  PtyOutputPayload,
  PtyExitPayload,
  Project,
  SessionInfo,
  SessionDetails,
  SessionSearchResult,
  AppConfig,
  ProjectsState,
  DefaultClaudeOptions,
  ProjectConfigResult,
  AgentInfo,
  McpServerInfo,
  PluginInfo,
  SkillInfo,
  UpdateInfo,
  DownloadProgress,
  ProjectInfo,
  ProjectStartupState,
};

// ============================================
// PTY Operations
// ============================================

interface PtySpawnOptions {
  id: string
  cwd: string
  cols: number
  rows: number
  type: 'claude' | 'shell'
  args?: string[]
}

export const ptySpawn = async (options: PtySpawnOptions): Promise<PtySpawnResult | null> => {
  return invoke<PtySpawnResult | null>('pty_spawn', { options });
};

export const ptyInput = async (id: string, data: string): Promise<boolean> => {
  return invoke<boolean>('pty_input', { id, data });
};

export const ptyResize = async (id: string, cols: number, rows: number): Promise<boolean> => {
  return invoke<boolean>('pty_resize', { id, cols, rows });
};

export const ptyKill = async (id: string): Promise<boolean> => {
  return invoke<boolean>('pty_kill', { id });
};

export const ptyKillAll = async (): Promise<void> => {
  return invoke<void>('pty_kill_all');
};

// ============================================
// Event Listeners (Tauri events)
// ============================================

export const onPtyOutput = (callback: (payload: PtyOutputPayload) => void): Promise<UnlistenFn> =>
  listen<PtyOutputPayload>('pty-output', (event) => callback(event.payload));

export const onPtyExit = (callback: (payload: PtyExitPayload) => void): Promise<UnlistenFn> =>
  listen<PtyExitPayload>('pty-exit', (event) => callback(event.payload));

// Hook 监控事件
export const onHookEvent = (callback: (payload: HookEventPayload) => void): Promise<UnlistenFn> =>
  listen<HookEventPayload>('hook-event', (event) => callback(event.payload));

// Menu events
export const onMenuSettings = (callback: () => void): Promise<UnlistenFn> =>
  listen('menu:settings', () => callback());

export const onMenuShortcuts = (callback: () => void): Promise<UnlistenFn> =>
  listen('menu:shortcuts', () => callback());

export const onConfigFontSize = (callback: (size: number) => void): Promise<UnlistenFn> =>
  listen<number>('config:fontSize', (event) => callback(event.payload));

export const onTerminalRestart = (callback: (data: { cwd: string }) => void): Promise<UnlistenFn> =>
  listen<{ cwd: string }>('terminal:restart', (event) => callback(event.payload));

// ============================================
// Projects and Sessions
// ============================================

export const getCheckResults = (): Promise<CheckResult[]> =>
  invoke<CheckResult[]>('get_check_results');

export const runChecks = (): Promise<CheckResult[]> =>
  invoke<CheckResult[]>('run_checks');

export const getHomeData = (
  projectLimit: number,
  sessionLimit: number,
  lastOpened: string,
  hidden: string[]
): Promise<HomeData> =>
  invoke<HomeData>('get_home_data', { projectLimit, sessionLimit, lastOpened, hidden });

export const getProjects = (limit?: number, offset?: number): Promise<Project[]> =>
  invoke<Project[]>('get_projects', { limit, offset });

export const getProjectInfo = (path: string): Promise<Project | null> =>
  invoke<Project | null>('get_project_info', { path });

export const getSessions = (projectPath: string, limit?: number, offset?: number): Promise<SessionInfo[]> =>
  invoke<SessionInfo[]>('get_sessions', { projectPath, limit, offset });

export const getSessionCount = (projectPath: string): Promise<number> =>
  invoke<number>('get_session_count', { projectPath });

export const getAllRecentSessions = (limit?: number): Promise<SessionInfo[]> =>
  invoke<SessionInfo[]>('get_all_recent_sessions', { limit });

export const getSessionDetails = (projectPath: string, sessionId: string): Promise<SessionDetails | null> =>
  invoke<SessionDetails | null>('get_session_details', { projectPath, sessionId });

export const searchSessionMessages = (
  projectPath: string,
  query: string,
  limit?: number
): Promise<SessionSearchResult[]> =>
  invoke<SessionSearchResult[]>('search_session_messages', { projectPath, query, limit });

// ============================================
// Configuration
// ============================================

export const getAppConfig = (): Promise<AppConfig> =>
  invoke<AppConfig>('get_app_config');

export const updateAppConfig = (updates: Record<string, unknown>): Promise<void> =>
  invoke<void>('update_app_config', { updates });

// 项目置顶 + 会话存档 + 别名状态（~/.cc-box/projects.json）——增量操作 command（返回最新 ProjectsState）
// session.ts import 时 alias 为 xxxApi 避免与 store action 同名。
export const getProjectsState = (): Promise<ProjectsState> =>
  invoke<ProjectsState>('get_projects_state');

export const pinProject = (path: string): Promise<ProjectsState> =>
  invoke<ProjectsState>('pin_project', { path });

export const unpinProject = (path: string): Promise<ProjectsState> =>
  invoke<ProjectsState>('unpin_project', { path });

export const archiveSession = (projectPath: string, sessionId: string): Promise<ProjectsState> =>
  invoke<ProjectsState>('archive_session', { projectPath, sessionId });

export const restoreSession = (projectPath: string, sessionId: string): Promise<ProjectsState> =>
  invoke<ProjectsState>('restore_session', { projectPath, sessionId });

export const deleteSessions = (projectPath: string, sessionIds: string[]): Promise<ProjectsState> =>
  invoke<ProjectsState>('delete_sessions', { projectPath, sessionIds });

export const setDisplayName = (path: string, alias: string): Promise<ProjectsState> =>
  invoke<ProjectsState>('set_display_name', { path, alias });

export const getDefaultClaudeOptions = (): Promise<DefaultClaudeOptions> =>
  invoke<DefaultClaudeOptions>('get_default_claude_options');

export const saveDefaultClaudeOptions = (options: Partial<DefaultClaudeOptions>): Promise<void> =>
  invoke<void>('save_default_claude_options', { options });

export const saveLastProject = (path: string): Promise<void> =>
  invoke<void>('save_last_project', { path });

export const getProjectConfig = (projectPath: string): Promise<ProjectConfigResult> =>
  invoke<ProjectConfigResult>('get_project_config', { projectPath });

export const getAllAgents = (projectPath: string): Promise<AgentInfo[]> =>
  invoke<AgentInfo[]>('get_all_agents', { projectPath });

export const getAllSkills = (projectPath: string): Promise<SkillInfo[]> =>
  invoke<SkillInfo[]>('get_all_skills', { projectPath });

export const getAllMcpServers = (projectPath: string): Promise<McpServerInfo[]> =>
  invoke<McpServerInfo[]>('get_all_mcp_servers', { projectPath });

export const getAllPlugins = (projectPath: string): Promise<PluginInfo[]> =>
  invoke<PluginInfo[]>('get_all_plugins', { projectPath });


// ============================================
// File Management
// ============================================

export const openInFileManager = (path: string): Promise<void> =>
  invoke<void>('open_in_file_manager', { path });

// ============================================
// Updater (Tauri official plugin)
// ============================================

export type { Update } from '@tauri-apps/plugin-updater';
export { check, relaunch };

export const checkForUpdates = async (): Promise<UpdateInfo> => {
  const update = await check();
  if (!update) {
    return {
      version: __APP_VERSION__,
      currentVersion: __APP_VERSION__,
      hasUpdate: false,
      releaseNotes: '',
      downloadUrl: '',
      platformAsset: null,
    };
  }
  return {
    version: update.version,
    currentVersion: __APP_VERSION__,
    hasUpdate: true,
    releaseNotes: update.body || '',
    downloadUrl: '',
    platformAsset: null,
  };
};

// ============================================
// App Instance
// ============================================

export const getAppPath = (): Promise<string> =>
  invoke<string>('get_app_path');

export const spawnNewInstance = (): Promise<void> =>
  invoke<void>('spawn_new_instance');

// ============================================
// Logging
// ============================================

export const logMessage = (level: 'error' | 'warn' | 'info' | 'debug', message: string): Promise<void> =>
  invoke<void>('log_message', { level, message });

// ============================================
// Dialog (Tauri dialog plugin)
// ============================================

export const selectDirectory = async (): Promise<{ path: string } | null> => {
  const result = await open({
    directory: true,
    multiple: false,
    title: 'Select Project Directory'
  } as any);
  if (result && typeof result === 'string') {
    return { path: result };
  }
  return null;
};


// 右键菜单打开目录
export const onOpenDirectory = (callback: (dir: string) => void): Promise<UnlistenFn> =>
  listen<string>('open-directory', (event) => callback(event.payload));