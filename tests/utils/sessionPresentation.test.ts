import { describe, expect, test } from 'vitest'
import {
  deriveSessionVisualState,
  makeSessionCatalogKey,
  selectSessionMenuActions,
  selectSessionPrimaryAction,
} from '@/utils/sessionPresentation'
import type { UnifiedSession } from '@/types/unifiedSession'

const base: UnifiedSession = {
  id: 'native:run-1',
  projectKey: 'd:/work/game',
  projectPath: 'D:\\Work\\Game\\',
  cli: 'codex',
  runtime: 'native-cli',
  title: 'Fix login',
  processState: 'running',
  attentionState: 'none',
  lastActivityAt: 1,
  archived: false,
  resumable: true,
  adapterSessionId: 'run-1',
  nativeSessionId: 'session-1',
  renameState: 'idle',
}

describe('session presentation', () => {
  test('needs-user overrides a running visual state', () => {
    expect(deriveSessionVisualState({ ...base, attentionState: 'needs-user' })).toBe('needs-user')
  })

  test('selects at most one state-owned primary action', () => {
    expect(selectSessionPrimaryAction({ ...base, processState: 'failed' })).toBe('retry')
    expect(selectSessionPrimaryAction({ ...base, attentionState: 'needs-user' })).toBeNull()
    expect(selectSessionPrimaryAction({ ...base, archived: true })).toBe('restore-archive')
    expect(selectSessionPrimaryAction({ ...base, renameState: 'editing' })).toBe('save-rename')
  })

  // 没有来源改名能力时隐藏菜单；归档仍走 Desk 注册表，互不借用授权。
  test('SessionMenu_OpencodeRenameHidden_001', () => {
    const actions = selectSessionMenuActions({ ...base, cli: 'opencode', processState: 'failed' })
    expect(actions.some(action => action.id === 'rename')).toBe(false)
    expect(actions.some(action => action.id === 'archive')).toBe(true)
  })

  test('SessionMenu_OpencodeUsesVerifiedRenameCapability_002', () => {
    expect(selectSessionMenuActions({ ...base, cli: 'opencode', renameAvailable: true }).some(action => action.id === 'rename')).toBe(true)
    expect(selectSessionMenuActions({ ...base, cli: 'opencode', renameAvailable: true, archived: true }).some(action => action.id === 'rename')).toBe(false)
  })

  test('catalog identity contains runtime, cli, normalized project and adapter identities', () => {
    const windows = makeSessionCatalogKey({
      runtime: 'native-cli',
      cli: 'codex',
      projectPath: 'D:\\Work\\Game\\',
      adapterSessionId: 'run-1',
      nativeSessionId: 'session-1',
    })
    const normalized = makeSessionCatalogKey({
      runtime: 'native-cli',
      cli: 'codex',
      projectPath: 'd:/work/game',
      adapterSessionId: 'run-1',
      nativeSessionId: 'session-1',
    })
    expect(windows).toBe(normalized)
    expect(windows).toContain('native-cli')
    expect(windows).toContain('codex')
  })

  test('catalog identity rejects empty and NUL-bearing identity fields', () => {
    expect(() => makeSessionCatalogKey({
      runtime: 'legacy-claude',
      cli: 'claude',
      projectPath: '',
      adapterSessionId: 'tab-1',
    })).toThrow(/PROJECT_PATH_REQUIRED/)
    expect(() => makeSessionCatalogKey({
      runtime: 'legacy-claude',
      cli: 'claude',
      projectPath: '/tmp/work',
      adapterSessionId: 'bad\0id',
    })).toThrow(/ADAPTER_SESSION_ID_INVALID/)
  })
})
