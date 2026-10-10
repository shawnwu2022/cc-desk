import type { UnifiedSession } from '@/types/unifiedSession'

/** Missing historical opening times have one fixed baseline, never activity. */
export function compareSessionOpenOrder(left: UnifiedSession, right: UnifiedSession): number {
  return (right.lastOpenedAt ?? 0) - (left.lastOpenedAt ?? 0) || left.id.localeCompare(right.id)
}
