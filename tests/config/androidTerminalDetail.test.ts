import { readFileSync } from 'node:fs'
import { expect, it } from 'vitest'

const source = (name: string) => readFileSync(`android-client/src/cn/ccdesk/android/${name}.java`, 'utf8')
const main = source('MainActivity')
const panel = source('TerminalControlPanel')

it('TerminalDetail_ReadonlyReconnectOnOriginalVerifiedDevice_001', () => {
  const start = main.indexOf('private void showControlEnded(')
  const end = main.indexOf('private void leaveControl(', start)
  const notice = main.slice(start, end)
  expect(notice).toMatch(/backend\.loadShares\(desktop, identities\)/)
  expect(notice).toMatch(/!active \|\| !canUseDesktop\(desktop\)/)
  expect(notice).toContain('State.REVOKED')
  expect(notice).toContain('unconfirmed')
  expect(notice).not.toMatch(/sendControl|chooseShare|openControl/)
})

it('TerminalDetail_IncomingFrameCannotMutateCompose_002', () => {
  const update = panel.slice(panel.indexOf('void update('), panel.indexOf('private void scheduleScroll('))
  expect(update).toContain('scheduleScroll(viewport.nextLayout())')
  expect(update).not.toMatch(/draft\.getText|actions\.send|scrollTo\(/)
  expect(panel).toContain('if (!active || pending || closed) return;')
})
