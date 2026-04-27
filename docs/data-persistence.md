# 数据持久化

## 数据保存原则

应用遵循以下原则管理数据：

1. **原生数据只读** — Claude Code 原生配置只读取不修改
2. **应用配置独立** — GUI 特有设置保存在独立目录
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
| `~/.claude-gui/config.json` | GUI 配置（启动选项默认值、主题、字号等） |

## ~/.claude.json 结构

应用直接读取此文件获取项目列表：

```json
{
  "projects": {
    "D:/projects/my-app": {
      "lastSessionId": "uuid-xxx",
      "lastDuration": 123456,
      "lastCost": 0.05
    },
    "D:/projects/api-server": {
      "lastSessionId": "uuid-yyy",
      "lastDuration": 78901
    }
  },
  "theme": "light",
  "autoConnectIde": true
}
```

项目列表按 `lastDuration` 降序排序，最近使用的项目排在最前面。

## ~/.claude-gui/config.json 结构

应用专属配置，保存 GUI 特有设置：

```json
{
  "defaultContinue": true,
  "defaultSkipPermissions": false,
  "defaultCustomArgs": "",
  "theme": "light",
  "fontSize": 14,
  "lastOpenedProject": "D:/projects/my-app"
}
```

字段说明：

| 字段 | 类型 | 说明 |
|------|------|------|
| `defaultContinue` | boolean | `-c` 选项默认值 |
| `defaultSkipPermissions` | boolean | `--dangerously-skip-permissions` 默认值 |
| `defaultCustomArgs` | string | 自定义参数默认值 |
| `theme` | string | GUI 主题 |
| `fontSize` | number | 终端字号 |
| `lastOpenedProject` | string | 上次打开的项目路径 |

## 存储模块 (main/store.ts)

### Claude Code 原生数据

```typescript
// 获取项目列表（按最近使用排序）
export function getProjects(): Project[]

// 获取项目信息
export function getProjectInfo(path: string): ProjectInfo | null

// 获取全局设置
export function getGlobalSettings(): Record<string, unknown>

// 获取用户偏好
export function getUserPreferences(): { theme, autoConnectIde, ... }
```

### 应用专属配置

```typescript
// 获取应用配置
export function getAppConfig(): AppConfig

// 更新应用配置
export function updateAppConfig(updates: Partial<AppConfig>): void

// 获取默认启动选项
export function getDefaultClaudeOptions(): ClaudeOptions

// 保存默认启动选项
export function saveDefaultClaudeOptions(options): void

// 保存上次打开的项目
export function saveLastProject(path: string): void
```

## IPC 通道

### Claude Code 数据

| 通道 | 说明 |
|------|------|
| `store:getProjects` | 获取项目列表 |
| `store:getProjectInfo` | 获取项目详情 |
| `store:getGlobalSettings` | 获取全局设置 |
| `store:getUserPreferences` | 获取用户偏好 |

### 应用配置

| 通道 | 说明 |
|------|------|
| `store:getAppConfig` | 获取应用配置 |
| `store:updateAppConfig` | 更新应用配置 |
| `store:getDefaultClaudeOptions` | 获取默认启动选项 |
| `store:saveDefaultClaudeOptions` | 保存默认启动选项 |
| `store:saveLastProject` | 保存上次项目 |

## 数据流程

```
启动时:
  App.vue → loadAppConfig() → 恢复上次项目、默认选项
  ProjectSelectView → getProjects() → 显示项目列表

选择项目:
  setCwd(path) → saveLastProject(path) → 记录到应用配置

设置默认选项:
  saveAsDefault() → saveDefaultClaudeOptions() → 持久化到应用配置

启动终端:
  getClaudeArgs() → 应用启动选项 → 传递给 Claude CLI
```

## 兼容性

用户可随时回到 CLI：

- 项目列表由 Claude Code 自动维护
- 会话数据存储在原生目录
- GUI 配置不影响 CLI 行为
- 启动选项只是 CLI 参数的便捷封装