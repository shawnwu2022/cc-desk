import { afterEach, expect, it } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import SessionDiagnosticsDialog from '@/components/sessions/SessionDiagnosticsDialog.vue'
import { projectSessionDiagnostics } from '@/utils/sessionDiagnostics'
import { mapSafeUserError, safeUserErrorCode } from '@/utils/userError'
import { LaunchConfigurationRequiredError } from '@/utils/launchPreparation'
import en from '@/i18n/locales/en'
import zh from '@/i18n/locales/zh'

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

// 实际诊断投影到对话框必须显示暂停原因与 no-replay 说明，中英都不能只剩通用错误码。
it.each([
  ['en', 'Input is paused. Pending input will not be resent automatically. Check the session before deciding whether to restart it.'],
  ['zh', '输入已暂停。待处理输入不会自动重发。请先检查会话，再决定是否重启。'],
])('InputDiagnostics_RendersPaused_003 %s', (locale, message) => {
  const diagnostics = projectSessionDiagnostics({ id: 'row', adapterSessionId: 'tab', projectKey: '/private/path', projectPath: '/private/path',
    cli: 'codex', runtime: 'native-cli', title: 'PRIVATE_TITLE', processState: 'running', attentionState: 'none',
    lastActivityAt: 0, archived: false, resumable: false, safeErrorCode: 'NATIVE_INPUT_PAUSED' }, true, 2)
  wrapper = mount(SessionDiagnosticsDialog, { attachTo: document.body, props: { diagnostics },
    global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })] } })
  expect(document.querySelector('[data-session-diagnostics]')?.textContent).toContain(message)
  expect(document.querySelector('[role="status"]')?.textContent).toContain(message)
  expect(document.querySelector('[role="status"] button')).toBeNull()
  expect(document.body.textContent).toContain('NATIVE_INPUT_PAUSED')
  expect(document.body.textContent).not.toMatch(/GENERIC_UNAVAILABLE|PRIVATE_TITLE|private\/path/)
  expect(diagnostics.errorCode).toBe('NATIVE_INPUT_PAUSED')
  expect(mapSafeUserError('NATIVE_INPUT_PAUSED', 'session')).toMatchObject({
    messageKey: 'errorNativeInputPaused', severity: 'warning', retryable: false, actionKey: null,
  })
  expect(safeUserErrorCode({ code: 'NATIVE_INPUT_PAUSED', message: 'SECRET' })).toBe('NATIVE_INPUT_PAUSED')
})

// 类似暂停码的原始字符串仍被投影成通用码，不能展示 payload 或错误地触发暂停文案。
it.each(['en', 'zh'])('InputDiagnostics_RedactsUnknown_004 %s', locale => {
  const raw = 'NATIVE_INPUT_PAUSED token=SECRET /private/path'
  const diagnostics = projectSessionDiagnostics({ id: 'row', adapterSessionId: 'tab', projectKey: '/private/path', projectPath: '/private/path',
    cli: 'codex', runtime: 'native-cli', title: 'PRIVATE_TITLE', processState: 'running', attentionState: 'none',
    lastActivityAt: 0, archived: false, resumable: false, safeErrorCode: raw }, true, 2)
  wrapper = mount(SessionDiagnosticsDialog, { attachTo: document.body, props: { diagnostics },
    global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh } })] } })
  expect(diagnostics.errorCode).toBe('GENERIC_UNAVAILABLE')
  expect(document.body.textContent).toContain('GENERIC_UNAVAILABLE')
  expect(document.body.textContent).not.toMatch(/NATIVE_INPUT_PAUSED|SECRET|PRIVATE_TITLE|private\/path/)
  expect(document.querySelector('[role="status"]')).toBeNull()
  expect(safeUserErrorCode({ code: raw })).toBe('GENERIC_UNAVAILABLE')
})

// 其他诊断继续保持原有详情展示，不添加暂停提示或新的恢复操作。
it('InputDiagnostics_KeepsOtherCodes_005', () => {
  const diagnostics = projectSessionDiagnostics({ id: 'row', adapterSessionId: 'tab', projectKey: '/repo', projectPath: '/repo',
    cli: 'claude', runtime: 'native-cli', title: 'Session', processState: 'unknown', attentionState: 'none',
    lastActivityAt: 0, archived: false, resumable: false, safeErrorCode: 'NATIVE_STOP_UNCONFIRMED' }, true, 1)
  wrapper = mount(SessionDiagnosticsDialog, { attachTo: document.body, props: { diagnostics },
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
  expect(document.body.textContent).toContain('NATIVE_STOP_UNCONFIRMED')
  expect(document.querySelector('[role="status"]')).toBeNull()
})
