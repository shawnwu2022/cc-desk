import type { UnifiedSession } from '@/types/unifiedSession'

/** Open terminals always precede closed/history rows. Within each group the
 * accepted opening time descends, then the exact identity breaks ties. */
export function compareSessionOpenOrder(left: UnifiedSession, right: UnifiedSession): number {
  return Number(right.opened === true) - Number(left.opened === true)
    || (right.lastOpenedAt ?? 0) - (left.lastOpenedAt ?? 0) || left.id.localeCompare(right.id)
}
