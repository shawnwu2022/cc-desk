import { describe, expect, it } from 'vitest'
import {
  buildSessionDragPayload,
  formatSessionReference,
  parseSessionDragPayload,
} from '@/utils/sessionReference'
import type { UnifiedSession } from '@/types/unifiedSession'

const base = {
  id: 'catalog-claude-1',
  projectKey: 'E:/repo/app',
  projectPath: 'E:/repo/app',
  cli: 'claude',
  runtime: 'native-cli',
  title: 'Fix login bug',
  processState: 'running',
  attentionState: 'none',
  lastActivityAt: 0,
  archived: false,
  resumable: true,
  adapterSessionId: 'adapter-1',
  nativeSessionId: 'native-1',
} as UnifiedSession

describe('Session reference drag payload', () => {
  // 载荷身份优先 native 会话 ID；无 native ID（legacy 行）回退 adapter ID。
  it('SessionRef_PayloadPrefersNativeIdentity_001', () => {
    const payload = JSON.parse(buildSessionDragPayload(base))
    expect(payload).toEqual({ title: 'Fix login bug', cli: 'claude', projectPath: 'E:/repo/app', sessionId: 'native-1' })
    const legacy = JSON.parse(buildSessionDragPayload({ ...base, nativeSessionId: null }))
    expect(legacy.sessionId).toBe('adapter-1')
  })
  // 字段缺失、类型不符或 CLI 未知时不猜测回退，返回 null 让调用方丢弃。
  it('SessionRef_ParseRejectsMalformedPayload_002', () => {
    expect(parseSessionDragPayload(null)).toBeNull()
    expect(parseSessionDragPayload('')).toBeNull()
    expect(parseSessionDragPayload('not json')).toBeNull()
    expect(parseSessionDragPayload('["native-1"]')).toBeNull()
    const valid = JSON.parse(buildSessionDragPayload(base))
    expect(parseSessionDragPayload(JSON.stringify({ ...valid, cli: 'shell' }))).toBeNull()
    expect(parseSessionDragPayload(JSON.stringify({ ...valid, projectPath: 1 }))).toBeNull()
    expect(parseSessionDragPayload(JSON.stringify(valid))).toEqual({
      title: 'Fix login bug', cli: 'claude', projectPath: 'E:/repo/app', sessionId: 'native-1',
    })
  })
  it('SessionRef_PayloadAcceptsOpencode_003', () => {
    const valid = JSON.parse(buildSessionDragPayload(base))
    expect(parseSessionDragPayload(JSON.stringify({ ...valid, cli: 'opencode' }))).not.toBeNull()
  })
})

describe('Session reference injection text', () => {
  // 合法载荷生成单行线索文本，包含标题、CLI、项目与会话身份。
  it('SessionRef_FormatsSingleLineClue_003', () => {
    const payload = parseSessionDragPayload(buildSessionDragPayload(base))!
    const reference = formatSessionReference(payload)
    expect(reference).toBe('Session: Fix login bug (cli: claude, project: E:/repo/app, id: native-1)')
    expect(reference).not.toMatch(/[\r\n]/)
  })

  // 控制字符标题转义注入（JSON 字符串保留完整信息），不产生原始换行或 ESC 字节。
  it('SessionRef_EscapesControlCharsInTitle_004', () => {
    const payload = parseSessionDragPayload(buildSessionDragPayload(base))!
    const reference = formatSessionReference({ ...payload, title: 'line1\nline2\x1b[0m' })
    expect(reference).toContain('line1\\nline2\\u001b[0m')
    expect(reference).not.toMatch(/[\r\n\x1b]/)
  })

  // 超长标题截断到 200 个码点，emoji 不得劈成孤立代理对。
  it('SessionRef_TruncatesLongTitleByCodePoint_005', () => {
    const payload = parseSessionDragPayload(buildSessionDragPayload(base))!
    const reference = formatSessionReference({ ...payload, title: 'x'.repeat(199) + '😀y' })
    expect(reference).toContain('😀')
    const titlePart = reference.slice('Session: '.length, reference.indexOf(' (cli:'))
    expect(Array.from(titlePart)).toHaveLength(200)
  })

  // 空标题回退为会话 ID，不产出空引用字段。
  it('SessionRef_FallsBackToSessionIdWhenTitleEmpty_006', () => {
    const payload = parseSessionDragPayload(buildSessionDragPayload(base))!
    expect(formatSessionReference({ ...payload, title: '' })).toBe('Session: native-1 (cli: claude, project: E:/repo/app, id: native-1)')
  })

  // JSON 转义规则遗漏 DEL 与 C1：它们必须显式展开为 \uXXXX 字面量，
  // 否则 sendUserText 会把裸 0x7F/0x85 字节发给 CLI（DEL 删除输入）。
  it('SessionRef_EscapesDelAndC1Controls_007', () => {
    const payload = parseSessionDragPayload(buildSessionDragPayload(base))!
    const reference = formatSessionReference({ ...payload, title: 'a\x7fb\x85c' })
    expect(reference).toContain('a\\u007fb\\u0085c')
    expect(reference).not.toMatch(/[\u007F-\u009F]/)
  })

  // 空标题回退的会话 ID 同样经过控制字符转义，不留裸字节通道。
  it('SessionRef_EscapesSessionIdFallback_008', () => {
    const payload = parseSessionDragPayload(buildSessionDragPayload(base))!
    const reference = formatSessionReference({ ...payload, title: '', sessionId: 'id\x7f' })
    expect(reference).toContain('id\\u007f')
    expect(reference).not.toMatch(/\u007F/)
  })

  // 标题、项目路径与空标题回退 ID 均覆盖完整 C0/DEL/C1 和 Unicode 行分隔符。
  it.each(['title', 'projectPath', 'sessionId'] as const)('SessionRef_AllFieldsRejectRawControls_009 %s', field => {
    const controls = Array.from({ length: 32 }, (_, index) => String.fromCharCode(index)).join('')
      + Array.from({ length: 33 }, (_, index) => String.fromCharCode(0x7f + index)).join('') + '\u2028\u2029'
    const payload = { title: field === 'sessionId' ? '' : 'Title', cli: 'claude' as const, projectPath: '/repo', sessionId: 'session-1', [field]: `before${controls}after` }
    const reference = formatSessionReference(payload)
    expect(reference).not.toMatch(/[\p{Cc}\u2028\u2029]/u)
    expect(reference).toContain('before')
    expect(reference).toContain('after')
  })
})
