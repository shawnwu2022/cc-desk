# 环境变量注入

通过 CC Desk 统一管理环境变量，在 PTY 启动时注入到终端进程，控制 Claude CLI 运行时行为。

## 设计原则

- **单一管理入口**：所有环境变量在 Settings → Startup → Environment Variables 中集中管理
- **默认值在代码中定义**：首次使用自动填充，用户可随时重置
- **PTY 注入**：环境变量在创建新终端时直接注入进程环境，不修改 Claude Code 配置文件
- **与 Provider 无冲突**：本页环境变量只注入 PTY；只有 Provider 模块在用户显式激活时合并 `~/.claude/settings.json`

## 数据存储

### CC Desk 配置 (`~/.cc-box/config.json`)

```jsonc
{
  "claudeEnvVars": {             // CC Desk 管理的完整键值对
    "LANG": "en_US.UTF-8",
    "LC_ALL": "en_US.UTF-8",
    "PYTHONUTF8": "1",
    "CLAUDE_CODE_SCROLL_SPEED": "5",
    "PYTHONIOENCODING": "utf-8",
    "CLAUDE_CODE_NO_FLICKER": "1"
  }
}
```

### 不修改 Claude 配置

CC Desk 的环境变量页面**不写入** `~/.claude/settings.json` 的 `env` 字段。环境变量通过 PTY 进程环境注入，Claude CLI 作为子进程自动继承。

## 默认环境变量

定义在 `src/stores/app.ts` 的 `DEFAULT_CLAUDE_ENV_VARS`：

```typescript
const DEFAULT_CLAUDE_ENV_VARS: Record<string, string> = {
  LANG: 'en_US.UTF-8',
  LC_ALL: 'en_US.UTF-8',
  PYTHONUTF8: '1',
  CLAUDE_CODE_SCROLL_SPEED: '5',
  PYTHONIOENCODING: 'utf-8',
  CLAUDE_CODE_NO_FLICKER: '1',
}
```

**添加新默认变量**：在此常量中添加 key-value，新用户首次启动自动获得，现有用户点"Reset to defaults"获取。

## 数据流

```
用户在 Settings 编辑 env vars
  → setClaudeEnvVars(vars)
    → doSyncEnv()
      → updateAppConfig({ claudeEnvVars })      // 写入 ~/.cc-box/config.json

PTY 启动时（spawn_claude / spawn_shell）：
  → get_app_config().claude_env_vars
  → cmd.env(key, value) 逐个注入到子进程环境
```

### 注入顺序

PTY 启动时按以下顺序设置环境变量（后设置的覆盖先设置的）：

1. `env::vars()` — 继承父进程所有环境变量
2. `TERM`、`COLORTERM` — 终端基础变量
3. `CC_BOX_HOOK_PORT`、`CC_BOX_SESSION_ID` — Hook 监控
4. `CLAUDE_CODE_GIT_BASH_PATH`（Windows）— Git Bash 路径
5. **CC Desk env vars** — 用户配置的环境变量（优先级最高）

## 设置面板交互

### Startup Section — 环境变量编辑器

| 操作 | 行为 |
|------|------|
| **打开面板** | 从 `appStore.claudeEnvVars` 加载当前值 |
| **编辑值** | `@change` → `syncEnv()` → `setClaudeEnvVars(vars)` → 写 CC Desk config |
| **编辑 key** | 删除旧 key + 添加新 key → `setClaudeEnvVars(vars)` → 写 CC Desk config |
| **删除行** | 移除 key → `setClaudeEnvVars(vars)` → 写 CC Desk config |
| **新增行** | 添加 key-value → `syncEnv()` → 写 CC Desk config |
| **Reset to defaults** | 只覆盖 `DEFAULT_CLAUDE_ENV_VARS` 中 key 的值，保留用户额外添加的变量不变 |

修改后需新建终端才能生效（环境变量在 PTY 启动时注入，不影响已运行的终端）。

## 扩展方式

添加新的默认环境变量：

1. 在 `src/stores/app.ts` 的 `DEFAULT_CLAUDE_ENV_VARS` 中添加 key-value
2. 新用户首次启动自动获得
3. 现有用户点击"Reset to defaults"获取
4. 新建终端后生效
