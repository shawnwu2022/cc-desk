import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const read = (path: string) => readFileSync(path, 'utf8')

describe('D26 native host security boundaries', () => {
  it('D26_DOM_NativeWorkbenchHasNoExecutableHtmlOrAutomaticExternalOpen_003', () => {
    const workbench = read('src/components/NativeCliWorkbench.vue')
    const terminal = read('src/components/NativeCliTerminal.vue')
    const combined = workbench + '\n' + terminal

    expect(combined).not.toContain('v-html')
    expect(combined).not.toContain('.innerHTML')
    expect(combined).not.toContain('.outerHTML')
    expect(combined).not.toContain('insertAdjacentHTML')
    expect(terminal).not.toContain('WebLinksAddon')
    expect(terminal).not.toContain('@tauri-apps/plugin-shell')
    expect(terminal).not.toContain('window.open(')
  })

  it('D26_IPC_NativeCommandsRequireDocumentBridgeAndHaveNoDirectInvokeFallback_004', () => {
    const api = read('src/api/tauri.ts')
    expect(api).toContain('DOCUMENT_BRIDGE_UNAVAILABLE')
    expect(api).toContain('nativeDocumentBridge().invoke')
    expect(api).not.toMatch(/(^|[^.\w])invoke(?:<[^>]+>)?\(['"]cli_/m)
  })

  it('D26_Observer_NativeEventsStayOwnerTargetedNotApplicationBroadcast_005', () => {
    const hook = read('src-tauri/src/hook_server.rs')
    expect(hook).toContain('emit_to(')
    expect(hook).not.toMatch(/\bapp\.emit\(/)
    expect(hook).toContain('native-observation')
  })

  it('D26_UI_NativeErrorsRemainCodesNotRawExceptionInterpolation_006', () => {
    const workbenchStore = read('src/stores/nativeWorkbench.ts')
    const workspaceStore = read('src/stores/cliWorkspace.ts')
    expect(workbenchStore).toContain('safeWorkbenchError')
    expect(workspaceStore).toContain('safeErrorCode')
    expect(workbenchStore).not.toContain('String(failure)')
    expect(workspaceStore).not.toContain('String(failure)')
  })

  it('D26_Observer_AuthenticatesBeforeBodyAllocationAndBoundsConcurrency_007', () => {
    const source = read('src-tauri/src/observer_http.rs')
    const auth = source.indexOf('check_binding(&binding)')
    const body = source.indexOf('to_bytes(request.into_body(), MAX_OBSERVER_PAYLOAD)')
    expect(source).toContain('try_acquire()')
    expect(source).toContain('MAX_OBSERVER_PAYLOAD')
    expect(source).toContain('accept_event(&binding, &event_id, &body)')
    expect(auth).toBeGreaterThan(-1)
    expect(body).toBeGreaterThan(auth)
  })

  it('D26_Logging_CommandNeverFormatsRawFrontendMessage_008', () => {
    const commands = read('src-tauri/src/commands.rs')
    const start = commands.indexOf('pub async fn log_message')
    const end = commands.indexOf('/// 获取当前应用可执行文件路径', start)
    const section = commands.slice(start, end)
    expect(section).toContain('frontend_message_summary(&message)')
    expect(section).not.toContain('{}", message')
    expect(section).not.toMatch(/log::(?:error|warn|info|debug)!\([^\n]*\bmessage\b/)
  })

})
