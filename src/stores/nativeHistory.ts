import { defineStore } from 'pinia'
import { reactive } from 'vue'
import { createNativeProjectionClient } from '@/api/tauri'
import type { NativeCliKind } from '@/types/cli'
import type { HistoryReadFailure, ResourceItem, SourceRef } from '@/types/nativeProjection'
import { projectionFailure, type ProjectionStage } from '@/api/nativeProjection'
import { normalizePath } from '@/utils/path'

export interface NativeHistoryContext {
  cli: NativeCliKind
  profileId: string
  profileRevision: string
  projectId: string
  projectPath: string
  force?: boolean
  /** 读失败时保留已发布显示行（本地证据最强）；错误标注仍阻止恢复/缺席判定。 */
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
  source?: SourceRef
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

export function createNativeHistoryReader() {
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
   * 同 profile 按 scope 的 64 项预算合并观察，行经后端归属分发到各项目 entry。
   * 项目准入失败时拆分该批次，健康项目仍使用独立的后端授权观察。
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
    const epoch = (++sequence).toString()
    const replacement = (key: string): Promise<NativeHistoryEntry> => {
      const newer = inFlight.get(key)
      if (newer && newer.owner !== owner) return newer.promise
      const current = entries.get(key)
      return current ? Promise.resolve(current)
        : Promise.reject(projectionFailure({ code: 'SCOPE_REVOKED' }, 'read-capability'))
    }

    const work = (async () => {
      const state = claimed.map(({ context, key, cached }) => {
        owners.set(key, owner)
        const entry = reactive<NativeHistoryEntry>({
          key, context, sessions: cached?.sessions ?? [], loading: true, loaded: false, error: null, requestEpoch: epoch,
          // root 是 scope（目录）的身份属性：同一 context 的已证明 root 在本轮 scope
          // 返回前不可能合法变化，沿用缓存值。否则重载过渡态丢失 root，同源折叠与
          // 归档判断（bare/legacy 路径）失效一个往返，列表会闪出双行与归档行。
          ...(cached?.sourceRootKey ? { sourceRootKey: cached.sourceRootKey, source: cached.source } : {}),
        })
        entries.set(key, entry)
        return { context, key, entry, cached,
          sessions: new Map<string, NativeHistorySession>(),
          previousSessions: new Map<string, NativeHistorySession>(),
          // 本轮观察到的来源；在发布行之前不写入 entry —— root 与行必须同时换。
          observedRootKey: undefined as string | undefined,
          observedSource: undefined as SourceRef | undefined }
      })
      const observe = async (batch: typeof state): Promise<void> => {
        let stage: ProjectionStage = 'frontend-bridge'
        let observationEpoch = epoch
        const live = () => batch.filter(item => owners.get(item.key) === owner)
        if (!live().length) return
        try {
          const client = createNativeProjectionClient()
          stage = 'scope-invoke'
          const source = await client.scope({
            kind: 'profile', profileId: profile.profileId, expectedProfileRevision: profile.profileRevision,
            projectIds: batch.map(item => item.context.projectId),
          })
          if (!live().length) return
          if (source.cli !== profile.cli) throw new Error('PROFILE_CLI_MISMATCH')
          const sourceRootKey = typeof source.sourceRootKey === 'string' && source.sourceRootKey ? source.sourceRootKey : undefined
          // 新 root 只暂存到本地：root/source/行必须作为同一份快照原子发布。
          // 读取中途换根会把旧 root 的行重标成新 root，归档判断随之翻转。
          for (const item of live()) {
            item.observedRootKey = sourceRootKey
            item.observedSource = source
            // 无已发布行（或根未变化）时立即记录新根：诊断/警告身份需要它，且不存
            // 在重标风险。已有行且根变化时延迟到提交，避免旧行被重标成新来源。
            const rootUnchanged = !sourceRootKey || !item.entry.sessions.length || item.entry.sourceRootKey === sourceRootKey
            if (rootUnchanged && sourceRootKey) {
              item.entry.sourceRootKey = sourceRootKey
              item.entry.source = source
            }
            // 既有显示值的保留只对同一 backend root key 成立，跨 root 重新观察。
            item.previousSessions = new Map(sourceRootKey && item.cached?.sourceRootKey === sourceRootKey
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
            const priorKnown = previous?.lastKnownActivityAt ?? NaN
            const prior = Math.max(Number.isFinite(priorUpdated) ? priorUpdated : -1, Number.isFinite(priorKnown) ? priorKnown : -1)
            // Same canonical context + backend root/session key only. No new row,
            // source timestamp, completeness or absence evidence is inferred.
            // 部分读取的旧时间只是下界；完整新观察可以纠正旧显示值。
            const known = previous?.nativeSessionId === item.nativeSessionId && prior >= 0
              && (!Number.isFinite(observed) || item.metadataIncomplete === true && prior > observed)
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
            const result = await client.read({ source, resourceKind: 'history', requestEpoch: observationEpoch, limit: 200, offset })
            if (!live().length) return
            if (result.state !== 'ready') {
              if (result.reason === 'SOURCE_SNAPSHOT_EXPIRED' && offset > 0 && !restartedObservation) {
                // Restart one entire owned observation; never append fresh pages
                // to rows from an expired/evicted receipt. No root-error retry.
                restartedObservation = true
                if (sequence === BigInt('18446744073709551615')) throw new Error('SCOPE_EPOCH_EXHAUSTED')
                observationEpoch = (++sequence).toString()
                offset = 0
                metadataIncomplete = false
                readFailures.clear()
                for (const item of live()) {
                  item.entry.requestEpoch = observationEpoch
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
                ? batch.find(entry => entry.context.projectId === item.projectId)
                : batch.length === 1 ? batch[0] : undefined
              if (target && owners.get(target.key) === owner) absorb(item, target)
            }
            if (!result.hasMore) {
              for (const item of live()) {
                // 部分观察不能撤销已证明的存在：缺失的行保留上一次同根观察的投影，
                // 直到一次无遗漏的完整观察证明其消失。保留只在真实读取失败
                // （readFailures）时成立 —— 有界元数据读取（metadataIncomplete）
                // 是正常观察，不构成「行可能缺失」的证据，否则会积累幽灵行。
                const merged = readFailures.size
                  ? [...item.sessions.values(), ...(item.observedRootKey && item.cached?.sourceRootKey === item.observedRootKey
                    ? item.cached!.sessions.filter(row => !item.sessions.has(row.sessionKey)) : [])]
                  : [...item.sessions.values()]
                // root、来源与行作为同一份不可拆分的显示快照一起提交：先换 root
                // 再保留旧行会把旧 root 的归档状态重标成新 root。
                if (item.observedRootKey) {
                  item.entry.sourceRootKey = item.observedRootKey
                  item.entry.source = item.observedSource
                }
                item.entry.sessions = merged
                if (offset === 0 && !metadataIncomplete && item.observedRootKey) {
                  item.entry.absenceEvidence = { cli: source.cli, sourceRootKey: item.observedRootKey }
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
          if (stage === 'scope-invoke' && diagnostic.stage === 'scope-project-registration' && batch.length > 1) {
            // 仅隔离后端明确报告的项目准入失败，不能重试 profile/root/读取错误或绕过授权。
            const middle = Math.ceil(alive.length / 2)
            for (const subset of [alive.slice(0, middle), alive.slice(middle)]) {
              if (subset.length) await observe(subset).catch(() => {})
            }
            return
          }
          for (const { entry } of alive) {
            entry.error = diagnostic.code
            entry.diagnosticStage = diagnostic.stage
            if (!options.retainOnFailure) entry.sessions = []
            entry.loaded = diagnostic.code !== 'SOURCE_BUSY'
          }
          throw diagnostic
        }
      }
      try {
        for (let offset = 0; offset < state.length; offset += 64) {
          try { await observe(state.slice(offset, offset + 64)) }
          catch (failure) { if (claimed.length === 1) throw failure }
        }
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
    for (const { context, key } of claimed) {
      const entry = owners.get(key) === owner ? entries.get(key)! : await replacement(key)
      result.set(context.projectId, entry)
    }
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
}

export const useNativeHistoryStore = defineStore('native-history', createNativeHistoryReader)
