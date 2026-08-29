/**
 * 粘贴文本预处理（供 Ctrl+V 粘贴进 Claude CLI 前调用）。
 *
 * 背景：xterm 的 `Terminal.paste` 会把 `\r?\n` 统一转成 `\r`（回车），这是给传统
 * 终端用的（回车即换行）。但 Claude CLI 是 Ink 全屏 TUI，期望标准 LF（`\n`）换行。
 * 直接把含 `\r` 的文本发给它，会触发光标回行首、后续内容覆盖前面——表现为
 * "长文本粘贴只显示尾部 / 出现隐藏字符 / 删除时先删不可见内容光标不动"。
 *
 * 仅把 CRLF / 单独 CR 规范成 LF（Claude 期望的行尾）。其他剪贴板内容原样保留，
 * 避免终端层擅自改写用户输入。
 */
import type { Platform } from './platform'

export function preparePasteText(text: string): string {
  return text.replace(/\r\n?/g, '\n')
}

/**
 * 生成最终写入 PTY 的粘贴 payload：规范化正文 + 按 bracketed paste 模式包装。
 *
 * Claude CLI 开启 bracketed paste 时用 `ESC[200~…ESC[201~` 包裹多行文本，使其作为
 * 单次粘贴提交（而非逐行触发 Enter）。调用方应传入 `term.modes.bracketedPasteMode`
 * （与 xterm 的 paste 判据一致：bracketedPasteMode 且未忽略）。纯函数，便于测试。
 */
export function bracketPasteText(
  text: string,
  bracketedPasteMode: boolean,
  ignoreBracketedPasteMode: boolean,
): string {
  if (bracketedPasteMode && !ignoreBracketedPasteMode) {
    return '\x1b[200~' + text + '\x1b[201~'
  }
  return text
}

/**
 * 构造最终写入 PTY 的粘贴 payload：规范化 LF 换行 + 按 bracketed paste 模式包装。
 * 组件粘贴链路唯一入口，测试据此锁死"剪贴板文本 → 最终写 PTY 字节"的完整行为。
 */
export function buildPastePayload(
  text: string,
  bracketedPasteMode: boolean,
  ignoreBracketedPasteMode: boolean,
): string {
  const prepared = preparePasteText(text)
  // 空正文不包装，让调用方跳过发送，避免空 bracketed 标记。
  if (!prepared) return ''
  return bracketPasteText(prepared, bracketedPasteMode, ignoreBracketedPasteMode)
}

/**
 * 判断一次粘贴是否已过期（应丢弃）：`readText()` 是异步的，等待期间终端可能被
 * restart/recreate 重建出同 tabId 的新 PTY。按键瞬间捕获的 ptyId 与完成后当前实例的
 * ptyId 不一致，说明旧终端发起的粘贴会落到新实例上，须丢弃。首参为捕获值，次参为当前值。
 */
export function isPasteStale(capturedPtyId: string | undefined, currentPtyId: string | undefined): boolean {
  return !capturedPtyId || currentPtyId !== capturedPtyId
}

type PasteInstance = { ptyId: string }

/**
 * 一次粘贴的完整异步流程：同步读取键按瞬间的实例 → 异步 readText → 复核当前实例仍是
 * 同一 ptyId（否则视为过期丢弃）→ 构造 payload → 写 PTY。依赖（readText / 取实例 /
 * 构造 payload / 写 PTY）全部注入，便于在纯函数层锁定"重启重建后不写到新 PTY"的竞态行为。
 */
export async function commitPaste(
  readTextAsync: () => Promise<string>,
  getInstance: () => PasteInstance | undefined,
  buildPayload: (text: string) => string,
  write: (ptyId: string, payload: string) => Promise<unknown>,
): Promise<void> {
  const capturedPtyId = getInstance()?.ptyId
  const text = await readTextAsync()
  if (!text) return
  const instance = getInstance()
  if (instance?.ptyId && !isPasteStale(capturedPtyId, instance.ptyId)) {
    const payload = buildPayload(text)
    if (!payload) return // 正文为空：跳过发送，避免产生空 bracketed-paste 标记
    await write(instance.ptyId, payload)
  }
}

/**
 * 平台对应的 CLI 图片粘贴键字节(chat:imagePaste 官方默认键位):
 * Windows/WSL 为 Alt+V(\x1bv),其余平台默认 Ctrl+V(\x16)。
 * 这是按键序列不是粘贴文本,调用方不得再包 bracketed paste 标记。
 */
export function imagePasteBytes(platform: Platform): string {
  return platform === 'windows' ? '\x1bv' : '\x16'
}
