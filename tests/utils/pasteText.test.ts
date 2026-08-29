import { describe, expect, it, vi } from 'vitest'
import { preparePasteText, bracketPasteText, buildPastePayload, compactJsonForPaste, isPasteStale, commitPaste, imagePasteBytes } from '@/utils/pasteText'

describe('compactJsonForPaste', () => {
  // 多行 pretty JSON 压缩成单行：结构性换行与缩进移除，token 不粘连。
  // 背景：Windows ConPTY 会吞掉 bracketed paste 标记，Claude Code 收到大段多行
  // 无标记 burst 时会间歇性丢弃头部或尾部（上游 #49673/#49337，不修）；
  // 单行 burst 稳定触发 chip 识别或完整进入编辑器（e2e 实测 3/3 安全）。
  it('PasteJson_PrettyMultiline_Compacted_001', () => {
    const pretty = '{\n  "a": 1,\n  "b": [\n    true,\n    null\n  ]\n}'
    expect(compactJsonForPaste(pretty)).toBe('{"a": 1,"b": [true,null]}')
  })

  // 压缩走正则而非 JSON.parse+stringify：字符串值内部的多空格原样保留。
  it('PasteJson_StringSpaces_Preserved_002', () => {
    const pretty = '{\n  "msg": "hello  world  ok",\n  "n": 2\n}'
    expect(compactJsonForPaste(pretty)).toBe('{"msg": "hello  world  ok","n": 2}')
  })

  // 大整数只存在于原文文本中，禁止 parse 往返（会丢精度），压缩后数字逐字不变。
  it('PasteJson_BigNumberDigits_Unchanged_003', () => {
    const pretty = '{\n  "id": 12345678901234567890,\n  "k": "v"\n}'
    const out = compactJsonForPaste(pretty)
    expect(out).toContain('12345678901234567890')
    expect(out).toBe('{"id": 12345678901234567890,"k": "v"}')
  })

  // CRLF 行尾的 pretty JSON 同样压缩。
  it('PasteJson_CrlfMultiline_Compacted_004', () => {
    const pretty = '{\r\n  "a": 1,\r\n  "b": 2\r\n}'
    expect(compactJsonForPaste(pretty)).toBe('{"a": 1,"b": 2}')
  })

  // 仅 CR 行尾的 pretty JSON 同样压缩（先于 LF 规范化，不能漏）。
  it('PasteJson_CrOnlyMultiline_Compacted_009', () => {
    const pretty = '{\r  "a": 1,\r  "b": 2\r}'
    expect(compactJsonForPaste(pretty)).toBe('{"a": 1,"b": 2}')
  })

  // 前导 BOM 不阻断校验：BOM 保留在压缩结果开头，JSON 主体被压缩。
  it('PasteJson_BomPrefix_CompactedKeepBom_010', () => {
    const pretty = '﻿{\n  "a": 1\n}'
    const out = compactJsonForPaste(pretty)
    expect(out.charCodeAt(0)).toBe(0xfeff)
    expect(out.slice(1)).toBe('{"a": 1}')
  })

  // 超过尺寸上限的文本跳过压缩（防 UI 线程同步解析大文本卡顿），原样返回。
  it('PasteJson_Oversize_SkipCompaction_011', () => {
    const big = '{\n  "a": "' + 'x'.repeat(2 * 1024 * 1024 + 1) + '"\n}'
    expect(compactJsonForPaste(big)).toBe(big)
  })

  // 非 JSON 文本原样返回（不得破坏多行 prose/代码）。
  it('PasteJson_NonJson_Unchanged_005', () => {
    const text = 'def foo():\n    return bar\n'
    expect(compactJsonForPaste(text)).toBe(text)
  })

  // 语法非法的 JSON 形状（尾逗号）原样返回，不做部分压缩。
  it('PasteJson_TrailingComma_Unchanged_006', () => {
    const text = '{\n  "a": 1,\n}'
    expect(compactJsonForPaste(text)).toBe(text)
  })

  // 已是单行的 JSON 原样返回（同一字符串，不做无谓替换）。
  it('PasteJson_SingleLine_Identity_007', () => {
    const text = '{"a": 1}'
    expect(compactJsonForPaste(text)).toBe(text)
  })

  // 空串原样返回。
  it('PasteJson_Empty_Identity_008', () => {
    expect(compactJsonForPaste('')).toBe('')
  })

  // 跨层桥接：DevTools 风格样本上，regex 压缩与 JSON.stringify 压缩语义等价
  // （解析结果 deep-equal）且同为单行。二者空白风格不同（regex 保留冒号后空格），
  // 故比较解析结果而非字节。Rust e2e 探针（paste_claude_e2e.rs）用 serde minify
  // 生成同形态 payload，其对 Claude 端行为的结论借此用例传导回生产 TS 管线。
  it('PasteJson_ProductionEquivalence_012', () => {
    const pairs: Array<[string, string]> = [
      ['"key_0"', '"AAAA"'],
      ['"key_1"', '"BBBB"'],
      ['"key_2"', '"CCCC"'],
    ]
    const pretty = '{\n  ' + pairs.map(([k, v]) => `${k}: ${v}`).join(',\n  ') + '\n}'
    const compacted = compactJsonForPaste(pretty)
    expect(compacted).not.toContain('\n')
    expect(JSON.parse(compacted)).toEqual(JSON.parse(JSON.stringify(JSON.parse(pretty))))
  })
})

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

  // buildPastePayload 集成 JSON 压缩：多行 pretty JSON 进，单行 + bracketed 包装出。
  it('PasteText_BuildPayload_JsonCompacted_013', () => {
    const pretty = '{\n  "a": 1,\n  "b": 2\n}'
    expect(buildPastePayload(pretty, true, false)).toBe('\x1b[200~{"a": 1,"b": 2}\x1b[201~')
  })

  // buildPastePayload 对非 JSON 多行文本保持原行为：仅 LF 规范化 + 包装。
  it('PasteText_BuildPayload_NonJsonUntouched_014', () => {
    expect(buildPastePayload('a\r\nb\r\nc', true, false))
      .toBe('\x1b[200~a\nb\nc\x1b[201~')
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
