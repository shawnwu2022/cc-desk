import { defineStore } from 'pinia'
import { reactive } from 'vue'
import { createNativeProjectionClient } from '@/api/tauri'
import type { NativeCliKind } from '@/types/cli'
import type { HistoryReadFailure, ResourceItem } from '@/types/nativeProjection'
import { projectionFailure, type ProjectionStage } from '@/api/nativeProjection'
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
  diagnosticStage?: ProjectionStage
  metadataIncomplete?: boolean
  readFailures?: HistoryReadFailure[]
  requestEpoch: string
  /** Only one complete authenticated response can prove absence. Offset pages
   * have no common snapshot token and are positive discovery only. */
  absenceEvidence?: { cli: NativeCliKind; sourceRootKey: string }
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
  const inFlight = new Map<string, { owner: object; promise: Promise<NativeHistoryEntry> }>()
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
    const pending = inFlight.get(key)
    if (pending && !input.force) return pending.promise
    const cached = entries.get(key)
    if (cached?.loaded && !input.force) return cached

    const owner = {}
    if (sequence === BigInt('18446744073709551615')) throw new Error('SCOPE_EPOCH_EXHAUSTED')
    owners.set(key, owner)
    const epoch = (++sequence).toString()
    const entry = reactive<NativeHistoryEntry>({
      key,
      context,
      sessions: cached?.sessions ?? [],
      loading: true,
      loaded: false,
      error: null,
      requestEpoch: epoch,
    })
    entries.set(key, entry)

    const replacement = (): Promise<NativeHistoryEntry> => {
      const newer = inFlight.get(key)
      if (newer && newer.owner !== owner) return newer.promise
      const current = entries.get(key)
      return current ? Promise.resolve(current)
        : Promise.reject(projectionFailure({ code: 'SCOPE_REVOKED' }, 'read-capability'))
    }
    const promise = (async () => {
      let stage: ProjectionStage = 'frontend-bridge'
      try {
        const client = createNativeProjectionClient()
        stage = 'scope-invoke'
        const source = await client.scope({
          kind: 'profile',
          profileId: context.profileId,
          expectedProfileRevision: context.profileRevision,
          projectId: context.projectId,
        })
        if (owners.get(key) !== owner) return replacement()
        if (source.cli !== context.cli) throw new Error('PROFILE_CLI_MISMATCH')
        const sessions = new Map<string, NativeHistorySession>()
        let offset = 0
        let metadataIncomplete = false
        const readFailures = new Set<HistoryReadFailure>()
        while (true) {
          stage = 'read-invoke'
          const result = await client.read({ source, resourceKind: 'history', requestEpoch: epoch, limit: 200, offset })
          if (owners.get(key) !== owner) return replacement()
          if (result.state !== 'ready') {
            entry.error = result.reason ?? 'SOURCE_UNAVAILABLE'
            entry.diagnosticStage = 'read-source-enumeration'
            entry.sessions = []
            break
          }
          metadataIncomplete ||= result.historyMetadataIncomplete === true
          for (const code of result.historyReadFailures ?? []) readFailures.add(code)
          if (readFailures.size) { entry.readFailures = [...readFailures]; metadataIncomplete = true }
          entry.metadataIncomplete = metadataIncomplete
          for (const item of result.items) if (item.type === 'session') sessions.set(item.sessionKey, item)
          if (!result.hasMore) {
            entry.sessions = [...sessions.values()]
            if (offset === 0 && !metadataIncomplete && typeof source.sourceRootKey === 'string' && source.sourceRootKey) {
              entry.absenceEvidence = { cli: source.cli, sourceRootKey: source.sourceRootKey }
            }
            break
          }
          if (!result.items.length || offset + result.items.length > 1_000_000) throw new Error('SOURCE_UNAVAILABLE')
          offset += result.items.length
        }
        entry.loaded = entry.error !== 'SOURCE_BUSY'
        return entry
      } catch (failure) {
        if (owners.get(key) !== owner) return replacement()
        const diagnostic = projectionFailure(failure, stage)
        entry.error = diagnostic.code
        entry.diagnosticStage = diagnostic.stage
        entry.sessions = []
        entry.loaded = diagnostic.code !== 'SOURCE_BUSY'
        throw diagnostic
      } finally {
        if (owners.get(key) === owner) entry.loading = false
      }
    })()
    inFlight.set(key, { owner, promise })
    const release = () => { if (inFlight.get(key)?.owner === owner) inFlight.delete(key) }
    // Register cleanup after publication, including synchronous bridge failures.
    void promise.then(release, release)
    return promise
  }

  function invalidate(input?: Omit<NativeHistoryContext, 'force'>): void {
    if (!input) {
      for (const key of entries.keys()) owners.set(key, {})
      entries.clear()
      inFlight.clear()
      return
    }
    const key = nativeHistoryContextKey(input)
    owners.set(key, {})
    entries.delete(key)
    inFlight.delete(key)
  }

  return { entries, get, all, load, invalidate }
})
