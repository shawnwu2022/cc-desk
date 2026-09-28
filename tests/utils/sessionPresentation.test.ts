import { describe, expect, test } from 'vitest'
import {
  deriveSessionVisualState,
  makeSessionCatalogKey,
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
