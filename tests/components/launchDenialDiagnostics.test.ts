import { afterEach, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import SessionDiagnosticsDialog from '@/components/sessions/SessionDiagnosticsDialog.vue'
import { projectSessionDiagnostics } from '@/utils/sessionDiagnostics'
import { safeUserErrorCode } from '@/utils/userError'
import { LaunchConfigurationRequiredError } from '@/utils/launchPreparation'
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

it.each(['RUNNER_UNAVAILABLE', 'ENV_SOURCE_MISSING', 'LEGACY_INVALID', 'LEGACY_READ_FAILED', 'LEGACY_TOO_LARGE', 'PRIVATE_UNKNOWN_CODE'])('LaunchDiagnostics_PreflightCodeIsAllowlisted_002: %s', code => {
  const failure = new LaunchConfigurationRequiredError('private-profile', { code, field: '/private/path', message: 'SECRET' })
  const expected = code === 'PRIVATE_UNKNOWN_CODE' ? 'GENERIC_UNAVAILABLE' : code
  expect(failure.issueCode).toBe(expected)
  const diagnostics = projectSessionDiagnostics({ id: 'row', adapterSessionId: 'tab', projectKey: '/repo', projectPath: '/repo',
    cli: 'codex', runtime: 'native-cli', title: 'PRIVATE_TITLE', processState: 'failed', attentionState: 'none',
    lastActivityAt: 0, archived: false, resumable: false, safeErrorCode: failure.message, preparationIssueCode: failure.issueCode }, false)
  expect(diagnostics.errorCode).toBe(expected)
  expect(JSON.stringify(diagnostics)).not.toMatch(/PRIVATE|SECRET|private/)
})
