import { relativizePath } from '@/utils/path'

// 引用文本用于 coding CLI 的文件路径输入；控制字符不能进入终端键盘通道。
const terminalControls = /[\p{Cc}\u2028\u2029]/u
const simpleReference = /^[\p{L}\p{N}_./:~-]+$/u

/** 整批保留路径身份；危险路径拒绝，不通过删字符生成另一个有效路径。 */
export function formatDroppedPaths(paths: readonly string[], projectPath: string): string | null {
  if (!paths.length || terminalControls.test(projectPath)) return null
  const references: string[] = []
  for (const path of paths) {
    if (!path || terminalControls.test(path)) return null
    const relative = relativizePath(path, projectPath)
    if (!relative || terminalControls.test(relative)) return null
    // 明确使用 JSON 字符串引用，保留 Unicode 并转义双引号与字面反斜杠。
    references.push(simpleReference.test(relative) ? relative : JSON.stringify(relative))
  }
  return references.join(' ')
}
