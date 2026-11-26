import { readFileSync } from 'node:fs'
import { expect, it } from 'vitest'

it('AndroidSessionBrowser_RefreshKeepsSearchMountedAndKeepsAccessGates_001', () => {
  const main = readFileSync('android-client/src/cn/ccdesk/android/MainActivity.java', 'utf8')
  const home = readFileSync('android-client/src/cn/ccdesk/android/RemoteHomeView.java', 'utf8')
  expect(main.includes('sessionBrowser.refresh()')).toBe(true)
  expect(main.includes('SessionTitleFilter.matches(query, name, offer.label)')).toBe(true)
  expect(main.includes('SessionTitleFilter.matches(query, name, session.title)')).toBe(true)
  expect(main.includes('SessionTitleFilter.matches(query, name, session.text)')).toBe(false)
  expect(main.includes('if (!canUseDesktop(desktop))')).toBe(true)
  expect(home.includes('WindowInsets.Type.ime()')).toBe(true)
})

it('AndroidSessionBrowser_RecreationSavesOnlyHomeTab_002', () => {
  const main = readFileSync('android-client/src/cn/ccdesk/android/MainActivity.java', 'utf8')
  expect(main).toMatch(/showingSessions = state != null && state\.getBoolean\(HOME_SESSIONS, false\)/)
  const save = main.match(/void onSaveInstanceState\(Bundle state\) \{([^}]+)\}/)?.[1]
  expect(save?.trim().replace(/\s+/g, ' ')).toBe('super.onSaveInstanceState(state); state.putBoolean(HOME_SESSIONS, showingSessions);')
  expect(main).toContain('search.setSaveEnabled(false)')
  expect(main).toContain('FLAG_SECURE')
})


it('AndroidSessionBrowser_PartialCoverageIsShownWithAndWithoutMatches_002', () => {
  const main = readFileSync('android-client/src/cn/ccdesk/android/MainActivity.java', 'utf8')
  expect(main).toContain('coverage.isPartial()')
  expect(main).toContain('只搜索已读取的标题')
  expect(main).toContain('设备未连接或列表不可用')
  expect(main).toContain('设备的配对凭据未验证')
  expect(main).toContain('设备正在读取或连接')
  expect(main).toContain('已读取的标题中没有匹配')
  expect(main).toContain('if (canRefreshRow(row)) refreshDesktop(row);')
})
