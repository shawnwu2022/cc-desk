import { readFileSync } from 'node:fs'
import { expect, it } from 'vitest'

const main = readFileSync('android-client/src/cn/ccdesk/android/MainActivity.java', 'utf8')
const home = readFileSync('android-client/src/cn/ccdesk/android/RemoteHomeView.java', 'utf8')

it('AndroidPairing_DevicePageHasOneAddEntryAndRecoverableFailure_001', () => {
  const devices = main.slice(main.indexOf('private void renderDevices('), main.indexOf('private void renderSessions('))
  expect(devices.match(/"添加电脑", true, this::showPairingOptions/g)).toHaveLength(1)
  expect(devices).toContain('.setPositiveButton("扫描二维码"')
  expect(devices).toContain('.setNeutralButton("输入配对码"')
  expect(devices).toContain('PairingNotice.FAILED')
  expect(devices).toContain('新的邀请')
  expect(main).toContain('if (!active || epoch != enrollmentEpoch) return;')
})

it('AndroidPairing_SettingsAndNavigationDoNotStartConnections_002', () => {
  const settings = main.slice(main.indexOf('private void editDesktopAlias('), main.indexOf('private void manualPairing('))
  expect(settings).toContain('store.renameAlias(desktop, input.getText().toString())')
  expect(settings).not.toMatch(/loadShares\(|redeemInvite\(|chooseShare\(/)
  expect(main).toContain('home.action(card, "改别名", false, () -> editDesktopAlias(row, name))')
  expect(home.indexOf('root.addView(scroll,')).toBeLessThan(home.indexOf('root.addView(bottomNavigation)'))
})

it('AndroidPairing_ManualAndConfirmationRemainUsableWithKeyboardOrLargeText_003', () => {
  const manual = main.slice(main.indexOf('private void manualPairing('), main.indexOf('private void scanPairing('))
  const confirm = main.slice(main.indexOf('private void confirm('), main.indexOf('private void redeem('))
  expect(manual.match(/addPairingField\(form, "[^"]+", (relay|code)\);/g)).toHaveLength(2)
  expect(manual).toContain('label.setLabelFor(input.getId())')
  expect(manual).toContain('input.setImportantForAutofill(android.view.View.IMPORTANT_FOR_AUTOFILL_NO)')
  expect(manual).toContain('relay.setNextFocusForwardId(code.getId())')
  expect(manual).toContain('relay.requestFocus(); relay.setError')
  expect(manual).toContain('code.requestFocus(); code.setError')
  expect(manual).toContain('dialog.getButton(AlertDialog.BUTTON_POSITIVE).performClick()')
  expect(manual).toContain('new ScrollView(this)')
  expect(manual).toContain('.setView(pairingDialogContent(form))')
  expect(confirm).toContain('.setView(pairingDialogContent(confirmation))')
  expect(manual).toContain('FLAG_SECURE')
  expect(confirm).toContain('FLAG_SECURE')
  expect(manual).toContain('code.getText().clear()')
  expect(manual).toContain('confirm(invite)')
})
