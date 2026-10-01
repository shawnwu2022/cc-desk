import { afterEach, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import SessionDiagnosticsDialog from '@/components/sessions/SessionDiagnosticsDialog.vue'
import { projectSessionDiagnostics } from '@/utils/sessionDiagnostics'
import { safeUserErrorCode } from '@/utils/userError'
import en from '@/i18n/locales/en'

let wrapper: VueWrapper | undefined
afterEach(() => { wrapper?.unmount(); document.body.innerHTML = '' })
it.each(['PROGRAM_TRUST_REQUIRED', 'PROGRAM_UNAVAILABLE', 'WORKING_DIRECTORY_UNAVAILABLE',
  'PROFILE_NOT_FOUND', 'PROFILE_CLI_MISMATCH', 'PROFILE_MISMATCH', 'REVISION_CONFLICT',
  'NATIVE_RUNTIME_NOT_READY', 'RUN_SUPERVISOR_STOPPING', 'FORBIDDEN', 'DOCUMENT_BRIDGE_UNAVAILABLE',
  'INVALID_REQUEST', 'LAUNCH_CANCEL_UNAVAILABLE'])('LaunchDiagnostics_PreservesSafeDenial_001: %s', code => {
  const errorCode = safeUserErrorCode({ code, field: '/private/path', message: 'SECRET' })
  const diagnostics = projectSessionDiagnostics({ id: 'row', adapterSessionId: 'tab', projectKey: '/repo', projectPath: '/repo',
    cli: 'codex', runtime: 'native-cli', title: 'PRIVATE_TITLE', processState: 'unknown', attentionState: 'none',
    lastActivityAt: 0, archived: false, resumable: false, safeErrorCode: errorCode }, true, 1)
  wrapper = mount(SessionDiagnosticsDialog, { attachTo: document.body, props: { diagnostics },
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
  expect(document.querySelector('[data-session-diagnostics]')?.textContent).toContain(code)
  expect(document.body.textContent).not.toMatch(/PRIVATE_TITLE|SECRET|private\/path/)
})
