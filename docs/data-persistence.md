# 数据持久化

## 数据保存原则

1. **原生数据只读** — Claude Code 原生配置只读取不修改
2. **应用配置独立** — GUI 特有设置保存在 `~/.cc-box/`
3. **不重复存储** — 项目列表等数据直接从原生配置读取
4. **默认值持久化** — 用户偏好设置保存在应用配置

## 文件路径

### Claude Code 原生文件（只读）

| 文件 | 用途 |
|------|------|
| `~/.claude.json` | 项目列表、用户偏好、会话信息 |
| `~/.claude/settings.json` | 全局配置（MCP、权限、模型） |
| `~/.claude/projects/<encoded-path>/` | 项目会话数据 |
| `<project>/.claude/settings.json` | 项目配置 |

### 应用专属文件（读写）

| 文件 | 用途 |
|------|------|
| `~/.cc-box/config.json` | GUI 配置（路径缓存、主题、字号、启动参数默认值） |
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
| `lastOpenedProject` | string? | 上次打开的项目路径 |

## Store 命令 (IPC 通道)

| 命令 | 说明 |
|------|------|
| `get_home_data` | 一次获取项目列表 + 近期会话（合并 IO） |
| `get_projects` | 项目列表（分页） |
| `get_project_info` | 项目详情 |
| `get_sessions` | 会话列表（分页） |
| `get_session_count` | 会话总数 |
| `get_all_recent_sessions` | 跨项目近期会话 |
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
