import { readFileSync } from 'node:fs'
import { expect, it } from 'vitest'

it('AndroidTerminal_ExplicitJumpDoesNotFocusOrSend_001', () => {
  const panel = readFileSync('android-client/src/cn/ccdesk/android/TerminalControlPanel.java', 'utf8')
  expect(panel).toContain('jumpToLatest.setOnClickListener(unused -> scheduleScroll(viewport.returnToLatest()))')
  const scroll = panel.slice(panel.indexOf('private void scheduleScroll('), panel.indexOf('private void removePendingLayout('))
  expect(scroll).toContain('viewport.follow(exactLayout)')
  expect(scroll).not.toMatch(/requestFocus\(|actions\.send|actions\.key|draft\.getText/)
  expect(panel.includes('console.fullScroll(')).toBe(false)
  expect(panel.includes('stop.setOnClickListener(unused -> leave())')).toBe(true)
  expect(panel.includes('uncertain = true;')).toBe(true)
})


it('AndroidTerminal_KeyShelfUsesImeInsetsAndKeepsActionsExplicit_002', () => {
  const panel = readFileSync('android-client/src/cn/ccdesk/android/TerminalControlPanel.java', 'utf8')
  const activity = readFileSync('android-client/src/cn/ccdesk/android/MainActivity.java', 'utf8')
  expect(activity).toContain('setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)')
  expect(activity).toContain('controlPanel.requestApplyInsets()')
  expect(panel).toContain('Math.max(safe.bottom, ime.bottom)')
  expect(panel).toContain('keyShelf.updateIme(insets.isVisible(WindowInsets.Type.ime()))')
  expect(panel).toMatch(/Key\.LEFT, Key\.DOWN, Key\.UP, Key\.RIGHT, Key\.TAB/)
  expect(panel).toContain('request(false, (completion) -> actions.key(key, completion))')
  expect(panel).toContain('draft.setEnabled(!closed && !uncertain && !(pending && pendingText))')
  expect(panel).toContain('boolean enabled = active && !pending && !closed;')
  expect(panel).toContain('send.setEnabled(enabled && draft.length() != 0)')
  expect(panel).not.toMatch(/requestFocus\(|showSoftInput\(|setOnEditorActionListener\(/)
})
