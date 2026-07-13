import { isMac, isWindows } from '@/utils/platform'

/**
 * 把绝对路径转换为相对于项目根目录的路径。
 *
 * 用于把拖入终端的文件路径换成相对路径，便于 Claude 直接引用项目内文件。
 * - 仅当文件确实在项目目录下时才返回相对路径，否则原样返回
 * - 相对路径统一使用 `/` 分隔，兼容 PowerShell / bash / zsh / git bash
 * - Windows 下盘符与目录名不区分大小写
 * - 拖入路径恰好就是项目根目录时返回 '.'
 */
export function relativizePath(filePath: string, projectPath: string): string {
  if (!projectPath || !filePath) return filePath

  // Windows 路径以盘符开头（C:\ / D:\ ...），比较时大小写不敏感
  const isWindows = /^[a-zA-Z]:[\\/]/.test(projectPath)
  const norm = (p: string) => p.replace(/\\/g, '/').replace(/\/+$/, '')
  const base = norm(projectPath)
  const target = norm(filePath)
  const eq = (a: string, b: string) =>
    isWindows ? a.toLowerCase() === b.toLowerCase() : a === b

  // 拖入的就是项目根目录本身
  if (eq(base, target)) return '.'

  // target 必须位于 base/ 之下才视为项目内文件
  if (
    target.length > base.length + 1 &&
    target[base.length] === '/' &&
    eq(target.slice(0, base.length), base)
  ) {
    return target.slice(base.length + 1)
  }

  return filePath
}

/**
 * 路径归一化（平台感知身份）：
 * - 斜杠规范（\ -> /）+ 去尾斜杠
 * - Windows / macOS（文件系统不区分大小写）：toLowerCase
 * - Linux（区分大小写）：不 lower，保留大小写身份
 * - 根路径边界：POSIX '/' 去尾斜杠后恢复 '/'；Windows drive 根 'C:\'/'C:'/'C:/' -> 'c:'
 *
 * 用于跨平台/跨重启匹配 pinned / archived / hidden / displayNames，
 * 消除各处内联 normalize 的不一致风险。平台身份读 utils/platform（模块级，
 * 测试用 vi.doMock('@/utils/platform') + 动态 import 注入）。
 */
export function normalizePath(p: string): string {
  let s = p.replace(/\\/g, '/')
  if (isWindows || isMac) s = s.toLowerCase()
  s = s.replace(/\/+$/, '')
  // POSIX 根 '/' 被去成空串 -> 恢复 '/'（保留根身份，避免空串 key）
  if (s === '' && p !== '') s = '/'
  return s
}

/**
 * 判断两个项目路径是否为同一项目（规范化后比较）。
 * 统一所有项目/tab 路径比较点，消除各处内联 === / normalizePath 比较的不一致风险
 * （Windows 大小写/斜杠差异、尾斜杠、跨重启路径漂移）。Linux 区分大小写。
 */
export function sameProjectPath(a: string, b: string): boolean {
  return normalizePath(a) === normalizePath(b)
}
