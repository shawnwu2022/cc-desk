import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'
import type { LaunchStatus } from '@/api/cliLaunchAttempt'
import type { LaunchAction, NativeCliKind } from '@/types/cli'
import { parseU64 } from '@/utils/nativeIdentity'

export type NativeTabStatus =
  | 'stopped'
  | 'starting'
  | 'running'
  | 'unknown'
  | 'failed'
  | 'exited'

export interface NativeCliTab {
  tabId: string
  cli: NativeCliKind
  projectId: string
  projectPath: string
  profileId: string
  profileRevision: string
  requestId: string
  runId: string
  generation: number
  action: LaunchAction
  status: NativeTabStatus
  errorCode: string | null
  launchRevision: string | null
}

export interface NativeTabCreate {
  cli: NativeCliKind
  projectId: string
  projectPath: string
  profileId: string
  profileRevision: string
  action: LaunchAction
}

function id(prefix: string): string {
  return `${prefix}-${crypto.randomUUID()}`
}

function text(value: string, code: string): string {
  if (!value || value.includes('\0')) throw new Error(code)
  return value
}

function revision(value: string): string {
  try {
    parseU64(value)
    return value
  } catch {
    throw new Error('INVALID_PROFILE_REVISION')
  }
}

function copyAction(action: LaunchAction): LaunchAction {
  return structuredClone(action)
}

function snapshot(tab: NativeCliTab): NativeCliTab {
  return {
    ...tab,
    action: copyAction(tab.action),
  }
}

function statusFromLaunch(value: LaunchStatus): {
  status: NativeTabStatus
  errorCode: string | null
} {
  switch (value.phase) {
    case 'reserved':
    case 'starting':
      return { status: 'starting', errorCode: null }
    case 'running':
      return { status: 'running', errorCode: null }
    case 'indeterminate':
      return { status: 'unknown', errorCode: 'LAUNCH_STATE_UNKNOWN' }
    case 'failed':
      return { status: 'failed', errorCode: value.failure ?? 'LAUNCH_FAILED' }
    case 'cancelled':
      return { status: 'failed', errorCode: value.failure ?? 'LAUNCH_CANCELLED' }
    case 'exited':
      return { status: 'exited', errorCode: null }
  }
}

export const useNativeTabsStore = defineStore('native-cli-tabs', () => {
  const tabs = reactive(new Map<string, NativeCliTab>())
  const activeTabId = ref<string | null>(null)

  function create(input: NativeTabCreate): NativeCliTab {
    if (input.cli !== 'claude' && input.cli !== 'codex') {
      throw new Error('NATIVE_CLI_REQUIRED')
    }
    const tabId = id('tab')
    const value: NativeCliTab = {
      tabId,
      cli: input.cli,
      projectId: text(input.projectId, 'PROJECT_ID_REQUIRED'),
      projectPath: text(input.projectPath, 'PROJECT_PATH_REQUIRED'),
      profileId: text(input.profileId, 'PROFILE_ID_REQUIRED'),
      profileRevision: revision(input.profileRevision),
      requestId: id('request'),
      runId: id('run'),
      generation: 1,
      action: copyAction(input.action),
      status: 'stopped',
      errorCode: null,
      launchRevision: null,
    }
    tabs.set(tabId, value)
    activeTabId.value = tabId
    return snapshot(value)
  }

  function tab(tabId: string): NativeCliTab | undefined {
    return tabs.get(tabId)
  }

  function byProject(projectPath: string): NativeCliTab[] {
    return [...tabs.values()].filter(item => item.projectPath === projectPath)
  }

  function setActive(tabId: string | null): void {
    if (tabId !== null && !tabs.has(tabId)) throw new Error('TAB_NOT_FOUND')
    activeTabId.value = tabId
  }

  function markStarting(tabId: string): void {
    const value = tabs.get(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    value.status = 'starting'
    value.errorCode = null
  }

  function markUnknown(tabId: string): void {
    const value = tabs.get(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    value.status = 'unknown'
    value.errorCode = 'LAUNCH_STATE_UNKNOWN'
  }

  function markError(tabId: string, code: string): void {
    const value = tabs.get(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    value.status = 'failed'
    value.errorCode = text(code, 'ERROR_CODE_REQUIRED')
  }

  function applyLaunchStatus(tabId: string, launch: LaunchStatus): boolean {
    const value = tabs.get(tabId)
    if (!value) return false
    if (
      launch.requestId !== value.requestId
      || launch.run.runId !== value.runId
      || launch.run.generation !== value.generation
    ) {
      return false
    }
    const next = statusFromLaunch(launch)
    value.status = next.status
    value.errorCode = next.errorCode
    value.launchRevision = launch.revision
    return true
  }

  function restart(
    tabId: string,
    profile: {
      profileId: string
      profileRevision: string
      cli?: NativeCliKind
    },
  ): NativeCliTab {
    const value = tabs.get(tabId)
    if (!value) throw new Error('TAB_NOT_FOUND')
    if (value.status === 'unknown') throw new Error('LAUNCH_STATE_UNKNOWN')
    if (value.status === 'running' || value.status === 'starting') {
      throw new Error('TAB_STILL_RUNNING')
    }
    if (profile.cli !== undefined && profile.cli !== value.cli) {
      throw new Error('PROFILE_CLI_MISMATCH')
    }
    if (value.generation >= 0xffffffff) throw new Error('GENERATION_EXHAUSTED')

    value.profileId = text(profile.profileId, 'PROFILE_ID_REQUIRED')
    value.profileRevision = revision(profile.profileRevision)
    value.requestId = id('request')
    value.runId = id('run')
    value.generation += 1
    value.status = 'stopped'
    value.errorCode = null
    value.launchRevision = null
    activeTabId.value = tabId
    return snapshot(value)
  }

  function close(tabId: string): void {
    if (!tabs.delete(tabId)) return
    if (activeTabId.value === tabId) {
      activeTabId.value = tabs.keys().next().value ?? null
    }
  }

  function clear(): void {
    tabs.clear()
    activeTabId.value = null
  }

  return {
    tabs,
    activeTabId,
    create,
    tab,
    byProject,
    setActive,
    markStarting,
    markUnknown,
    markError,
    applyLaunchStatus,
    restart,
    close,
    clear,
  }
})
