import { computed } from 'vue'
import { useSessionStore } from '@/stores/session'
import { useNativeTabsStore } from '@/stores/nativeTabs'
import { useUnifiedSessionsStore } from '@/stores/unifiedSessions'
/** Counts real runtime owners, keeping identical IDs from different runtimes separate. */
export function useOwnedSessionCounts() {
  const legacy = useSessionStore(), native = useNativeTabsStore(), catalog = useUnifiedSessionsStore()
  return computed(() => {
    const owners = [...legacy.tabs.values(), ...native.tabs.values()]
    const preparing = catalog.sessions.filter(row => catalog.isPreparingSession(row.id)
      && !(row.runtime === 'native-cli' ? native.tabs : legacy.tabs).has(row.adapterSessionId))
    return { open: owners.length + preparing.length, running: owners.filter(row => row.status === 'running').length,
      starting: owners.filter(row => row.status === 'starting').length + preparing.filter(row => row.processState === 'starting').length,
      unknown: owners.filter(row => row.status === 'unknown').length + preparing.filter(row => row.processState === 'unknown').length }
  })
}
