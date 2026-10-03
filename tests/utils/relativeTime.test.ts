import { describe, expect, test } from 'vitest'
import { formatRelativeActivity } from '@/utils/relativeTime'

describe('formatRelativeActivity', () => {
  const now = new Date(2026, 8, 28, 16, 32, 0).getTime()

  test('uses the pinned compact units', () => {
    expect(formatRelativeActivity(now - 30_000, now, 'zh')).toBe('刚刚')
    expect(formatRelativeActivity(now - 30_000, now, 'en')).toBe('now')
    expect(formatRelativeActivity(now - 6 * 60_000, now, 'en')).toBe('6m')
    expect(formatRelativeActivity(now - 3 * 3_600_000, now, 'zh')).toBe('3h')
    expect(formatRelativeActivity(now - 11 * 86_400_000, now, 'zh')).toBe('11d')
  })

  test('uses short calendar dates after ninety days', () => {
    expect(formatRelativeActivity(new Date(2026, 3, 8).getTime(), now, 'zh')).toBe('4/8')
    expect(formatRelativeActivity(new Date(2025, 11, 20).getTime(), now, 'en')).toBe('25/12/20')
  })

  test('clamps future activity to now', () => {
    expect(formatRelativeActivity(now + 5_000, now, 'zh')).toBe('刚刚')
  })
})
