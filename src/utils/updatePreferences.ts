import { isReleaseChannel, type ReleaseChannel } from './updatePolicy'
import { updateFailure } from './updateErrors'

const storageKey = 'cc-desk-update-preferences-v1'
export const updateHistoryEvents = ['available', 'no-update', 'failed', 'downloading', 'verifying', 'installing', 'restarting', 'cancelled', 'ignored', 'ignore-cleared'] as const
type HistoryEvent = typeof updateHistoryEvents[number]
export interface IgnoredCandidate { version: string; releaseId: number; sourceSha: string }
export interface UpdateHistoryEntry { at: number; event: HistoryEvent; version: string | null; channel: ReleaseChannel; failure: ReturnType<typeof updateFailure> | null }
export interface UpdatePreferences { ignored: IgnoredCandidate | null; history: UpdateHistoryEntry[] }
export function updateVersion(value: unknown): string | null {
  return typeof value === 'string' && value.length <= 64 && /^(?:0|[1-9]\d{0,8})\.(?:0|[1-9]\d{0,8})\.(?:0|[1-9]\d{0,8})(?:-(?:beta|rc|preview)\.[1-9]\d{0,8})?$/.test(value) ? value : null
}
function record(value: unknown): value is Record<string, unknown> { return !!value && typeof value === 'object' && !Array.isArray(value) }
export function safeIgnoredCandidate(value: unknown): IgnoredCandidate | null {
  if (!record(value) || !updateVersion(value.version) || !Number.isSafeInteger(value.releaseId) || (value.releaseId as number) < 1
    || typeof value.sourceSha !== 'string' || !/^[a-f0-9]{40}$/.test(value.sourceSha)) return null
  return { version: value.version as string, releaseId: value.releaseId as number, sourceSha: value.sourceSha }
}
export function safeHistoryEntry(value: unknown): UpdateHistoryEntry | null {
  if (!record(value) || !Number.isSafeInteger(value.at) || (value.at as number) < 0 || (value.at as number) > 8640000000000000 || !isReleaseChannel(value.channel)
    || !updateHistoryEvents.some(event => event === value.event)) return null
  return { at: value.at as number, event: value.event as HistoryEvent, channel: value.channel,
    version: updateVersion(value.version), failure: value.failure ? updateFailure(value.failure) : null }
}
export function readUpdatePreferences(): { value: UpdatePreferences; persistent: boolean } {
  try {
    const raw = localStorage.getItem(storageKey)
    if (!raw) return { value: { ignored: null, history: [] }, persistent: true }
    if (raw.length > 32768) throw new Error()
    const data: unknown = JSON.parse(raw)
    if (!record(data) || data.schema !== 1) throw new Error()
    const ignored = safeIgnoredCandidate(data.ignored)
    const history = Array.isArray(data.history) ? data.history.slice(0, 20).map(safeHistoryEntry).filter((entry): entry is UpdateHistoryEntry => entry !== null) : []
    const historyValid = Array.isArray(data.history) && data.history.length <= 20 && data.history.length === history.length
      && data.history.every((entry, index) => record(entry) && Object.keys(entry).every(key => ['at', 'event', 'channel', 'version', 'failure'].includes(key))
        && entry.version === history[index]?.version && (entry.failure === null || record(entry.failure)
          && Object.keys(entry.failure).every(key => ['code', 'stage', 'key'].includes(key))
          && entry.failure.code === history[index]?.failure?.code && entry.failure.stage === history[index]?.failure?.stage
          && entry.failure.key === history[index]?.failure?.key))
    const ignoredValid = data.ignored === null || !!ignored && record(data.ignored)
      && Object.keys(data.ignored).length === 3
    return { value: { ignored, history }, persistent: historyValid && ignoredValid && Object.keys(data).every(key => ['schema', 'ignored', 'history'].includes(key)) }
  } catch { return { value: { ignored: null, history: [] }, persistent: false } }
}
export function writeUpdatePreferences(value: UpdatePreferences): boolean {
  try {
    const history = value.history.slice(0, 20).map(safeHistoryEntry).filter((entry): entry is UpdateHistoryEntry => entry !== null)
    localStorage.setItem(storageKey, JSON.stringify({ schema: 1, ignored: safeIgnoredCandidate(value.ignored), history }))
    return true
  } catch { return false }
}
