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
  /** 仅 close 路径使用：读失败时保留已有显示行（本地证据最强），错误标注仍阻止 absence 误判。 */
  retainOnFailure?: boolean
}

export type NativeHistorySession = Extract<ResourceItem, { type: 'session' }> & {
  /** Display-only prior complete timestamp. Never written into source updatedAt. */
  lastKnownActivityAt?: number
  /** Display-only title retained when the current bounded observation is unknown. */
  lastKnownTitle?: string
  lastKnownTitleSource?: 'prompt' | 'ai' | 'custom'
}

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
  /** Opaque backend-admitted identity; retained even when enumeration fails. */
  sourceRootKey?: string
  requestEpoch: string
  /** Only one complete authenticated response can prove absence. Consecutive
   * pages can share a bounded backend observation, but do not grant additional
   * absence authority and remain positive discovery only. */
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

  /**
   * 一次后端观察服务同 profile 的整组项目：行经后端 cwd 归属分发到各项目 entry。
   * 启动巨慢的根因是每个项目各做一次全库扫描；合并后扫描次数从项目数降为 1。
   * 逐 entry 的 owner/inFlight 语义与单项目路径完全一致，并发单项目 load 可整组接管。
   */
  async function loadGrouped(
    group: { cli: NativeCliKind; profileId: string; profileRevision: string; projects: { projectId: string; projectPath: string }[] },
    options: { force?: boolean; retainOnFailure?: boolean } = {},
  ): Promise<Map<string, NativeHistoryEntry>> {
    const profile = {
      cli: group.cli,
      profileId: required(group.profileId, 'PROFILE_ID_REQUIRED'),
      profileRevision: required(group.profileRevision, 'PROFILE_REVISION_REQUIRED'),
    }
    const contexts = group.projects.map(project => ({ ...profile,
      projectId: required(project.projectId, 'PROJECT_ID_REQUIRED'),
      projectPath: required(project.projectPath, 'PROJECT_PATH_REQUIRED'),
    }))
    // 重复 projectId 无法唯一归属后端行，直接拒绝而不是静默合并。
    if (new Set(contexts.map(context => context.projectId)).size !== contexts.length) throw new Error('PROJECT_ID_REQUIRED')

    const result = new Map<string, NativeHistoryEntry>()
    const claimed: { context: (typeof contexts)[number]; key: string; cached?: NativeHistoryEntry }[] = []
    for (const context of contexts) {
      const key = nativeHistoryContextKey(context)
      const flight = inFlight.get(key)
      if (!options.force && flight) { result.set(context.projectId, await flight.promise); continue }
      const cached = entries.get(key)
      if (!options.force && cached?.loaded) { result.set(context.projectId, cached); continue }
      claimed.push({ context, key, cached })
    }
    if (!claimed.length) return result

    const owner = {}
    if (sequence === BigInt('18446744073709551615')) throw new Error('SCOPE_EPOCH_EXHAUSTED')
    let epoch = (++sequence).toString()
    const replacement = (key: string): Promise<NativeHistoryEntry> => {
      const newer = inFlight.get(key)
      if (newer && newer.owner !== owner) return newer.promise
      const current = entries.get(key)
      return current ? Promise.resolve(current)
        : Promise.reject(projectionFailure({ code: 'SCOPE_REVOKED' }, 'read-capability'))
    }

    const work = (async () => {
      let stage: ProjectionStage = 'frontend-bridge'
      const state = claimed.map(({ context, key, cached }) => {
        owners.set(key, owner)
        const entry = reactive<NativeHistoryEntry>({
          key, context, sessions: cached?.sessions ?? [], loading: true, loaded: false, error: null, requestEpoch: epoch,
        })
        entries.set(key, entry)
        return { context, key, entry, cached,
          sessions: new Map<string, NativeHistorySession>(),
          previousSessions: new Map<string, NativeHistorySession>() }
      })
      const live = () => state.filter(item => owners.get(item.key) === owner)
      try {
        const client = createNativeProjectionClient()
        stage = 'scope-invoke'
        const source = await client.scope({
          kind: 'profile', profileId: profile.profileId, expectedProfileRevision: profile.profileRevision,
          projectIds: state.map(item => item.context.projectId),
        })
        if (!live().length) return
        if (source.cli !== profile.cli) throw new Error('PROFILE_CLI_MISMATCH')
        const sourceRootKey = typeof source.sourceRootKey === 'string' && source.sourceRootKey ? source.sourceRootKey : undefined
        if (sourceRootKey) for (const { entry } of state) entry.sourceRootKey = sourceRootKey
        // 既有显示值的保留只对同一 backend root key 成立，跨 root 重新观察。
        for (const item of state) {
          item.previousSessions = new Map(item.entry.sourceRootKey && item.cached?.sourceRootKey === item.entry.sourceRootKey
            ? item.cached?.sessions.map(row => [row.sessionKey, row]) ?? [] : [])
        }
        let offset = 0
        let metadataIncomplete = false
        let restartedObservation = false
        const readFailures = new Set<HistoryReadFailure>()
        const absorb = (item: Extract<ResourceItem, { type: 'session' }>, target: (typeof state)[number]) => {
          const previous = target.previousSessions.get(item.sessionKey)
          const observed = item.updatedAt ? Date.parse(item.updatedAt) : NaN
          const priorUpdated = previous?.updatedAt ? Date.parse(previous.updatedAt) : NaN
          const prior = Number.isFinite(priorUpdated) ? priorUpdated : previous?.lastKnownActivityAt ?? NaN
          // Same canonical context + backend root/session key only. No new row,
          // source timestamp, completeness or absence evidence is inferred.
          const known = previous?.nativeSessionId === item.nativeSessionId && Number.isFinite(prior) && prior >= 0 && !Number.isFinite(observed)
            ? prior : undefined
          const priorTitle = previous?.lastKnownTitle ?? previous?.title
          const priorSource = previous?.lastKnownTitleSource ?? previous?.titleSource
          const priority = (value: typeof item.titleSource) => value === 'custom' ? 3 : value === 'ai' ? 2 : value === 'prompt' ? 1 : 0
          // Display retention only: a bounded lower-priority prompt fallback
          // does not establish that a previously observed AI/custom name vanished.
          const lowerPriorityPartial = item.metadataIncomplete === true && priority(priorSource) > priority(item.titleSource)
          const lastKnownTitle = (item.titleUnknown === true || lowerPriorityPartial) && previous?.nativeSessionId === item.nativeSessionId
            && typeof priorTitle === 'string' && priorTitle ? priorTitle : undefined
          target.sessions.set(item.sessionKey, { ...item,
            ...(known === undefined ? {} : { lastKnownActivityAt: known }),
            ...(lastKnownTitle === undefined ? {} : { lastKnownTitle, ...(priorSource ? { lastKnownTitleSource: priorSource } : {}) }),
          })
        }
        while (true) {
          stage = 'read-invoke'
          const result = await client.read({ source, resourceKind: 'history', requestEpoch: epoch, limit: 200, offset })
          if (!live().length) return
          if (result.state !== 'ready') {
            if (result.reason === 'SOURCE_SNAPSHOT_EXPIRED' && offset > 0 && !restartedObservation) {
              // Restart one entire owned observation; never append fresh pages
              // to rows from an expired/evicted receipt. No root-error retry.
              restartedObservation = true
              if (sequence === BigInt('18446744073709551615')) throw new Error('SCOPE_EPOCH_EXHAUSTED')
              epoch = (++sequence).toString()
              offset = 0
              metadataIncomplete = false
              readFailures.clear()
              for (const item of live()) {
                item.entry.requestEpoch = epoch
                item.sessions.clear()
                item.entry.metadataIncomplete = undefined
                item.entry.readFailures = undefined
              }
              continue
            }
            for (const { entry } of live()) {
              entry.error = result.reason ?? 'SOURCE_UNAVAILABLE'
              entry.diagnosticStage = 'read-source-enumeration'
              if (!options.retainOnFailure) entry.sessions = []
            }
            break
          }
          metadataIncomplete ||= result.historyMetadataIncomplete === true
          for (const code of result.historyReadFailures ?? []) readFailures.add(code)
          if (readFailures.size) metadataIncomplete = true
          for (const { entry } of live()) {
            if (readFailures.size) entry.readFailures = [...readFailures]
            entry.metadataIncomplete = metadataIncomplete
          }
          for (const item of result.items) {
            if (item.type !== 'session') continue
            // 单项目观察的归属唯一，后端省略 projectId 时归给唯一 entry；
            // 多项目观察必须有归属，否则该行不属于任何已 admission 项目。
            const target = item.projectId
              ? state.find(entry => entry.context.projectId === item.projectId)
              : state.length === 1 ? state[0] : undefined
            if (target && owners.get(target.key) === owner) absorb(item, target)
          }
          if (!result.hasMore) {
            for (const item of live()) {
              item.entry.sessions = [...item.sessions.values()]
              if (offset === 0 && !metadataIncomplete && sourceRootKey) {
                item.entry.absenceEvidence = { cli: source.cli, sourceRootKey }
              }
            }
            break
          }
          if (!result.items.length || offset + result.items.length > 1_000_000) throw new Error('SOURCE_UNAVAILABLE')
          offset += result.items.length
        }
        for (const { entry } of live()) entry.loaded = entry.error !== 'SOURCE_BUSY'
      } catch (failure) {
        const alive = live()
        if (!alive.length) return
        const diagnostic = projectionFailure(failure, stage)
        for (const { entry } of alive) {
          entry.error = diagnostic.code
          entry.diagnosticStage = diagnostic.stage
          if (!options.retainOnFailure) entry.sessions = []
          entry.loaded = diagnostic.code !== 'SOURCE_BUSY'
        }
        throw diagnostic
      } finally {
        for (const { entry } of state) if (owners.get(entry.key) === owner) entry.loading = false
      }
    })()
    for (const { key } of claimed) {
      const promise = work.then(
        () => (inFlight.get(key)?.owner === owner ? entries.get(key)! : replacement(key)),
        failure => (inFlight.get(key)?.owner === owner ? Promise.reject(failure) : replacement(key)),
      )
      inFlight.set(key, { owner, promise })
      // 逐 entry promise 与整组 work 一起失败：await 方仍看到 rejection，
      // 但未逐个 await 的 entry 不能变成 unhandled rejection。
      void promise.catch(() => {})
    }
    const release = () => { for (const { key } of claimed) if (inFlight.get(key)?.owner === owner) inFlight.delete(key) }
    // Register cleanup after publication, including synchronous bridge failures.
    void work.then(release, release)
    // 单项目调用仍以 rejection 暴露失败，且不产生未经消费的整组 rejection。
    await (claimed.length === 1 ? work : work.then(() => undefined, () => undefined))
    for (const { context, key } of claimed) result.set(context.projectId, entries.get(key)!)
    return result
  }

  async function load(input: NativeHistoryContext): Promise<NativeHistoryEntry> {
    const map = await loadGrouped(
      { cli: input.cli, profileId: required(input.profileId, 'PROFILE_ID_REQUIRED'),
        profileRevision: required(input.profileRevision, 'PROFILE_REVISION_REQUIRED'),
        projects: [{ projectId: required(input.projectId, 'PROJECT_ID_REQUIRED'), projectPath: required(input.projectPath, 'PROJECT_PATH_REQUIRED') }] },
      { force: input.force, retainOnFailure: input.retainOnFailure },
    )
    const entry = map.get(required(input.projectId, 'PROJECT_ID_REQUIRED'))
    if (!entry) throw projectionFailure({ code: 'SCOPE_REVOKED' }, 'read-capability')
    // 被更强 owner 接管时，本次 load 必须等到接管方完成而不是立刻返回加载中的 entry；
    // 否则调用方会把「尚未验证」的中间状态当成验证结果。
    const owned = inFlight.get(entry.key)
    return owned ? owned.promise : entry
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

  return { entries, get, all, load, loadGrouped, invalidate }
})
