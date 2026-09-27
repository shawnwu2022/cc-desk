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
})
