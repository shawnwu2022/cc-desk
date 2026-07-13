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
