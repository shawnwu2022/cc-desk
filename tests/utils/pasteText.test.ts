import { describe, expect, it, vi } from 'vitest'
import { preparePasteText, bracketPasteText, buildPastePayload, isPasteStale, commitPaste, imagePasteBytes } from '@/utils/pasteText'

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

  // ===== 剪贴板图片分流（剪贴板无文本时转发 CLI 图片粘贴键字节） =====

  // 非空文本优先：走 buildPayload，注入的 fallback 完全不参与。
  it('ClipboardImage_NonEmptyStillText_002', async () => {
    const current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const write = vi.fn()
    const fallback = vi.fn(() => '\x1bv')
    await commitPaste(async () => 'a\r\nb', () => current, t => t, write, fallback)
    expect(fallback).not.toHaveBeenCalled()
    expect(write).toHaveBeenCalledWith('pty-1', 'a\r\nb')
  })

  // resolve 空串 + 注入 fallback → 写入键字节，无 bracketed 标记。
  it('ClipboardImage_EmptyTextFallback_004', async () => {
    const current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const write = vi.fn()
    await commitPaste(async () => '', () => current, t => t, write, () => '\x1bv')
    expect(write).toHaveBeenCalledTimes(1)
    expect(write).toHaveBeenCalledWith('pty-1', '\x1bv')
  })

  // resolve 空串 + 等待期间 ptyId 变更 → 过期丢弃（restart 竞态对分流路径同样生效）。
  it('ClipboardImage_FallbackStale_006', async () => {
    let resolveRead: (t: string) => void = () => {}
    const readTextMock = () => new Promise<string>(res => { resolveRead = res })
    let current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const write = vi.fn()
    const p = commitPaste(readTextMock, () => current, t => t, write, () => '\x1bv')
    current = { ptyId: 'pty-2' }
    resolveRead('')
    await p
    expect(write).not.toHaveBeenCalled()
  })

  // resolve 空串 + fallback 返回空串 → 不写。
  it('ClipboardImage_EmptyFallbackNoWrite_008', async () => {
    const current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const write = vi.fn()
    await commitPaste(async () => '', () => current, t => t, write, () => '')
    expect(write).not.toHaveBeenCalled()
  })

  // 主场景：截图剪贴板使 readText reject，注入 fallback 后仍写入键字节。
  it('ClipboardImage_RejectFallback_010', async () => {
    const current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const write = vi.fn()
    await commitPaste(
      async () => { throw new Error('ContentNotAvailable') },
      () => current, t => t, write, () => '\x1bv',
    )
    expect(write).toHaveBeenCalledTimes(1)
    expect(write).toHaveBeenCalledWith('pty-1', '\x1bv')
  })

  // reject + 未注入 fallback → 原异常向上传播（handler .catch 吞掉），不写。
  it('ClipboardImage_RejectNoFallback_012', async () => {
    const current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const write = vi.fn()
    const boom = new Error('ContentNotAvailable')
    await expect(
      commitPaste(async () => { throw boom }, () => current, t => t, write),
    ).rejects.toThrow(boom)
    expect(write).not.toHaveBeenCalled()
  })

  // reject + 等待期间 ptyId 变更 → 过期丢弃（reject 路径同样进 stale 复核）。
  it('ClipboardImage_RejectStale_014', async () => {
    let rejectRead: (e: unknown) => void = () => {}
    const readTextMock = () => new Promise<string>((_, rej) => { rejectRead = rej })
    let current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const write = vi.fn()
    const p = commitPaste(readTextMock, () => current, t => t, write, () => '\x1bv')
    current = { ptyId: 'pty-2' }
    rejectRead(new Error('ContentNotAvailable'))
    await p
    expect(write).not.toHaveBeenCalled()
  })

  // reject + fallback 返回空串 → 传播原异常、不写（与 resolve 空串的静默跳过区分）。
  it('ClipboardImage_RejectEmptyFallback_016', async () => {
    const current: { ptyId: string } | undefined = { ptyId: 'pty-1' }
    const write = vi.fn()
    const boom = new Error('ContentNotAvailable')
    await expect(
      commitPaste(async () => { throw boom }, () => current, t => t, write, () => ''),
    ).rejects.toThrow(boom)
    expect(write).not.toHaveBeenCalled()
  })
})

describe('imagePasteBytes', () => {
  // 平台键位字节（chat:imagePaste 官方默认）：仅 Windows/WSL 绑 Alt+V（\x1bv），
  // 其余平台默认 Ctrl+V（\x16）；unknown 按官方默认兜底。
  it('ClipboardImage_FallbackBytes_001', () => {
    expect(imagePasteBytes('windows')).toBe('\x1bv')
    expect(imagePasteBytes('macos')).toBe('\x16')
    expect(imagePasteBytes('linux')).toBe('\x16')
    expect(imagePasteBytes('unknown')).toBe('\x16')
  })
})
