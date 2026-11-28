import { readFileSync } from 'node:fs'
import { describe, it, expect } from 'vitest'

const activity = readFileSync('android-client/src/cn/ccdesk/android/MainActivity.java', 'utf8')
const home = readFileSync('android-client/src/cn/ccdesk/android/RemoteHomeView.java', 'utf8')
describe('Android paired desktop list wiring', () => {
  it('AndroidDesktopList_KeepsIndexedRowsWhenLocalVerificationIsPending_001', () => {
    expect(activity).toContain('List<DesktopBackend.Desktop> rows = store.read();')
    expect(activity).not.toContain('rows.removeIf(')
    expect(activity).toMatch(/if \(!canUseDesktop\(desktop\)\) \{[^}]*return true;\s*\}/)
    expect(activity).toMatch(/home\.action\(card, "设备设置", false, \(\) -> editDesktopAlias\(row, name\)\)/)
  })
  it('AndroidDesktopList_GatesBulkAndScopedRefreshOnVerification_002', () => {
    expect(activity).toContain('rows.stream().anyMatch(this::canRefreshRow)')
    expect(activity).toContain('if (canUseDesktop(desktop) && canRefresh(backend.state(desktop))) backend.loadShares(desktop, identities);')
    expect(activity).toContain('return backend != null && backend.verified(desktop);')
  })
  it('AndroidHome_SeparatesDevicePairingFromSessionActions_003', () => {
    expect(home).toContain('tab(nav, "设备", !sessions, devices);')
    expect(home).toContain('tab(nav, "会话", sessions, openSessions);')
    expect(activity).toContain('if (showingSessions) renderSessions(rows);')
    expect(activity).toContain('else renderDevices(rows);')
    expect(activity).toMatch(/"去配对电脑", true, \(\) -> navigateHome\(false\)/)
    expect(activity).toContain('controller ? "请求控制" : "查看快照"')
    expect(activity).toContain('return desktop.name.equals("电脑 " + desktop.hostId.substring(0, 8))')
    expect(activity).not.toContain('name + " · " + row.hostId')
  })
  it('AndroidDesktopList_AutomaticallyLoadsOnlyVerifiedOrdinarySessions_004', () => {
    expect(activity).toMatch(/onStart\(\) \{[^}]*loadInitialOrdinarySessions\(\)/)
    expect(activity).toMatch(/private void updateConnectionViews\(\) \{\s+loadInitialOrdinarySessions\(\)/)
    expect(activity).toMatch(/finishEnrollment\(epoch, PairingNotice.SAVED\);\s+loadInitialOrdinarySessions\(\)/)
    expect(activity).toContain('canUseDesktop(row) && backend.state(row) == DesktopBackend.State.DISCONNECTED')
    expect(activity).toContain('initialSessionLoads.add(row.localId)) refreshDesktop(row);')
  })
  it('AndroidDesktopList_RetriesUnavailableOrdinaryCatalogueOnlyWhileForeground_005', () => {
    expect(activity).toContain('ORDINARY_RETRY_MS = 5 * 60_000L')
    expect(activity).toContain('sessionRetryHandler.postDelayed(retryOrdinarySessions, ORDINARY_RETRY_MS)')
    expect(activity).toMatch(/onStop\(\) \{[^}]*sessionRetryHandler.removeCallbacks\(retryOrdinarySessions\)/)
    expect(activity).toContain('if (!canUseDesktop(row) || !initialSessionLoads.contains(row.localId)) continue;')
    expect(activity).toMatch(/state == DesktopBackend.State.DISCONNECTED\s*\|\| state == DesktopBackend.State.UNAVAILABLE\) refreshDesktop\(row\)/)
  })
})
