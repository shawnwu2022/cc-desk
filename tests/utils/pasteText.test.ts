import { describe, expect, it, vi } from 'vitest'
import { preparePasteText, bracketPasteText, buildPastePayload, isPasteStale, commitPaste } from '@/utils/pasteText'

describe('preparePasteText', () => {
  // CRLF 规范成 LF：Windows 剪贴板多行文本粘贴给 Claude 不再产生 \r 行首覆盖。
  it('PasteText_CrlfToLf_001', () => {
    expect(preparePasteText('a\r\nb\r\nc')).toBe('a\nb\nc')
  })

  // 单独 CR 也规范成 LF（xterm 会把 \r 当成回车，Claude 里会覆盖前一行）。
  it('PasteText_LoneCrToLf_002', () => {
    expect(preparePasteText('a\rb')).toBe('a\nb')
  })

  // 已是 LF 的文本保持不变（不引入无关改动）。
  it('PasteText_PureLfUnchanged_003', () => {
    expect(preparePasteText('a\nb\nc')).toBe('a\nb\nc')
  })

  // 换行之外的剪贴板内容原样保留，不擅自改写 ANSI 或控制字节。
  it('PasteText_AnsiContent_Preserved_004', () => {
    const text = 'status\x1b[31;1mred\x1b[0m\x1b]0;title\x07\x00end'
    expect(preparePasteText(text)).toBe(text)
  })

  // 空串原样返回。
  it('PasteText_EmptySafe_005', () => {
    expect(preparePasteText('')).toBe('')
  })

  // 纯空行（CRLF）规范成单个 LF。
  it('PasteText_BlankCrlfToLf_006', () => {
    expect(preparePasteText('\r\n')).toBe('\n')
  })

  // bracketed paste 开启时用 ESC[200~/ESC[201~ 包裹，多行 LF 作为单次粘贴提交。
  it('PasteText_BracketOn_Wraps_007', () => {
    expect(bracketPasteText('a\nb\nc', true, false)).toBe('\x1b[200~a\nb\nc\x1b[201~')
  })

  // bracketed paste 关闭时原样返回（不包装）。
  it('PasteText_BracketOff_Passthrough_008', () => {
    expect(bracketPasteText('a\nb', false, false)).toBe('a\nb')
  })

  // 开启但 ignoreBracketedPasteMode=true 时同样不包装（对齐 xterm 判据）。
  it('PasteText_BracketIgnored_NoWrap_009', () => {
    expect(bracketPasteText('a\nb', true, true)).toBe('a\nb')
  })

  // buildPastePayload 组合：规范化 LF + bracketed 包装，得到最终写 PTY 的字节。
  it('PasteText_BuildPayload_BracketOn_010', () => {
    expect(buildPastePayload('a\r\nb\r\nc', true, false))
      .toBe('\x1b[200~a\nb\nc\x1b[201~')
  })

  // buildPastePayload 关闭 bracketed：只规范化，不包装。
  it('PasteText_BuildPayload_BracketOff_011', () => {
    expect(buildPastePayload('a\r\nb', false, false)).toBe('a\nb')
  })

  // 空正文在 bracketed 包装前返回空串，调用方据此跳过 PTY 写入。
  it('PasteText_BuildPayload_Empty_012', () => {
    expect(buildPastePayload('', true, false)).toBe('')
  })
})

describe('isPasteStale', () => {
  // 捕获 ptyId 相同 = 仍是同一实例，粘贴有效。
  it('PasteStale_SamePty_NotStale_001', () => {
    expect(isPasteStale('pty-1', 'pty-1')).toBe(false)
  })

  // 按键瞬间无实例（ptyId 未捕获）→ 过期，丢弃。
  it('PasteStale_NoCaptured_Stale_002', () => {
    expect(isPasteStale(undefined, 'pty-1')).toBe(true)
  })

  // readText 等待期间 restartTab 重建新 PTY：当前 ptyId 与捕获值不同 → 过期，丢弃。
  // 锁死"旧终端的粘贴不落到同 tabId 的新实例"竞态语义。
  it('PasteStale_RestartRecreated_Stale_003', () => {
    expect(isPasteStale('pty-1', 'pty-2')).toBe(true)
  })

  // 完成后实例已销毁（当前 ptyId 为 undefined）→ 过期，丢弃。
  it('PasteStale_InstanceGone_Stale_004', () => {
    expect(isPasteStale('pty-1', undefined)).toBe(true)
  })
})

describe('commitPaste', () => {
  // 行为级竞态测试：readText 等待期间实例被 restart 重建（ptyId 变化），
  // 最终不得把旧粘贴写入新 PTY。
  it('CommitPaste_RestartRead_NoWrite_001', async () => {
    let resolveRead: (t: string) => void = () => {}
    const readTextMock = () => new Promise<string>(res => { resolveRead = res })
    let current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const getInstance = () => current
    const write = vi.fn()
    const p = commitPaste(readTextMock, getInstance, t => t, write)
    current = { ptyId: 'pty-2' } // restartTab 重建同 tabId 的新 PTY
    resolveRead('hello')
    await p
    expect(write).not.toHaveBeenCalled()
  })

  // 实例未变：正常构造 payload 并写入同一 ptyId。
  it('CommitPaste_SameInstance_Writes_002', async () => {
    const current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const getInstance = () => current
    const write = vi.fn()
    await commitPaste(async () => 'a\r\nb', getInstance, t => t, write)
    expect(write).toHaveBeenCalledTimes(1)
    expect(write).toHaveBeenCalledWith('pty-1', 'a\r\nb')
  })

  // 空正文：不写入、不产生空 bracketed 标记。
  it('CommitPaste_EmptyText_NoWrite_003', async () => {
    const current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const write = vi.fn()
    await commitPaste(async () => '', () => current, t => t, write)
    expect(write).not.toHaveBeenCalled()
  })

  // 构造的 payload 为空（规范性函数返回空串）：跳过发送。
  it('CommitPaste_EmptyPayload_NoWrite_004', async () => {
    const current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const write = vi.fn()
    await commitPaste(async () => 'x', () => current, () => '', write)
    expect(write).not.toHaveBeenCalled()
  })

  // 完成后实例已销毁：丢弃，不写 PTY。
  it('CommitPaste_InstanceGone_NoWrite_005', async () => {
    let current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const getInstance = () => current
    const write = vi.fn()
    const p = commitPaste(async () => 'x', getInstance, t => t, write)
    current = undefined
    await p
    expect(write).not.toHaveBeenCalled()
  })
})
