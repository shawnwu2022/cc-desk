/** 别名校验结果 */
export type ValidationResult =
  | { ok: true }
  | { ok: false; error: 'tooLong' | 'invalid' }

/** 别名最大长度（含多字节字符按 UTF-16 code unit 计，与 input maxlength 一致） */
export const DISPLAY_NAME_MAX = 32

/** 控制字符集（\p{Cc}：含 \r \n \t \x00 等所有 Unicode 控制字符） */
const CONTROL_CHAR = /\p{Cc}/u

/**
 * 校验显示别名（store + 两编辑入口共用）：
 * - tooLong：长度 > 32
 * - invalid：含控制字符（\p{Cc}）
 * - 空 / 纯空白：合法（=清除别名语义，由调用方据 trim 决定是否删 key）
 * 返回 {ok:true} 表示可接受（含空），{ok:false,error} 表示拒绝。
 */
export function validateDisplayName(raw: string): ValidationResult {
  if (raw.length > DISPLAY_NAME_MAX) return { ok: false, error: 'tooLong' }
  if (CONTROL_CHAR.test(raw)) return { ok: false, error: 'invalid' }
  return { ok: true }
}

/**
 * 取项目 basename（去尾斜杠 + 反斜杠规范后 split 末段）。
 * 用于 getDisplayName 回退 + matchProjectQuery 的 basename 参数 + archivedProjects 名称。
 * 根路径（POSIX '/'）末段为空 -> 回退原路径。
 */
export function projectBasename(projectPath: string): string {
  const parts = projectPath.replace(/\\/g, '/').replace(/\/+$/, '').split('/')
  return parts[parts.length - 1] || projectPath
}

/**
 * 解析 native window title（纯函数，便于测）：
 * cwd 空 -> 'CC Desk'；否则用 resolveName(cwd)（调用方传 getDisplayName，含别名/basename 回退）。
 */
export function resolveWindowTitle(
  cwd: string | null | undefined,
  resolveName: (p: string) => string,
): string {
  if (!cwd) return 'CC Desk'
  return resolveName(cwd)
}

/**
 * 项目搜索匹配（纯函数，三字段）：displayName + basename + path 任一小写包含 query。
 * - displayName：别名（或无别名时的 basename）
 * - basename：原路径末段（别名设置时仍可搜原名命中）
 * - path：完整路径
 * 大小写不敏感（query 与三字段统一 toLowerCase 比较）；调用方通常会 trim+lower，
 * 函数内再 lower 一次作防御，保证无论调用方是否预处理都大小写不敏感。
 */
export function matchProjectQuery(displayName: string, basename: string, path: string, query: string): boolean {
  const ql = query.toLowerCase()
  return (
    displayName.toLowerCase().includes(ql) ||
    basename.toLowerCase().includes(ql) ||
    path.toLowerCase().includes(ql)
  )
}

/** 编辑状态机：idle（未编辑）/ editing（输入中）/ submitting（提交中）/ error（失败，保留 input） */
export type EditState = 'idle' | 'editing' | 'submitting' | 'error'
export type EditAction =
  | { type: 'start' }    // 进入编辑
  | { type: 'submit' }   // 提交（editing/error -> submitting；submitting/idle 忽略防重复）
  | { type: 'success' }  // 提交成功（submitting -> idle，关 input）
  | { type: 'fail' }     // 提交失败（submitting -> error，保留 input + 错误提示）
  | { type: 'cancel' }   // 取消（-> idle，不改）

/**
 * 编辑状态机 reducer（纯函数，便于自动测）。
 * 关键不变量：
 * - 成功才关 input（success -> idle）；失败保留 input（fail -> error）
 * - 防重复提交：submit 仅 editing/error 态生效（submitting 态忽略；error 态可重试 retry）
 * - 校验失败：调用方先 submit(->submitting) 再 fail(->error)，error 态显示 input + 错误
 */
export function editReducer(state: EditState, action: EditAction): EditState {
  switch (action.type) {
    case 'start': return state === 'submitting' ? state : 'editing'
    case 'submit': return state === 'editing' || state === 'error' ? 'submitting' : state
    case 'success': return state === 'submitting' ? 'idle' : state
    case 'fail': return state === 'submitting' ? 'error' : state
    case 'cancel': return 'idle'
  }
}
