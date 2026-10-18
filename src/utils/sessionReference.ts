/**
 * 会话引用拖拽：把会话列表行拖入终端后向 CLI 注入单行引用线索文本。
 * 载荷只含显示安全字段（标题/CLI/项目路径/会话 ID），不含 transcript 路径；
 * CLI 能否据此定位原始会话由其自身存储布局决定，GUI 不做路径承诺。
 *
 * 曾实验"读取源会话开头消息生成摘要块"，实测 14/14 真实会话全部
 * SOURCE_TOO_LARGE（Messages 全量读取受 2MiB 文件/响应双预算约束），
 * 已回退；将来若后端提供 tail/prefix 消息读取再恢复该档位。
 */

export const SESSION_DRAG_MIME = 'application/x-cc-desk-session'

import type { UnifiedCliKind } from '@/types/unifiedSession'

export interface SessionReferencePayload {
  title: string
  cli: UnifiedCliKind
  projectPath: string
  sessionId: string
}

// 与 formatDroppedPaths 同一通道契约：控制字符不能进入终端键盘通道。
const terminalControls = new RegExp('[\\p{Cc}\\u2028\\u2029]', 'u')
// JSON.stringify 按规范不转义 U+2028/U+2029，需先行替换。
const lineSeparators = new RegExp('[\\u2028\\u2029]', 'g')
// 标题来自可重命名字段与 CLI 元数据，长度无前端保证；超长单行 burst 有上游截断风险。
const MAX_TITLE_LENGTH = 200

export function buildSessionDragPayload(session: {
  title: string
  cli: UnifiedCliKind
  projectPath: string
  nativeSessionId?: string | null
  adapterSessionId: string
}): string {
  return JSON.stringify({
    title: session.title,
    cli: session.cli,
    projectPath: session.projectPath,
    sessionId: session.nativeSessionId || session.adapterSessionId,
  })
}

/** 解析 dataTransfer 载荷；任何字段不合法返回 null，不猜测回退。 */
export function parseSessionDragPayload(raw: unknown): SessionReferencePayload | null {
  if (typeof raw !== 'string' || !raw) return null
  let payload: unknown
  try {
    payload = JSON.parse(raw)
  } catch {
    return null
  }
  if (typeof payload !== 'object' || payload === null) return null
  const { title, cli, projectPath, sessionId } = payload as Record<string, unknown>
  if (typeof title !== 'string' || typeof projectPath !== 'string' || typeof sessionId !== 'string') return null
  if ((cli !== 'claude' && cli !== 'codex') || !sessionId) return null
  return { title, cli, projectPath, sessionId }
}

// 标题是显示标签而非路径身份，转义保留完整信息即可，不必像路径那样整批拒绝。
// JSON.stringify 不转义 U+2028/U+2029，先折叠行分隔符再判断。
function escapeReferenceField(value: string): string {
  const folded = value.replace(lineSeparators, ' ')
  return terminalControls.test(folded) ? JSON.stringify(folded) : folded
}

/** 生成注入文本：单行线索，包含标题、CLI、项目与会话身份。 */
export function formatSessionReference(payload: SessionReferencePayload): string {
  const displayTitle = payload.title
    ? escapeReferenceField(Array.from(payload.title).slice(0, MAX_TITLE_LENGTH).join(''))
    : payload.sessionId
  return `Session: ${displayTitle} (cli: ${payload.cli}, project: ${escapeReferenceField(payload.projectPath)}, id: ${escapeReferenceField(payload.sessionId)})`
}
