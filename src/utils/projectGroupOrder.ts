import type { UnifiedProjectGroup } from '@/types/unifiedSession'

type ProjectGroupOrder = Pick<UnifiedProjectGroup, 'pinned' | 'name' | 'projectKey'>

// Keep the same natural name order regardless of the host's default locale.
const projectNameOrder = new Intl.Collator('en', { numeric: true, sensitivity: 'base' })

/** Order projects by pin and displayed name, independent of session activity. */
export function compareProjectGroups(a: ProjectGroupOrder, b: ProjectGroupOrder): number {
  return Number(b.pinned) - Number(a.pinned)
    || projectNameOrder.compare(a.name, b.name)
    // Exact identity breaks equivalent-name ties without depending on discovery order.
    || (a.projectKey < b.projectKey ? -1 : a.projectKey > b.projectKey ? 1 : 0)
}
