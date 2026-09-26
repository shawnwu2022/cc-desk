import { createProjectionClient } from './nativeProjection'
import { createLaunchAttempt } from './cliLaunchAttempt';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { pasteTrace } from '@/utils/pasteTrace';
import { setPasteObserver } from '@/utils/pasteText';
import { persistProjectRegistration, registerSelectedDirectory } from './projectRegistration';

// Diagnostic executable only. No user settings, hooks or clipboard are persisted.
if (import.meta.env.VITE_CC_DESK_PASTE_TRACE === '1') {
  pasteTrace.setEnabled(true);
  setPasteObserver(id => {
    const ticket = pasteTrace.begin(id);
    return ticket
      ? (target, expected, send) => pasteTrace.observe(ticket, target, expected, send)
      : undefined;
  });
}

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

export type PtyInputSource =
  | 'terminal-ondata'
  | 'xterm-ondata-paste'
  | 'clipboard-keyboard'
  | 'clipboard-dom'
  | 'ime-fallback'
  | 'other'

export const ptyInput = async (
  id: string,
  data: string,
  source: PtyInputSource = 'other',
): Promise<boolean> => {
  const trace = pasteTrace.input(id, data);
  return invoke<boolean>('pty_input', trace ? { id, data, source, trace } : { id, data, source });
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

export const saveLastProject = async (path: string): Promise<void> => {
  await persistProjectRegistration(path);
  await invoke<void>('save_last_project', { path });
};

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
  if (result && typeof result === 'string' && await registerSelectedDirectory(result)) {
    return { path: result };
  }
  return null;
};


// 右键菜单打开目录
export const onOpenDirectory = (callback: (dir: string) => void): Promise<UnlistenFn> =>
  listen<string>('open-directory', (event) => callback(event.payload));

// The native document bridge owns the proof and raw transport. Never fall back
// to an unguarded invoke when this document has no authenticated bridge.
interface NativeDocumentBridge {
  readonly instanceId: string;
  invoke(command: string, payload: unknown, channel?: unknown): Promise<unknown>;
}

function nativeDocumentBridge(): NativeDocumentBridge {
  const bridge = (window as Window & { __CC_DESK_DOCUMENT__?: NativeDocumentBridge }).__CC_DESK_DOCUMENT__;
  if (!bridge || typeof bridge.invoke !== 'function') {
    throw { code: 'DOCUMENT_BRIDGE_UNAVAILABLE' };
  }
  return bridge;
}

export async function cliStart<E>(
  request: import('@/types/cli').LaunchRequest,
  channel: import('@tauri-apps/api/core').Channel<E>,
): Promise<unknown> {
  return nativeDocumentBridge().invoke('cli_start', request, channel);
}

export async function cliGetLaunchStatus(requestId: string): Promise<unknown> {
  return nativeDocumentBridge().invoke('cli_get_launch_status', { requestId });
}

export async function cliAckOutput(
  ack: import('@/types/terminal').OutputAck,
): Promise<void> {
  await nativeDocumentBridge().invoke('cli_ack_output', ack)
}

export async function cliStop(
  run: import('@/types/terminal').RunKey,
): Promise<void> {
  await nativeDocumentBridge().invoke('cli_stop', run)
}


export const NATIVE_INPUT_UPLOAD_CHUNK_BYTES = 64 * 1024

export async function cliWriteInput(
  input: import('@/types/terminal').NativeInputFrame,
): Promise<import('@/types/terminal').InputWriteReceipt> {
  const bridge = nativeDocumentBridge()
  const key = {
    runId: input.runId,
    generation: input.generation,
    inputSeq: input.inputSeq,
  }

  try {
    await bridge.invoke('cli_input_begin', {
      ...key,
      modeEpoch: input.modeEpoch,
      totalBytes: String(input.bytes.byteLength),
    })

    for (let offset = 0; offset < input.bytes.byteLength; offset += NATIVE_INPUT_UPLOAD_CHUNK_BYTES) {
      const chunk = input.bytes.subarray(
        offset,
        Math.min(offset + NATIVE_INPUT_UPLOAD_CHUNK_BYTES, input.bytes.byteLength),
      )
      await bridge.invoke('cli_input_chunk', {
        ...key,
        offset: String(offset),
        bytes: Array.from(chunk),
      })
    }
  } catch (failure) {
    try {
      await bridge.invoke('cli_input_abort', key)
    } catch {
      // Staging-only cleanup is best effort. Never hide or replace the original
      // transport failure and never retry the user payload automatically.
    }
    throw failure
  }

  return bridge.invoke('cli_input_commit', key) as Promise<
    import('@/types/terminal').InputWriteReceipt
  >
}

export async function cliWriteProtocol(
  run: import('@/types/terminal').RunKey,
  bytes: Uint8Array,
): Promise<import('@/types/terminal').ProtocolWriteReceipt> {
  if (bytes.byteLength === 0 || bytes.byteLength > NATIVE_INPUT_UPLOAD_CHUNK_BYTES) {
    throw new Error('INVALID_PROTOCOL_INPUT_SIZE')
  }
  return nativeDocumentBridge().invoke('cli_input_protocol', {
    ...run,
    bytes: Array.from(bytes),
  }) as Promise<import('@/types/terminal').ProtocolWriteReceipt>
}


export function createCliLaunchAttempt<E>(
  request: import('@/types/cli').LaunchRequest,
  channel: import('@tauri-apps/api/core').Channel<E>,
): import('./cliLaunchAttempt').LaunchAttempt {
  const bridge = nativeDocumentBridge();
  if (typeof bridge.instanceId !== 'string' || !bridge.instanceId) {
    throw new Error('DOCUMENT_BRIDGE_UNAVAILABLE');
  }
  // Keep the original bridge as well as the original Channel. A new document
  // or backend cannot silently inherit and restart an uncertain old attempt.
  return createLaunchAttempt(request, bridge.instanceId, {
    start: (frozen) => bridge.invoke('cli_start', frozen, channel),
    status: (requestId) => bridge.invoke('cli_get_launch_status', { requestId }),
  });
}

export function createNativeProjectionClient(): import('./nativeProjection').ProjectionClient {
  return createProjectionClient(nativeDocumentBridge());
}
export async function nativeGetScope(target: import('@/types/nativeProjection').ScopeTarget): Promise<import('@/types/nativeProjection').SourceRef> {
  return createNativeProjectionClient().scope(target);
}
export async function nativeListResources(request: import('@/types/nativeProjection').ReadRequest): Promise<import('@/types/nativeProjection').ProjectionResult> {
  return createNativeProjectionClient().read(request);
}
