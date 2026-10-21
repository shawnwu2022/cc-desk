# OpenCode 历史读取修复边界

基准为 0bcc8674749cbc145224389b1144a3eb19fcbf1a 加审查时冻结的用户工作区改动；上游为已安装 OpenCode 1.19.0 对应 commit 733b2c627e5aec39cb33cc22373bdce33053ef2c。后续集成只应应用修复增量，不把用户基线改动算作本次修复提交。

- F01：Windows 私有复制继续使用 capability、普通文件、侧车与 held-handle 检查。DB shared 锁阻止独占清理；SHM 的 CKPT 121 与 READ(1) 124 shared 锁在两次复制期间阻止 checkpoint 回写 DB 和 writer 重置 WAL。没有 SHM 时持有 DB 整段 512 字节独占锁，防止其他连接在复制期间创建或恢复 WAL。锁冲突返回 SOURCE_BUSY；允许 writer 追加，复制时文件长度/mtime 变化仍被拒绝。没有通过宽松 VFS 打开源路径，也没有使用 immutable。依据为 [SQLite Windows WAL 锁协议](https://sqlite.org/walformat.html#wal_locks) 与本机 bundled libsqlite3-sys 0.28.0 源码；非标准 VFS 锁协议不在支持范围。
- F02：非空 OPENCODE_DB 仅在指向已授权默认 opencode.db 时准入，接受数据根下的默认 basename 或精确绝对拼写。自定义文件、:memory: 与其他名称返回可选历史的 SCOPE_UNKNOWN，避免拿默认库冒充自定义库。CLI 仍继承原始环境。完整自定义文件 scope、安装 channel 的数据库发现仍未支持，不宣称可读取这些历史。
- F03：global 会话按已准入项目目录拼写在 SQL 中先过滤，再排序及 LIMIT。Windows 仍只做既有 ASCII 大小写折叠与斜杠归一，不打开或 canonicalize 转录中的路径。遗漏与截断均设置历史 Budget incomplete，即使没有有效行；catalog 返回前复查授权及协作 deadline。
- F04：NUL 身份/目录被隔离省略并标记历史不完整。NUL 标题降为空的 unknown/incomplete 标题，保留其他有效会话，不放宽前端 DTO decoder。普通标题截断也披露不完整。

回归使用合成 SQLite fixture，包含真实 Windows SQLite checkpoint、WAL reset、无 SHM 第二连接等行为。它们不替代完整安装版 Desk/WebView2/ConPTY 验收，也不代表旧版 CLI 或非 Windows 平台已经验证。
