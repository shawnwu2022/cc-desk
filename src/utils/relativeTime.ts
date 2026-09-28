export type RelativeTimeLocale = 'zh' | 'en'

const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR
const NINETY_DAYS = 90 * DAY

function finiteTimestamp(value: number): number {
  if (!Number.isFinite(value)) throw new Error('INVALID_ACTIVITY_TIME')
  return value
}

export function formatRelativeActivity(
  timestamp: number,
  now: number = Date.now(),
  locale: RelativeTimeLocale = 'zh',
): string {
  const activity = finiteTimestamp(timestamp)
  const reference = finiteTimestamp(now)
  const diff = Math.max(0, reference - activity)

  if (diff < MINUTE) return locale === 'zh' ? '刚刚' : 'now'
  if (diff < HOUR) return `${Math.floor(diff / MINUTE)}m`
  if (diff < DAY) return `${Math.floor(diff / HOUR)}h`
  if (diff < NINETY_DAYS) return `${Math.floor(diff / DAY)}d`

  const value = new Date(activity)
  const current = new Date(reference)
  const month = value.getMonth() + 1
  const day = value.getDate()
  if (value.getFullYear() === current.getFullYear()) return `${month}/${day}`
  const year = String(value.getFullYear() % 100).padStart(2, '0')
  return `${year}/${month}/${day}`
}
