// Effect 4.1.0 默认请求扩展能力后再关闭；Node 权限模式拒绝该请求。
// 专用归档不使用扩展。只在构建时替换此 helper 的 SQLite import，不改上游或全局运行时。
import { DatabaseSync as NativeDatabase } from 'node:sqlite'
export class DatabaseSync extends NativeDatabase {
  constructor(filename, options = {}) { super(filename, { ...options, allowExtension: false }) }
  enableLoadExtension(allow) {
    if (allow) throw new Error('SQLITE_EXTENSION_FORBIDDEN')
    return super.enableLoadExtension(false)
  }
  loadExtension() { throw new Error('SQLITE_EXTENSION_FORBIDDEN') }
}
// 归档路径不做备份；保留驱动模块的命名导入，但拒绝误用。
export function backup() { throw new Error('SQLITE_BACKUP_FORBIDDEN') }