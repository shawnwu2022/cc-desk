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
