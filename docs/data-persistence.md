# 数据持久化

## 数据保存原则

1. **原生数据只读** — Claude Code 原生配置只读取不修改
2. **应用配置独立** — GUI 特有设置保存在 `~/.cc-box/`
3. **不重复存储权威数据** — 项目列表和消息正文直接从原生数据读取；会话名称索引仅保存可删除重建的派生值
4. **默认值持久化** — 用户偏好设置保存在应用配置

## 文件路径

### Claude Code 原生文件（只读，Provider 激活时写入）

| 文件 | 用途 |
|------|------|
| `~/.claude.json` | 项目列表、用户偏好、会话信息 |
| `~/.claude/settings.json` | 全局配置（MCP、权限、模型）—— **Provider 激活时完整替换写入** |
| `~/.claude/projects/<encoded-path>/` | 项目会话数据 |
| `<project>/.claude/settings.json` | 项目配置 |

### 应用专属文件（读写）

| 文件 | 用途 |
|------|------|
| `~/.cc-box/config.json` | GUI 配置（路径缓存、主题、字号、启动参数默认值） |
| `~/.cc-box/providers.json` | **Provider 配置**（列表 + 通用配置 + 激活状态） |
| `~/.cc-box/projects.json` | 项目置顶 + 会话存档 + 项目别名（displayNames） |
| `~/.cc-box/session-name-index.json` | 会话名称派生索引（schema/parser 版本均为 1） |
| `~/.cc-box/session-name-index.json.lock` | 名称索引跨进程永久锁文件 |
| `~/.cc-box/claude-plugin/` | Hook Plugin 文件（运行时生成） |
| `~/.cc-box/logs/` | 日志文件 |

## ~/.cc-box/config.json 结构

```json
{
  "claudePath": "C:\\Users\\xxx\\.local\\bin\\claude.exe",
  "claudeLauncherType": "direct",
  "gitBashPath": "C:\\Program Files\\Git\\bin\\bash.exe",
  "defaultSkipPermissions": false,
  "defaultCustomArgs": "",
  "theme": "light",
  "fontSize": 12,
  "webglRenderer": false,
  "lastOpenedProject": "D:/projects/my-app"
}
```

字段说明：

| 字段 | 类型 | 说明 |
|------|------|------|
| `claudePath` | string? | Claude CLI 路径（检测后缓存） |
| `claudeLauncherType` | "direct" \| "node"? | 启动类型（检测后缓存） |
| `gitBashPath` | string? | Git Bash 路径（Windows，检测后缓存） |
| `defaultSkipPermissions` | boolean | `--dangerously-skip-permissions` 默认值 |
| `defaultCustomArgs` | string | 自定义参数默认值 |
| `theme` | string | GUI 主题 |
| `fontSize` | number | 终端字号 |
| `webglRenderer` | boolean | 终端渲染后端：`false`=DOM（默认，稳定）/`true`=WebGL（高性能，CJK glyph atlas 可能留白/错位）。仅对新开终端生效 |
| `lastOpenedProject` | string? | 上次打开的项目路径 |

## ~/.cc-box/projects.json 结构

```json
{
  "pinnedProjects": ["/path/to/proj"],
  "archivedSessions": { "/path/to/proj": ["sessionId1"] },
  "displayNames": { "/normalized/path": "别名" }
}
```

字段说明：

| 字段 | 类型 | 说明 |
|------|------|------|
| `pinnedProjects` | string[] | 置顶项目路径列表（排序时置顶优先） |
| `archivedSessions` | Record<string, string[]> | 项目路径 -> 已存档 sessionId 列表 |
| `displayNames` | Record<string, string> | normalizedPath -> 项目别名（空/缺省 = 回退 basename） |

- **key 规范化**：`displayNames` 的 key 为 `normalizePath` 后的路径（Windows/macOS 大小写不敏感 lower，Linux 保留大小写；去尾斜杠）。设置别名时删等价旧 key（避免 `E:\Repo` / `e:/repo` 双份）。
- **原子写**：apply 增量命令（pin/unpin/archive/restore/setDisplayName）在 `with_projects_state_locked` 锁内读最新 → canonicalize → 校验应用 → `write_json_atomic`（完整写入并 `sync_all` `.json.tmp` 后替换）。POSIX 使用覆盖语义的 `rename`；Windows 已有目标使用 `ReplaceFileW`（忽略 ACL 合并错误 + write-through），首次创建使用 rename；替换失败保留原文件。
- **多实例并发安全**：写走后端独立 `projects.json.lock`（std `File::lock`）跨进程排他锁（写排他 / 读共享，有界超时；持锁进程被杀由 OS 自动释放），async command 在 `spawn_blocking` 内完成锁定 IO，apply 增量操作在锁内原子读改写，不再依赖前端完整快照覆盖。前端 `session.ts` 的 `opLock` 串行完整 action/reload request + apply；窗口聚焦 reload 共享锁读。config.json 的 hiddenProjects/lastOpened 暂未纳入（同 pattern 可扩展）；升级时须先关闭所有旧版本实例。

## 会话数据读取

- `get_home_data` 只枚举一次 `~/.claude/projects`，同一快照同时生成项目条目和 `realPath -> encoded directories` 映射；近期会话从显式目录列表读取，不再触发第二次项目路径全扫。同一真实路径有多个编码目录时仍保留原有聚合与展示语义。
- 项目路径解析按字节行读取 JSONL/TXT，在首个有效 `cwd` 后停止；损坏的 UTF-8 或 JSON 行被跳过。会话名称继续扫描到 EOF，保证末尾 `custom-title` 覆盖首条用户消息，峰值内存为 O(最大 JSONL 单行)。
- `get_home_data`、`get_sessions`、`get_all_recent_sessions` command 通过 Tokio `spawn_blocking` 执行同步文件 IO，避免阻塞 Tauri async worker。三条路径每请求只读取一次名称索引快照；home/all-recent 跨项目共享同一 resolver，仍先枚举 metadata、排序/分页，再只解析命中页。

### 会话名称派生索引

- 一级 key 是规范化后的 Claude 编码项目目录绝对路径，二级 key 是含扩展名的会话文件名；同一真实 cwd 的新旧编码目录互不覆盖。
- 条目只保存名称、`observedLength`、mtime 秒/纳秒和 `cachedAtMs`，不保存消息正文。schema/parser 版本当前均为 1；名称优先级、过滤或截断语义变化时必须递增 parser 版本，旧版本整份按空索引处理。
- exact-hit 要求长度、mtime 秒和纳秒完全相等，读取 0 JSONL bytes；append、truncate 或同长度 mtime 变化都扫描到 EOF full rebuild。扫描前后 stamp 不稳定时仍返回本次名称，但不生成 replacement。
- command 先返回业务值，再把至多一个 `PendingIndexFlush` 放入 detached blocking job。后台在索引锁外复核 JSONL stamp、解析/合并/压缩/序列化/写临时文件/`sync_all`；排他锁内只以 64 KiB 缓冲比较 raw base 并原子替换。
- 多实例通过 entry CAS、完整 bucket 清理 CAS、最多四次 whole-file raw CAS 收敛。写失败和同一损坏指纹在进程内退避 30 秒；这些失败只降低命中率，不改变成功的 IPC 返回。
- 8 MiB 以上确定性批量淘汰旧条目直到不超过 6 MiB；16 MiB 为读取硬上限。索引可随时删除，下次访问从 Claude JSONL 自动重建；Claude JSONL 始终是权威来源。

## Store 命令 (IPC 通道)

| 命令 | 说明 |
|------|------|
| `get_home_data` | 单次项目扫描获取项目列表 + 近期会话；单 resolver + response-first 索引写回 |
| `get_projects` | 项目列表（分页） |
| `get_project_info` | 项目详情 |
| `get_sessions` | 会话列表（分页）；复用路径映射和名称索引，阻塞 IO 在 `spawn_blocking` 中执行 |
| `get_session_count` | 会话总数 |
| `get_all_recent_sessions` | 跨项目近期会话；所有项目共享一次索引快照 |
| `get_session_details` | 会话详情 |
| `search_session_messages` | 搜索会话消息内容 |
| `get_app_config` | 获取应用配置 |
| `update_app_config` | 更新应用配置（合并更新） |
| `get_default_claude_options` | 获取默认启动选项 |
| `save_default_claude_options` | 保存默认启动选项 |
| `save_last_project` | 保存上次项目 |
| `get_project_config` | 获取项目 Claude 配置（只读） |
| `get_all_agents` | 获取所有 Agents |
| `get_all_skills` | 获取所有 Skills |
| `get_all_mcp_servers` | 获取所有 MCP Servers |
| `get_all_plugins` | 获取所有 Plugins |
| `get_mcp_server_detail` | 获取 MCP Server 详情（通过协议） |

## 兼容性

用户可随时回到 CLI：

- 项目列表由 Claude Code 自动维护
- 会话数据存储在原生目录
- GUI 配置不影响 CLI 行为
- 启动选项只是 CLI 参数的便捷封装
