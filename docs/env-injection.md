# 环境变量注入

通过 cc-box 统一管理注入到 `~/.claude/settings.json` 的环境变量，控制 Claude CLI 运行时行为。

## 设计原则

- **单一管理入口**：所有环境变量在 Settings → Startup → Environment Variables 中集中管理
- **默认值在代码中定义**：首次使用自动填充，用户可随时重置
- **启动时自动同步**：每次启动将 cc-box 配置的环境变量合并写入 `~/.claude/settings.json`
- **实时生效**：设置面板中修改立即写入，无需重启
- **安全删除**：用户删除/改名 key 时，旧 key 从 Claude settings 中清除；不影响用户手动添加的非 cc-box 管理 key

## 数据存储

### cc-box 配置 (`~/.cc-box/config.json`)

```jsonc
{
  "claudeEnvVars": {             // cc-box 管理的完整键值对
    "LANG": "en_US.UTF-8",
    "LC_ALL": "en_US.UTF-8",
    "PYTHONUTF8": "1",
    "CLAUDE_CODE_SCROLL_SPEED": "5",
    "PYTHONIOENCODING": "utf-8",
    "CLAUDE_CODE_NO_FLICKER": "1"
  }
}
```

### Claude 配置 (`~/.claude/settings.json`)

cc-box 管理的 key 被合并写入 `env` 字段。用户手动添加到 Claude settings 的其他 env key 不受影响。

```jsonc
{
  "env": {
    // cc-box 管理的变量
    "LANG": "en_US.UTF-8",
    // ...
    // 用户手动添加的，cc-box 不动
    "MY_CUSTOM_VAR": "xxx"
  }
}
```

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

## 启动同步流程

```
loadAppConfig()
  ├── 读取 claudeEnvVars
  ├── 为空 → 填充 DEFAULT_CLAUDE_ENV_VARS
  └── doSyncEnv()
        ├── updateAppConfig({ claudeEnvVars })      → 写入 cc-box config
        └── syncClaudeEnv(claudeEnvVars, [])        → Tauri command（无删除）
              └── store::sync_claude_env(user_env, removed_keys)
                    ├── 1) 删除 removed_keys 中的旧 key
                    ├── 2) 写入/更新 user_env 中的 key-value
                    └── 3) 写回 ~/.claude/settings.json
```

## 设置面板交互

### Startup Section — 环境变量编辑器

| 操作 | 行为 |
|------|------|
| **打开面板** | 从 `appStore.claudeEnvVars` 加载当前值 |
| **编辑值** | `@change` → `syncEnv()` → `setClaudeEnvVars(vars)` → 写 cc-box config + 写 Claude settings |
| **编辑 key** | 删除旧 key + 添加新 key → `setClaudeEnvVars(vars, [oldKey])` → Claude settings 中旧 key 被清除 |
| **删除行** | 移除 key → `setClaudeEnvVars(vars, [removedKey])` → Claude settings 中该 key 被清除 |
| **新增行** | 添加 key-value → `syncEnv()` → 写入 |
| **Reset to defaults** | 只覆盖 `DEFAULT_CLAUDE_ENV_VARS` 中 key 的值，保留用户额外添加的变量不变 |

## Rust API

```rust
// store.rs
pub fn sync_claude_env(
    user_env: HashMap<String, String>,   // 要写入的键值对
    removed_keys: Vec<String>,            // 要从 Claude settings env 中删除的 key
) -> Result<()>

// commands.rs
#[tauri::command]
pub async fn sync_claude_env(
    user_env: HashMap<String, String>,
    removed_keys: Vec<String>,
) -> Result<(), String>
```

## 扩展方式

添加新的默认环境变量：

1. 在 `src/stores/app.ts` 的 `DEFAULT_CLAUDE_ENV_VARS` 中添加 key-value
2. 新用户首次启动自动获得
3. 现有用户点击"Reset to defaults"获取
