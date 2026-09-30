import { defineStore } from 'pinia'
import { reactive } from 'vue'
import { createNativeProjectionClient } from '@/api/tauri'
import type { NativeCliKind } from '@/types/cli'
import type { ResourceItem } from '@/types/nativeProjection'
import { normalizePath } from '@/utils/path'

export interface NativeHistoryContext {
  cli: NativeCliKind
  profileId: string
  profileRevision: string
  projectId: string
  projectPath: string
  force?: boolean
}

export type NativeHistorySession = Extract<ResourceItem, { type: 'session' }>

export interface NativeHistoryEntry {
  key: string
  context: Omit<NativeHistoryContext, 'force'>
  sessions: NativeHistorySession[]
  loading: boolean
  loaded: boolean
  error: string | null
  requestEpoch: string
}

function required(value: string, code: string): string {
  const text = value.trim()
  if (!text || text.includes('\0')) throw new Error(code)
  return text
}

function normalizeProjectIdentity(value: string): string {
  const normalized = normalizePath(value)
  return (/^[A-Za-z]:\//.test(normalized) || normalized.startsWith('//'))
    ? normalized.toLocaleLowerCase('en-US')
    : normalized
}

export function nativeHistoryContextKey(input: Omit<NativeHistoryContext, 'force'>): string {
  return JSON.stringify([
    'native-history-v1',
    input.cli,
    required(input.profileId, 'PROFILE_ID_REQUIRED'),
    required(input.profileRevision, 'PROFILE_REVISION_REQUIRED'),
    required(input.projectId, 'PROJECT_ID_REQUIRED'),
    normalizeProjectIdentity(required(input.projectPath, 'PROJECT_PATH_REQUIRED')),
  ])
}

export const useNativeHistoryStore = defineStore('native-history', () => {
  const entries = reactive(new Map<string, NativeHistoryEntry>())
  const owners = new Map<string, object>()
  let sequence = BigInt(0)

  function get(input: Omit<NativeHistoryContext, 'force'>): NativeHistoryEntry | undefined {
    return entries.get(nativeHistoryContextKey(input))
  }

  function all(): NativeHistoryEntry[] {
    return [...entries.values()]
  }

  async function load(input: NativeHistoryContext): Promise<NativeHistoryEntry> {
    const context = {
      cli: input.cli,
      profileId: required(input.profileId, 'PROFILE_ID_REQUIRED'),
      profileRevision: required(input.profileRevision, 'PROFILE_REVISION_REQUIRED'),
      projectId: required(input.projectId, 'PROJECT_ID_REQUIRED'),
      projectPath: required(input.projectPath, 'PROJECT_PATH_REQUIRED'),
    }
    const key = nativeHistoryContextKey(context)
    const cached = entries.get(key)
    if (cached?.loaded && !input.force) return cached

    const owner = {}
    owners.set(key, owner)
    if (sequence === BigInt('18446744073709551615')) throw new Error('SCOPE_EPOCH_EXHAUSTED')
    const epoch = (++sequence).toString()
    const entry: NativeHistoryEntry = {
      key,
      context,
      sessions: cached?.sessions ?? [],
      loading: true,
      loaded: false,
      error: null,
      requestEpoch: epoch,
    }
    entries.set(key, entry)

    try {
      const client = createNativeProjectionClient()
      const source = await client.scope({
        kind: 'profile',
        profileId: context.profileId,
        expectedProfileRevision: context.profileRevision,
        projectId: context.projectId,
      })
      if (owners.get(key) !== owner) return entries.get(key) ?? entry
      if (source.cli !== context.cli) throw new Error('PROFILE_CLI_MISMATCH')
      const result = await client.read({ source, resourceKind: 'history', requestEpoch: epoch, limit: 200 })
      if (owners.get(key) !== owner) return entries.get(key) ?? entry
      if (result.state !== 'ready') {
        entry.error = result.reason ?? 'SOURCE_UNAVAILABLE'
        entry.sessions = []
      } else {
        entry.sessions = result.items.filter((item): item is NativeHistorySession => item.type === 'session')
      }
      entry.loaded = true
      return entry
    } catch (failure) {
      if (owners.get(key) !== owner) return entries.get(key) ?? entry
      entry.error = failure instanceof Error ? failure.message : 'SOURCE_UNAVAILABLE'
      entry.sessions = []
      entry.loaded = true
      throw failure
    } finally {
      if (owners.get(key) === owner) entry.loading = false
    }
  }

  function invalidate(input?: Omit<NativeHistoryContext, 'force'>): void {
    if (!input) {
      for (const key of entries.keys()) owners.set(key, {})
      entries.clear()
      return
    }
    const key = nativeHistoryContextKey(input)
    owners.set(key, {})
    entries.delete(key)
  }

  return { entries, get, all, load, invalidate }
})