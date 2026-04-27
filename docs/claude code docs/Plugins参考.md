> ## Documentation Index
> Fetch the complete documentation index at: https://code.claude.com/docs/llms.txt
> Use this file to discover all available pages before exploring further.

# Plugins 参考

> Claude Code 插件系统的完整技术参考，包括架构、CLI 命令和组件规范。

<Tip>
  想要安装插件？请参阅[发现和安装插件](/zh-CN/discover-plugins)。如需创建插件，请参阅[Plugins](/zh-CN/plugins)。如需分发插件，请参阅[Plugin marketplaces](/zh-CN/plugin-marketplaces)。
</Tip>

本参考提供了 Claude Code 插件系统的完整技术规范，包括组件架构、CLI 命令和开发工具。

**plugin** 是一个自包含的组件目录，用于扩展 Claude Code 的自定义功能。插件组件包括 skills、agents、hooks、MCP servers、LSP servers 和 monitors。

## Plugin 组件参考

### Skills

Plugins 向 Claude Code 添加 skills，创建可由您或 Claude 调用的 `/name` 快捷方式。

**位置**：插件根目录中的 `skills/` 或 `commands/` 目录

**文件格式**：Skills 是包含 `SKILL.md` 的目录；commands 是简单的 markdown 文件

**Skill 结构**：

```text theme={null}
skills/
├── pdf-processor/
│   ├── SKILL.md
│   ├── reference.md (可选)
│   └── scripts/ (可选)
└── code-reviewer/
    └── SKILL.md
```

**集成行为**：

* 安装插件时会自动发现 Skills 和 commands
* Claude 可以根据任务上下文自动调用它们
* Skills 可以在 SKILL.md 旁边包含支持文件

有关完整详情，请参阅[Skills](/zh-CN/skills)。

### Agents

Plugins 可以为特定任务提供专门的 subagents，Claude 可以在适当时自动调用。

**位置**：插件根目录中的 `agents/` 目录

**文件格式**：描述 agent 功能的 Markdown 文件

**Agent 结构**：

```markdown theme={null}
---
name: agent-name
description: 该 agent 的专长以及 Claude 应何时调用它
model: sonnet
effort: medium
maxTurns: 20
disallowedTools: Write, Edit
---

详细的系统提示，描述 agent 的角色、专业知识和行为。
```

Plugin agents 支持 `name`、`description`、`model`、`effort`、`maxTurns`、`tools`、`disallowedTools`、`skills`、`memory`、`background` 和 `isolation` frontmatter 字段。唯一有效的 `isolation` 值是 `"worktree"`。出于安全原因，plugin 提供的 agents 不支持 `hooks`、`mcpServers` 和 `permissionMode`。

**集成点**：

* Agents 出现在 `/agents` 界面中
* Claude 可以根据任务上下文自动调用 agents
* Agents 可以由用户手动调用
* Plugin agents 与内置 Claude agents 一起工作

有关完整详情，请参阅[Subagents](/zh-CN/sub-agents)。

### Hooks

Plugins 可以提供事件处理程序，自动响应 Claude Code 事件。

**位置**：插件根目录中的 `hooks/hooks.json`，或在 plugin.json 中内联

**格式**：具有事件匹配器和操作的 JSON 配置

**Hook 配置**：

```json theme={null}
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/scripts/format-code.sh"
          }
        ]
      }
    ]
  }
}
```

Plugin hooks 响应与[用户定义的 hooks](/zh-CN/hooks)相同的生命周期事件：

| Event                 | When it fires                                                                                                                                          |
| :-------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `SessionStart`        | When a session begins or resumes                                                                                                                       |
| `UserPromptSubmit`    | When you submit a prompt, before Claude processes it                                                                                                   |
| `UserPromptExpansion` | When a user-typed command expands into a prompt, before it reaches Claude. Can block the expansion                                                     |
| `PreToolUse`          | Before a tool call executes. Can block it                                                                                                              |
| `PermissionRequest`   | When a permission dialog appears                                                                                                                       |
| `PermissionDenied`    | When a tool call is denied by the auto mode classifier. Return `{retry: true}` to tell the model it may retry the denied tool call                     |
| `PostToolUse`         | After a tool call succeeds                                                                                                                             |
| `PostToolUseFailure`  | After a tool call fails                                                                                                                                |
| `PostToolBatch`       | After a full batch of parallel tool calls resolves, before the next model call                                                                         |
| `Notification`        | When Claude Code sends a notification                                                                                                                  |
| `SubagentStart`       | When a subagent is spawned                                                                                                                             |
| `SubagentStop`        | When a subagent finishes                                                                                                                               |
| `TaskCreated`         | When a task is being created via `TaskCreate`                                                                                                          |
| `TaskCompleted`       | When a task is being marked as completed                                                                                                               |
| `Stop`                | When Claude finishes responding                                                                                                                        |
| `StopFailure`         | When the turn ends due to an API error. Output and exit code are ignored                                                                               |
| `TeammateIdle`        | When an [agent team](/en/agent-teams) teammate is about to go idle                                                                                     |
| `InstructionsLoaded`  | When a CLAUDE.md or `.claude/rules/*.md` file is loaded into context. Fires at session start and when files are lazily loaded during a session         |
| `ConfigChange`        | When a configuration file changes during a session                                                                                                     |
| `CwdChanged`          | When the working directory changes, for example when Claude executes a `cd` command. Useful for reactive environment management with tools like direnv |
| `FileChanged`         | When a watched file changes on disk. The `matcher` field specifies which filenames to watch                                                            |
| `WorktreeCreate`      | When a worktree is being created via `--worktree` or `isolation: "worktree"`. Replaces default git behavior                                            |
| `WorktreeRemove`      | When a worktree is being removed, either at session exit or when a subagent finishes                                                                   |
| `PreCompact`          | Before context compaction                                                                                                                              |
| `PostCompact`         | After context compaction completes                                                                                                                     |
| `Elicitation`         | When an MCP server requests user input during a tool call                                                                                              |
| `ElicitationResult`   | After a user responds to an MCP elicitation, before the response is sent back to the server                                                            |
| `SessionEnd`          | When a session terminates                                                                                                                              |

**Hook 类型**：

* `command`：执行 shell 命令或脚本
* `http`：将事件 JSON 作为 POST 请求发送到 URL
* `mcp_tool`：在配置的 [MCP server](/zh-CN/mcp) 上调用工具
* `prompt`：使用 LLM 评估提示（使用 `$ARGUMENTS` 占位符表示上下文）
* `agent`：运行具有工具的 agentic 验证器以完成复杂验证任务

### MCP servers

Plugins 可以捆绑 Model Context Protocol (MCP) servers 以将 Claude Code 与外部工具和服务连接。

**位置**：插件根目录中的 `.mcp.json`，或在 plugin.json 中内联

**格式**：标准 MCP server 配置

**MCP server 配置**：

```json theme={null}
{
  "mcpServers": {
    "plugin-database": {
      "command": "${CLAUDE_PLUGIN_ROOT}/servers/db-server",
      "args": ["--config", "${CLAUDE_PLUGIN_ROOT}/config.json"],
      "env": {
        "DB_PATH": "${CLAUDE_PLUGIN_ROOT}/data"
      }
    },
    "plugin-api-client": {
      "command": "npx",
      "args": ["@company/mcp-server", "--plugin-mode"],
      "cwd": "${CLAUDE_PLUGIN_ROOT}"
    }
  }
}
```

**集成行为**：

* 启用插件时，Plugin MCP servers 会自动启动
* Servers 在 Claude 的工具包中显示为标准 MCP 工具
* Server 功能与 Claude 的现有工具无缝集成
* Plugin servers 可以独立于用户 MCP servers 进行配置

### LSP servers

<Tip>
  想要使用 LSP plugins？从官方市场安装它们：在 `/plugin` Discover 选项卡中搜索"lsp"。本部分记录了如何为官方市场未涵盖的语言创建 LSP plugins。
</Tip>

Plugins 可以提供[Language Server Protocol](https://microsoft.github.io/language-server-protocol/) (LSP) servers，在处理代码库时为 Claude 提供实时代码智能。

LSP 集成提供：

* **即时诊断**：Claude 在每次编辑后立即看到错误和警告
* **代码导航**：转到定义、查找引用和悬停信息
* **语言感知**：代码符号的类型信息和文档

**位置**：插件根目录中的 `.lsp.json`，或在 `plugin.json` 中内联

**格式**：将语言服务器名称映射到其配置的 JSON 配置

**`.lsp.json` 文件格式**：

```json theme={null}
{
  "go": {
    "command": "gopls",
    "args": ["serve"],
    "extensionToLanguage": {
      ".go": "go"
    }
  }
}
```

**在 `plugin.json` 中内联**：

```json theme={null}
{
  "name": "my-plugin",
  "lspServers": {
    "go": {
      "command": "gopls",
      "args": ["serve"],
      "extensionToLanguage": {
        ".go": "go"
      }
    }
  }
}
```

**必需字段：**

| 字段                    | 描述                         |
| :-------------------- | :------------------------- |
| `command`             | 要执行的 LSP 二进制文件（必须在 PATH 中） |
| `extensionToLanguage` | 将文件扩展名映射到语言标识符             |

**可选字段：**

| 字段                      | 描述                                          |
| :---------------------- | :------------------------------------------ |
| `args`                  | LSP server 的命令行参数                           |
| `transport`             | 通信传输：`stdio`（默认）或 `socket`                  |
| `env`                   | 启动 server 时要设置的环境变量                         |
| `initializationOptions` | 在初始化期间传递给 server 的选项                        |
| `settings`              | 通过 `workspace/didChangeConfiguration` 传递的设置 |
| `workspaceFolder`       | server 的工作区文件夹路径                            |
| `startupTimeout`        | 等待 server 启动的最长时间（毫秒）                       |
| `shutdownTimeout`       | 等待正常关闭的最长时间（毫秒）                             |
| `restartOnCrash`        | server 崩溃时是否自动重启                            |
| `maxRestarts`           | 放弃前的最大重启尝试次数                                |

<Warning>
  **您必须单独安装语言服务器二进制文件。** LSP plugins 配置 Claude Code 如何连接到语言服务器，但它们不包括服务器本身。如果在 `/plugin` Errors 选项卡中看到 `Executable not found in $PATH`，请为您的语言安装所需的二进制文件。
</Warning>

**可用的 LSP plugins：**

| Plugin           | 语言服务器                      | 安装命令                                                                            |
| :--------------- | :------------------------- | :------------------------------------------------------------------------------ |
| `pyright-lsp`    | Pyright (Python)           | `pip install pyright` 或 `npm install -g pyright`                                |
| `typescript-lsp` | TypeScript Language Server | `npm install -g typescript-language-server typescript`                          |
| `rust-lsp`       | rust-analyzer              | [参阅 rust-analyzer 安装](https://rust-analyzer.github.io/manual.html#installation) |

首先安装语言服务器，然后从市场安装 plugin。

### Monitors

Plugins 可以声明后台 monitors，Claude Code 在 plugin 激活时自动启动。每个 monitor 为会话的生命周期运行一个 shell 命令，并将每个 stdout 行作为通知传递给 Claude，以便 Claude 可以对日志条目、状态更改或轮询事件做出反应，而无需被要求启动监视本身。

Plugin monitors 使用与[Monitor tool](/zh-CN/tools-reference#monitor-tool)相同的机制，并共享其可用性约束。它们仅在交互式 CLI 会话中运行，在与[hooks](#hooks)相同的信任级别上无沙箱运行，并在 Monitor tool 不可用的主机上跳过。

<Note>
  Plugin monitors 需要 Claude Code v2.1.105 或更高版本。
</Note>

**位置**：插件根目录中的 `monitors/monitors.json`，或在 plugin.json 中内联

**格式**：监视器条目的 JSON 数组

以下 `monitors/monitors.json` 监视部署状态端点和本地错误日志：

```json theme={null}
[
  {
    "name": "deploy-status",
    "command": "${CLAUDE_PLUGIN_ROOT}/scripts/poll-deploy.sh ${user_config.api_endpoint}",
    "description": "Deployment status changes"
  },
  {
    "name": "error-log",
    "command": "tail -F ./logs/error.log",
    "description": "Application error log",
    "when": "on-skill-invoke:debug"
  }
]
```

要内联声明 monitors，请将 `plugin.json` 中的 `monitors` 键设置为相同的数组。要从非默认路径加载，请将 `monitors` 设置为相对路径字符串，例如 `"./config/monitors.json"`。

**必需字段：**

| 字段            | 描述                                     |
| :------------ | :------------------------------------- |
| `name`        | 在插件中唯一的标识符。防止插件重新加载或再次调用 skill 时出现重复进程 |
| `command`     | 在会话工作目录中作为持久后台进程运行的 shell 命令           |
| `description` | 正在监视的内容的简短摘要。显示在任务面板和通知摘要中             |

**可选字段：**

| 字段     | 描述                                                                                                          |
| :----- | :---------------------------------------------------------------------------------------------------------- |
| `when` | 控制 monitor 何时启动。`"always"` 在会话启动和插件重新加载时启动它，这是默认值。`"on-skill-invoke:<skill-name>"` 在此插件中的命名 skill 首次被分派时启动它 |

`command` 值支持与 MCP 和 LSP server 配置相同的[变量替换](#environment-variables)：`${CLAUDE_PLUGIN_ROOT}`、`${CLAUDE_PLUGIN_DATA}`、`${user_config.*}` 和环境中的任何 `${ENV_VAR}`。如果脚本需要从插件自己的目录运行，请在命令前加上 `cd "${CLAUDE_PLUGIN_ROOT}" && `。

在会话中途禁用插件不会停止已在运行的 monitors。它们在会话结束时停止。

### Themes

Plugins 可以提供颜色主题，这些主题与内置预设和用户的本地主题一起出现在 `/theme` 中。主题是 `themes/` 中的 JSON 文件，具有 `base` 预设和稀疏的 `overrides` 颜色令牌映射。

```json theme={null}
{
  "name": "Dracula",
  "base": "dark",
  "overrides": {
    "claude": "#bd93f9",
    "error": "#ff5555",
    "success": "#50fa7b"
  }
}
```

选择 plugin 主题会在用户的配置中持久化 `custom:<plugin-name>:<slug>`。Plugin 主题是只读的；在 `/theme` 中按 `Ctrl+E` 会将其复制到 `~/.claude/themes/`，以便用户可以编辑副本。

***

## Plugin 安装范围

安装 plugin 时，您选择一个**范围**，确定 plugin 的可用位置以及谁可以使用它：

| 范围        | 设置文件                                               | 用例                       |
| :-------- | :------------------------------------------------- | :----------------------- |
| `user`    | `~/.claude/settings.json`                          | 在所有项目中可用的个人 plugins（默认）  |
| `project` | `.claude/settings.json`                            | 通过版本控制共享的团队 plugins      |
| `local`   | `.claude/settings.local.json`                      | 项目特定的 plugins，gitignored |
| `managed` | [Managed settings](/zh-CN/settings#settings-files) | 托管 plugins（只读，仅更新）       |

Plugins 使用与其他 Claude Code 配置相同的范围系统。有关安装说明和范围标志，请参阅[安装 plugins](/zh-CN/discover-plugins#install-plugins)。有关范围的完整说明，请参阅[Configuration scopes](/zh-CN/settings#configuration-scopes)。

***

## Plugin 清单架构

`.claude-plugin/plugin.json` 文件定义了您的 plugin 的元数据和配置。本部分记录了所有支持的字段和选项。

清单是可选的。如果省略，Claude Code 会自动发现[默认位置](#file-locations-reference)中的组件，并从目录名称派生 plugin 名称。当您需要提供元数据或自定义组件路径时，使用清单。

### 完整架构

```json theme={null}
{
  "name": "plugin-name",
  "version": "1.2.0",
  "description": "Brief plugin description",
  "author": {
    "name": "Author Name",
    "email": "author@example.com",
    "url": "https://github.com/author"
  },
  "homepage": "https://docs.example.com/plugin",
  "repository": "https://github.com/author/plugin",
  "license": "MIT",
  "keywords": ["keyword1", "keyword2"],
  "skills": "./custom/skills/",
  "commands": ["./custom/commands/special.md"],
  "agents": "./custom/agents/",
  "hooks": "./config/hooks.json",
  "mcpServers": "./mcp-config.json",
  "outputStyles": "./styles/",
  "themes": "./themes/",
  "lspServers": "./.lsp.json",
  "monitors": "./monitors.json",
  "dependencies": [
    "helper-lib",
    { "name": "secrets-vault", "version": "~2.1.0" }
  ]
}
```

### 必需字段

如果包含清单，`name` 是唯一必需的字段。

| 字段     | 类型     | 描述                    | 示例                   |
| :----- | :----- | :-------------------- | :------------------- |
| `name` | string | 唯一标识符（kebab-case，无空格） | `"deployment-tools"` |

此名称用于命名空间组件。例如，在 UI 中，名为 `plugin-dev` 的 plugin 的 agent `agent-creator` 将显示为 `plugin-dev:agent-creator`。

### 元数据字段

| 字段            | 类型     | 描述                                                                                                                                                                       | 示例                                                 |
| :------------ | :----- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :------------------------------------------------- |
| `version`     | string | 可选。语义版本。设置此项会将 plugin 固定到该版本字符串，因此用户仅在您提升版本时才会收到更新。如果省略，Claude Code 会回退到 git commit SHA，因此每个 commit 都被视为新版本。如果也在市场条目中设置，`plugin.json` 优先。请参阅[版本管理](#version-management)。 | `"2.1.0"`                                          |
| `description` | string | plugin 目的的简要说明                                                                                                                                                           | `"Deployment automation tools"`                    |
| `author`      | object | 作者信息                                                                                                                                                                     | `{"name": "Dev Team", "email": "dev@company.com"}` |
| `homepage`    | string | 文档 URL                                                                                                                                                                   | `"https://docs.example.com"`                       |
| `repository`  | string | 源代码 URL                                                                                                                                                                  | `"https://github.com/user/plugin"`                 |
| `license`     | string | 许可证标识符                                                                                                                                                                   | `"MIT"`、`"Apache-2.0"`                             |
| `keywords`    | array  | 发现标签                                                                                                                                                                     | `["deployment", "ci-cd"]`                          |

### 组件路径字段

| 字段             | 类型                    | 描述                                                                                                     | 示例                                                   |
| :------------- | :-------------------- | :----------------------------------------------------------------------------------------------------- | :--------------------------------------------------- |
| `skills`       | string\|array         | 包含 `<name>/SKILL.md` 的自定义 skill 目录（替换默认 `skills/`）                                                     | `"./custom/skills/"`                                 |
| `commands`     | string\|array         | 自定义平面 `.md` skill 文件或目录（替换默认 `commands/`）                                                              | `"./custom/cmd.md"` 或 `["./cmd1.md"]`                |
| `agents`       | string\|array         | 自定义 agent 文件（替换默认 `agents/`）                                                                           | `"./custom/agents/reviewer.md"`                      |
| `hooks`        | string\|array\|object | Hook 配置路径或内联配置                                                                                         | `"./my-extra-hooks.json"`                            |
| `mcpServers`   | string\|array\|object | MCP 配置路径或内联配置                                                                                          | `"./my-extra-mcp-config.json"`                       |
| `outputStyles` | string\|array         | 自定义输出样式文件/目录（替换默认 `output-styles/`）                                                                    | `"./styles/"`                                        |
| `themes`       | string\|array         | 颜色主题文件/目录（替换默认 `themes/`）。请参阅[Themes](#themes)                                                         | `"./themes/"`                                        |
| `lspServers`   | string\|array\|object | [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) 配置用于代码智能（转到定义、查找引用等） | `"./.lsp.json"`                                      |
| `monitors`     | string\|array         | 后台[Monitor](/zh-CN/tools-reference#monitor-tool)配置，在 plugin 激活时自动启动。请参阅[Monitors](#monitors)           | `"./monitors.json"`                                  |
| `userConfig`   | object                | 用户可配置的值，在启用时提示。请参阅[用户配置](#user-configuration)                                                          | 见下文                                                  |
| `channels`     | array                 | 消息注入的频道声明（Telegram、Slack、Discord 风格）。请参阅[Channels](#channels)                                          | 见下文                                                  |
| `dependencies` | array                 | 此 plugin 需要的其他 plugins，可选择带有 semver 版本约束。请参阅[约束 plugin 依赖版本](/zh-CN/plugin-dependencies)               | `[{ "name": "secrets-vault", "version": "~2.1.0" }]` |

### 用户配置

`userConfig` 字段声明了 Claude Code 在启用 plugin 时提示用户的值。使用此字段而不是要求用户手动编辑 `settings.json`。

```json theme={null}
{
  "userConfig": {
    "api_endpoint": {
      "type": "string",
      "title": "API endpoint",
      "description": "Your team's API endpoint"
    },
    "api_token": {
      "type": "string",
      "title": "API token",
      "description": "API authentication token",
      "sensitive": true
    }
  }
}
```

键必须是有效的标识符。每个选项支持这些字段：

| 字段            | 必需 | 描述                                                    |
| :------------ | :- | :---------------------------------------------------- |
| `type`        | 是  | 以下之一：`string`、`number`、`boolean`、`directory` 或 `file` |
| `title`       | 是  | 在配置对话框中显示的标签                                          |
| `description` | 是  | 显示在字段下方的帮助文本                                          |
| `sensitive`   | 否  | 如果为 `true`，掩盖输入并将值存储在安全存储中而不是 `settings.json`         |
| `required`    | 否  | 如果为 `true`，当字段为空时验证失败                                 |
| `default`     | 否  | 用户未提供任何内容时使用的值                                        |
| `multiple`    | 否  | 对于 `string` 类型，允许字符串数组                                |
| `min` / `max` | 否  | `number` 类型的边界                                        |

每个值都可用于在 MCP 和 LSP server 配置、hook 命令和 monitor 命令中作为 `${user_config.KEY}` 进行替换。非敏感值也可以在 skill 和 agent 内容中替换。所有值都作为 `CLAUDE_PLUGIN_OPTION_<KEY>` 环境变量导出到 plugin 子进程。

非敏感值存储在 `settings.json` 中的 `pluginConfigs[<plugin-id>].options` 下。敏感值进入系统钥匙链（或在钥匙链不可用的地方进入 `~/.claude/.credentials.json`）。钥匙链存储与 OAuth 令牌共享，总限制约为 2 KB，因此请保持敏感值较小。

### Channels

`channels` 字段允许 plugin 声明一个或多个消息频道，将内容注入到对话中。每个频道绑定到 plugin 提供的 MCP server。

```json theme={null}
{
  "channels": [
    {
      "server": "telegram",
      "userConfig": {
        "bot_token": {
          "type": "string",
          "title": "Bot token",
          "description": "Telegram bot token",
          "sensitive": true
        },
        "owner_id": {
          "type": "string",
          "title": "Owner ID",
          "description": "Your Telegram user ID"
        }
      }
    }
  ]
}
```

`server` 字段是必需的，必须与 plugin 的 `mcpServers` 中的键匹配。可选的每个频道 `userConfig` 使用与顶级字段相同的架构，允许 plugin 在启用 plugin 时提示输入机器人令牌或所有者 ID。

### 路径行为规则

对于 `skills`、`commands`、`agents`、`outputStyles`、`themes` 和 `monitors`，自定义路径替换默认值。如果清单指定 `skills`，则不会扫描默认 `skills/` 目录；如果指定 `monitors`，则不会加载默认 `monitors/monitors.json`。[Hooks](#hooks)、[MCP servers](#mcp-servers) 和[LSP servers](#lsp-servers)对处理多个源有不同的语义。

* 所有路径必须相对于 plugin 根目录，并以 `./` 开头
* 来自自定义路径的组件使用相同的命名和命名空间规则
* 可以将多个路径指定为数组
* 要保留默认目录并为 skills、commands、agents 或 output styles 添加更多路径，请在数组中包含默认值：`"skills": ["./skills/", "./extras/"]`
* 当 skill 路径指向直接包含 `SKILL.md` 的目录时，例如 `"skills": ["./"]` 指向 plugin 根目录，frontmatter 中的 `name` 字段确定 skill 的调用名称。这提供了一个稳定的名称，无论安装目录如何。如果 frontmatter 中未设置 `name`，则使用目录基名作为后备。

**路径示例**：

```json theme={null}
{
  "commands": [
    "./specialized/deploy.md",
    "./utilities/batch-process.md"
  ],
  "agents": [
    "./custom-agents/reviewer.md",
    "./custom-agents/tester.md"
  ]
}
```

### 环境变量

Claude Code 提供两个变量用于引用 plugin 路径。两者都在 skill 内容、agent 内容、hook 命令、monitor 命令以及 MCP 或 LSP server 配置中出现的任何地方进行内联替换。两者也都作为环境变量导出到 hook 进程和 MCP 或 LSP server 子进程。

**`${CLAUDE_PLUGIN_ROOT}`**：plugin 安装目录的绝对路径。使用此路径引用与 plugin 捆绑的脚本、二进制文件和配置文件。当 plugin 更新时，此路径会更改，因此您在此处写入的文件不会在更新后保留。

**`${CLAUDE_PLUGIN_DATA}`**：用于 plugin 状态的持久目录，在更新后保留。使用此目录用于已安装的依赖项，如 `node_modules` 或 Python 虚拟环境、生成的代码、缓存以及任何应在 plugin 版本之间保留的其他文件。首次引用此变量时，目录会自动创建。

```json theme={null}
{
  "hooks": {
    "PostToolUse": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/scripts/process.sh"
          }
        ]
      }
    ]
  }
}
```

#### 持久数据目录

`${CLAUDE_PLUGIN_DATA}` 目录解析为 `~/.claude/plugins/data/{id}/`，其中 `{id}` 是 plugin 标识符，其中 `a-z`、`A-Z`、`0-9`、`_` 和 `-` 之外的字符被替换为 `-`。对于安装为 `formatter@my-marketplace` 的 plugin，目录是 `~/.claude/plugins/data/formatter-my-marketplace/`。

常见用途是一次安装语言依赖项并在会话和 plugin 更新中重复使用它们。由于数据目录的生命周期长于任何单个 plugin 版本，仅检查目录存在性无法检测到更新何时更改了 plugin 的依赖项清单。推荐的模式是将捆绑的清单与数据目录中的副本进行比较，并在它们不同时重新安装。

此 `SessionStart` hook 在第一次运行时安装 `node_modules`，并在 plugin 更新包含更改的 `package.json` 时再次安装：

```json theme={null}
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "diff -q \"${CLAUDE_PLUGIN_ROOT}/package.json\" \"${CLAUDE_PLUGIN_DATA}/package.json\" >/dev/null 2>&1 || (cd \"${CLAUDE_PLUGIN_DATA}\" && cp \"${CLAUDE_PLUGIN_ROOT}/package.json\" . && npm install) || rm -f \"${CLAUDE_PLUGIN_DATA}/package.json\""
          }
        ]
      }
    ]
  }
}
```

当存储的副本缺失或与捆绑的副本不同时，`diff` 退出非零，涵盖第一次运行和依赖项更改的更新。如果 `npm install` 失败，尾部的 `rm` 会删除复制的清单，以便下一个会话重试。

捆绑在 `${CLAUDE_PLUGIN_ROOT}` 中的脚本可以针对持久的 `node_modules` 运行：

```json theme={null}
{
  "mcpServers": {
    "routines": {
      "command": "node",
      "args": ["${CLAUDE_PLUGIN_ROOT}/server.js"],
      "env": {
        "NODE_PATH": "${CLAUDE_PLUGIN_DATA}/node_modules"
      }
    }
  }
}
```

当您从最后一个安装了 plugin 的范围卸载 plugin 时，数据目录会自动删除。`/plugin` 界面显示目录大小并在删除前提示。CLI 默认删除；传递 [`--keep-data`](#plugin-uninstall) 以保留它。

***

## Plugin 缓存和文件解析

Plugins 通过以下两种方式之一指定：

* 通过 `claude --plugin-dir`，用于会话期间。
* 通过市场，为将来的会话安装。

出于安全和验证目的，Claude Code 将\_市场\_ plugins 复制到用户的本地 **plugin 缓存**（`~/.claude/plugins/cache`），而不是就地使用它们。在开发引用外部文件的 plugins 时，理解此行为很重要。

每个已安装的版本是缓存中的单独目录。当您更新或卸载 plugin 时，前一个版本目录被标记为孤立，并在 7 天后自动删除。宽限期允许已加载旧版本的并发 Claude Code 会话继续运行而不出错。

Claude 的 Glob 和 Grep 工具在搜索期间跳过孤立版本目录，因此文件结果不包括过时的插件代码。

### 路径遍历限制

已安装的 plugins 无法引用其目录外的文件。遍历 plugin 根目录外的路径（例如 `../shared-utils`）在安装后将不起作用，因为这些外部文件不会被复制到缓存中。

### 使用外部依赖

如果您的 plugin 需要访问其目录外的文件，您可以在 plugin 目录中创建指向外部文件的符号链接。符号链接在缓存中被保留而不是解引用，并在运行时解析到其目标。以下命令在插件目录内创建指向共享实用程序位置的链接：

```bash theme={null}
ln -s /path/to/shared-utils ./shared-utils
```

这在保持缓存系统安全优势的同时提供了灵活性。

***

## Plugin 目录结构

### 标准 plugin 布局

完整的 plugin 遵循此结构：

```text theme={null}
enterprise-plugin/
├── .claude-plugin/           # 元数据目录（可选）
│   └── plugin.json             # plugin 清单
├── skills/                   # Skills
│   ├── code-reviewer/
│   │   └── SKILL.md
│   └── pdf-processor/
│       ├── SKILL.md
│       └── scripts/
├── commands/                 # Skills 作为平面 .md 文件
│   ├── status.md
│   └── logs.md
├── agents/                   # Subagent 定义
│   ├── security-reviewer.md
│   ├── performance-tester.md
│   └── compliance-checker.md
├── output-styles/            # 输出样式定义
│   └── terse.md
├── themes/                   # 颜色主题定义
│   └── dracula.json
├── monitors/                 # 后台 monitor 配置
│   └── monitors.json
├── hooks/                    # Hook 配置
│   ├── hooks.json           # 主 hook 配置
│   └── security-hooks.json  # 其他 hooks
├── bin/                      # 添加到 PATH 的 plugin 可执行文件
│   └── my-tool               # 在 Bash tool 中可作为裸命令调用
├── settings.json            # plugin 的默认设置
├── .mcp.json                # MCP server 定义
├── .lsp.json                # LSP server 配置
├── scripts/                 # Hook 和实用脚本
│   ├── security-scan.sh
│   ├── format-code.py
│   └── deploy.js
├── LICENSE                  # 许可证文件
└── CHANGELOG.md             # 版本历史
```

<Warning>
  `.claude-plugin/` 目录包含 `plugin.json` 文件。所有其他目录（commands/、agents/、skills/、output-styles/、themes/、monitors/、hooks/）必须在 plugin 根目录，而不是在 `.claude-plugin/` 内。
</Warning>

### 文件位置参考

| 组件                | 默认位置                         | 目的                                                                                                                        |
| :---------------- | :--------------------------- | :------------------------------------------------------------------------------------------------------------------------ |
| **清单**            | `.claude-plugin/plugin.json` | Plugin 元数据和配置（可选）                                                                                                         |
| **Skills**        | `skills/`                    | 具有 `<name>/SKILL.md` 结构的 Skills                                                                                           |
| **Commands**      | `commands/`                  | Skills 作为平面 Markdown 文件。新 plugins 使用 `skills/`                                                                            |
| **Agents**        | `agents/`                    | Subagent Markdown 文件                                                                                                      |
| **Output styles** | `output-styles/`             | 输出样式定义                                                                                                                    |
| **Themes**        | `themes/`                    | 颜色主题定义                                                                                                                    |
| **Hooks**         | `hooks/hooks.json`           | Hook 配置                                                                                                                   |
| **MCP servers**   | `.mcp.json`                  | MCP server 定义                                                                                                             |
| **LSP servers**   | `.lsp.json`                  | 语言服务器配置                                                                                                                   |
| **Monitors**      | `monitors/monitors.json`     | 后台 monitor 配置                                                                                                             |
| **Executables**   | `bin/`                       | 添加到 Bash tool 的 `PATH` 的可执行文件。此处的文件在 plugin 启用时可作为任何 Bash tool 调用中的裸命令调用                                                  |
| **Settings**      | `settings.json`              | 启用 plugin 时应用的默认配置。目前仅支持 [`agent`](/zh-CN/sub-agents) 和 [`subagentStatusLine`](/zh-CN/statusline#subagent-status-lines) 键 |

***

## CLI 命令参考

Claude Code 提供了用于非交互式 plugin 管理的 CLI 命令，对脚本和自动化很有用。

### plugin install

从可用市场安装 plugin。

```bash theme={null}
claude plugin install <plugin> [options]
```

**参数：**

* `<plugin>`：Plugin 名称或 `plugin-name@marketplace-name` 用于特定市场

**选项：**

| 选项                    | 描述                              | 默认值    |
| :-------------------- | :------------------------------ | :----- |
| `-s, --scope <scope>` | 安装范围：`user`、`project` 或 `local` | `user` |
| `-h, --help`          | 显示命令帮助                          |        |

范围确定将已安装的 plugin 添加到哪个设置文件。例如，`--scope project` 写入 `.claude/settings.json` 中的 `enabledPlugins`，使 plugin 对克隆项目存储库的每个人都可用。

**示例：**

```bash theme={null}
# 安装到用户范围（默认）
claude plugin install formatter@my-marketplace

# 安装到项目范围（与团队共享）
claude plugin install formatter@my-marketplace --scope project

# 安装到本地范围（gitignored）
claude plugin install formatter@my-marketplace --scope local
```

### plugin uninstall

删除已安装的 plugin。

```bash theme={null}
claude plugin uninstall <plugin> [options]
```

**参数：**

* `<plugin>`：Plugin 名称或 `plugin-name@marketplace-name`

**选项：**

| 选项                    | 描述                                        | 默认值    |
| :-------------------- | :---------------------------------------- | :----- |
| `-s, --scope <scope>` | 从范围卸载：`user`、`project` 或 `local`          | `user` |
| `--keep-data`         | 保留插件的[持久数据目录](#persistent-data-directory) |        |
| `-h, --help`          | 显示命令帮助                                    |        |

**别名：** `remove`、`rm`

默认情况下，从最后一个剩余范围卸载也会删除插件的 `${CLAUDE_PLUGIN_DATA}` 目录。使用 `--keep-data` 保留它，例如在测试新版本后重新安装时。

### plugin enable

启用已禁用的 plugin。

```bash theme={null}
claude plugin enable <plugin> [options]
```

**参数：**

* `<plugin>`：Plugin 名称或 `plugin-name@marketplace-name`

**选项：**

| 选项                    | 描述                                | 默认值    |
| :-------------------- | :-------------------------------- | :----- |
| `-s, --scope <scope>` | 要启用的范围：`user`、`project` 或 `local` | `user` |
| `-h, --help`          | 显示命令帮助                            |        |

### plugin disable

禁用 plugin 而不卸载它。

```bash theme={null}
claude plugin disable <plugin> [options]
```

**参数：**

* `<plugin>`：Plugin 名称或 `plugin-name@marketplace-name`

**选项：**

| 选项                    | 描述                                | 默认值    |
| :-------------------- | :-------------------------------- | :----- |
| `-s, --scope <scope>` | 要禁用的范围：`user`、`project` 或 `local` | `user` |
| `-h, --help`          | 显示命令帮助                            |        |

### plugin update

将 plugin 更新到最新版本。

```bash theme={null}
claude plugin update <plugin> [options]
```

**参数：**

* `<plugin>`：Plugin 名称或 `plugin-name@marketplace-name`

**选项：**

| 选项                    | 描述                                          | 默认值    |
| :-------------------- | :------------------------------------------ | :----- |
| `-s, --scope <scope>` | 要更新的范围：`user`、`project`、`local` 或 `managed` | `user` |
| `-h, --help`          | 显示命令帮助                                      |        |

***

### plugin list

列出已安装的 plugins 及其版本、源市场和启用状态。

```bash theme={null}
claude plugin list [options]
```

**选项：**

| 选项            | 描述                            | 默认值 |
| :------------ | :---------------------------- | :-- |
| `--json`      | 输出为 JSON                      |     |
| `--available` | 包括来自市场的可用 plugins。需要 `--json` |     |
| `-h, --help`  | 显示命令帮助                        |     |

### plugin tag

为当前目录中的 plugin 创建发布 git 标签。从 plugin 的文件夹内运行。请参阅[标记 plugin 发布](/zh-CN/plugin-dependencies#tag-plugin-releases-for-version-resolution)。

```bash theme={null}
claude plugin tag [options]
```

**选项：**

| 选项            | 描述                   | 默认值 |
| :------------ | :------------------- | :-- |
| `--push`      | 创建标签后将其推送到远程         |     |
| `--dry-run`   | 打印将被标记的内容而不创建标签      |     |
| `-f, --force` | 即使工作树是脏的或标签已存在，也创建标签 |     |
| `-h, --help`  | 显示命令帮助               |     |

***

## 调试和开发工具

### 调试命令

使用 `claude --debug` 查看 plugin 加载详情：

这显示：

* 正在加载哪些 plugins
* plugin 清单中的任何错误
* Skill、agent 和 hook 注册
* MCP server 初始化

### 常见问题

| 问题                                  | 原因                         | 解决方案                                                                                                                            |
| :---------------------------------- | :------------------------- | :------------------------------------------------------------------------------------------------------------------------------ |
| Plugin 未加载                          | 无效的 `plugin.json`          | 运行 `claude plugin validate` 或 `/plugin validate` 检查 `plugin.json`、skill/agent/command frontmatter 和 `hooks/hooks.json` 的语法和架构错误 |
| Skills 未出现                          | 目录结构错误                     | 确保 `skills/` 或 `commands/` 在根目录，而不是在 `.claude-plugin/` 中                                                                        |
| Hooks 未触发                           | 脚本不可执行                     | 运行 `chmod +x script.sh`                                                                                                         |
| MCP server 失败                       | 缺少 `${CLAUDE_PLUGIN_ROOT}` | 对所有 plugin 路径使用变量                                                                                                               |
| 路径错误                                | 使用了绝对路径                    | 所有路径必须是相对的，并以 `./` 开头                                                                                                           |
| LSP `Executable not found in $PATH` | 语言服务器未安装                   | 安装二进制文件（例如，`npm install -g typescript-language-server typescript`）                                                              |

### 示例错误消息

**清单验证错误**：

* `Invalid JSON syntax: Unexpected token } in JSON at position 142`：检查缺少的逗号、多余的逗号或未引用的字符串
* `Plugin has an invalid manifest file at .claude-plugin/plugin.json. Validation errors: name: Required`：缺少必需字段
* `Plugin has a corrupt manifest file at .claude-plugin/plugin.json. JSON parse error: ...`：JSON 语法错误

**Plugin 加载错误**：

* `Warning: No commands found in plugin my-plugin custom directory: ./cmds. Expected .md files or SKILL.md in subdirectories.`：命令路径存在但不包含有效的命令文件
* `Plugin directory not found at path: ./plugins/my-plugin. Check that the marketplace entry has the correct path.`：marketplace.json 中的 `source` 路径指向不存在的目录
* `Plugin my-plugin has conflicting manifests: both plugin.json and marketplace entry specify components.`：删除重复的组件定义或删除 marketplace 条目中的 `strict: false`

### Hook 故障排除

**Hook 脚本未执行**：

1. 检查脚本是否可执行：`chmod +x ./scripts/your-script.sh`
2. 验证 shebang 行：第一行应该是 `#!/bin/bash` 或 `#!/usr/bin/env bash`
3. 检查路径是否使用 `${CLAUDE_PLUGIN_ROOT}`：`"command": "${CLAUDE_PLUGIN_ROOT}/scripts/your-script.sh"`
4. 手动测试脚本：`./scripts/your-script.sh`

**Hook 未在预期事件上触发**：

1. 验证事件名称是否正确（区分大小写）：`PostToolUse`，而不是 `postToolUse`
2. 检查匹配器模式是否与您的工具匹配：`"matcher": "Write|Edit"` 用于文件操作
3. 确认 hook 类型有效：`command`、`http`、`mcp_tool`、`prompt` 或 `agent`

### MCP server 故障排除

**Server 未启动**：

1. 检查命令是否存在且可执行
2. 验证所有路径是否使用 `${CLAUDE_PLUGIN_ROOT}` 变量
3. 检查 MCP server 日志：`claude --debug` 显示初始化错误
4. 在 Claude Code 外手动测试 server

**Server 工具未出现**：

1. 确保 server 在 `.mcp.json` 或 `plugin.json` 中正确配置
2. 验证 server 是否正确实现 MCP 协议
3. 检查调试输出中的连接超时

### 目录结构错误

**症状**：Plugin 加载但组件（skills、agents、hooks）缺失。

**正确结构**：组件必须在 plugin 根目录，而不是在 `.claude-plugin/` 内。只有 `plugin.json` 属于 `.claude-plugin/`。

```text theme={null}
my-plugin/
├── .claude-plugin/
│   └── plugin.json      ← 仅清单在此处
├── commands/            ← 在根级别
├── agents/              ← 在根级别
└── hooks/               ← 在根级别
```

如果您的组件在 `.claude-plugin/` 内，请将它们移到 plugin 根目录。

**调试清单**：

1. 运行 `claude --debug` 并查找"loading plugin"消息
2. 检查每个组件目录是否在调试输出中列出
3. 验证文件权限允许读取 plugin 文件

***

## 分发和版本管理参考

### 版本管理

Claude Code 使用 plugin 的版本作为缓存键，以确定是否有可用的更新。当你运行 `/plugin update` 或自动更新触发时，Claude Code 会计算当前版本，如果与已安装的版本匹配，则跳过更新。

版本从以下第一个设置的字段解析：

1. plugin 的 `plugin.json` 中的 `version` 字段
2. plugin 的 `marketplace.json` 中的市场条目中的 `version` 字段
3. plugin 源的 git 提交 SHA，用于 git 托管市场中的 `github`、`url`、`git-subdir` 和相对路径源
4. `unknown`，用于 `npm` 源或不在 git 仓库内的本地目录

这为你提供了两种方式来对 plugin 进行版本管理：

| 方法            | 如何操作                                     | 更新行为                                                        | 最适合                 |
| :------------ | :--------------------------------------- | :---------------------------------------------------------- | :------------------ |
| **显式版本**      | 在 `plugin.json` 中设置 `"version": "2.1.0"` | 用户仅在你提升此字段时获得更新。推送新提交而不提升它没有效果，`/plugin update` 报告"已是最新版本"。 | 具有稳定发布周期的已发布 plugin |
| **提交 SHA 版本** | 从 `plugin.json` 和市场条目中省略 `version`       | 用户在每次对 plugin 的 git 源进行新提交时获得更新                             | 正在积极开发的内部或团队 plugin |

<Warning>
  如果你在 `plugin.json` 中设置 `version`，你必须在每次想让用户接收更改时提升它。仅推送新提交是不够的，因为 Claude Code 看到相同的版本字符串并保留缓存副本。如果你迭代速度很快，请不设置 `version`，以便改用 git 提交 SHA。
</Warning>

如果你使用显式版本，请遵循[语义版本控制](https://semver.org)（`MAJOR.MINOR.PATCH`）：为破坏性更改提升 MAJOR，为新功能提升 MINOR，为错误修复提升 PATCH。在 `CHANGELOG.md` 中记录更改。

***

## 另请参阅

* [Plugins](/zh-CN/plugins) - 教程和实际用法
* [Plugin marketplaces](/zh-CN/plugin-marketplaces) - 创建和管理市场
* [Skills](/zh-CN/skills) - Skill 开发详情
* [Subagents](/zh-CN/sub-agents) - Agent 配置和功能
* [Hooks](/zh-CN/hooks) - 事件处理和自动化
* [MCP](/zh-CN/mcp) - 外部工具集成
* [Settings](/zh-CN/settings) - Plugins 的配置选项
