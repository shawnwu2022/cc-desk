# Hook 监控系统

通过 Claude Code Plugin 机制注入 Hook，实时采集 Claude 运行时状态，经 HTTP 发送到 CC-Box 后端，再推送到前端展示。

## 设计理念

Hook 系统只采集**对 GUI 有意义**的信息：

1. **运行状态** — 推导 Claude 当前在做什么（思考/执行工具/等待用户/错误等），用于 SessionItem 指示灯
2. **模型** — 从 SessionStart 提取，保留供未来使用
3. **session_id** — 关联手动创建的 Claude 会话

其他事件详情不在后端提取。原始 JSON 仍完整传输，架构保持可扩展。

## 架构总览

```
┌─ CC-Box 启动 ────────────────────────────────────────────────┐
│  1. 生成 plugin 文件到 ~/.cc-box/claude-plugin/               │
│  2. 启动 axum HTTP 服务器（127.0.0.1:随机端口）                │
│  3. 端口写入 OnceLock，供 PTY spawn 读取                      │
└──────────────────────────────────────────────────────────────┘

┌─ 用户启动会话 ───────────────────────────────────────────────┐
│  pty.rs spawn_claude()                                        │
│  ├─ 命令追加：--plugin-dir ~/.cc-box/claude-plugin            │
│  └─ 环境变量：                                                │
│       CC_BOX_HOOK_PORT=<端口>     ← 共享，一个 CC-Box 实例一个 │
│       CC_BOX_SESSION_ID=<pty_id>  ← 每个 PTY 唯一             │
└──────────────────────────────────────────────────────────────┘

┌─ Claude 运行中 ─────────────────────────────────────────────┐
│  Hook 触发 → report-hook.sh 执行                              │
│  ├─ 检查 CC_BOX_HOOK_PORT（未设置则 exit 0）                  │
│  ├─ 检查 curl 可用性                                          │
│  ├─ 读取 stdin（Claude 传入的 JSON）                          │
│  └─ curl POST → http://127.0.0.1:$CC_BOX_HOOK_PORT/hook      │
│         header: X-CC-Box-Session: $CC_BOX_SESSION_ID         │
│         body: <hook 事件 JSON>                                │
└──────────────────────────────────────────────────────────────┘

┌─ CC-Box Rust 后端 ──────────────────────────────────────────┐
│  hook_server.rs handle_hook()                                 │
│  ├─ 从 header 读 pty_id（区分多终端）                         │
│  ├─ 从 body 读 session_id                                     │
│  ├─ 建立 session_id ↔ pty_id 映射                             │
│  ├─ hook_events.rs：推导状态 + 提取模型                        │
│  └─ emit "hook-event" → 前端                                  │
└──────────────────────────────────────────────────────────────┘

┌─ Vue 前端 ───────────────────────────────────────────────────┐
│  stores/hook.ts                                               │
│  ├─ 监听 "hook-event"                                         │
│  ├─ 按 ptyId 路由到对应终端 tab                                │
│  ├─ 更新运行状态（state）                                      │
│  ├─ 记录模型（SessionStart 时）                                │
│  └─ 2 分钟陈旧检测：活跃状态超时自动降级为 idle                 │
│                                                               │
│  SessionItem.vue ← 从 hookStore 获取 claudeState → 指示灯样式  │
└──────────────────────────────────────────────────────────────┘
```

## 多终端区分

| 标识 | 来源 | 用途 |
|------|------|------|
| `CC_BOX_SESSION_ID`（= PTY ID） | spawn 时注入环境变量 | 标识**哪个终端 tab** |
| `session_id` | Claude hook 事件 JSON | 标识 **Claude 内部会话** |
| `CC_BOX_HOOK_PORT` | CC-Box 启动时分配 | **HTTP 服务器端口**（所有终端共享） |

## Plugin 文件

源文件位于 `src-tauri/plugin/`，编译时通过 `include_str!()` 嵌入二进制，运行时写入 `~/.cc-box/claude-plugin/`。

```
src-tauri/plugin/                       运行时目标 ~/.cc-box/claude-plugin/
├── .claude-plugin/                     ├── .claude-plugin/
│   └── plugin.json                     │   └── plugin.json
├── hooks/                              ├── hooks/
│   └── hooks.json     ← 11 个事件定义   │   └── hooks.json
└── scripts/                            └── scripts/
    └── report-hook.sh                     └── report-hook.sh
```

**加载方式**：`--plugin-dir` 按 session 加载，仅在 CC-Box 启动的 Claude 会话中生效。
**更新方式**：修改 `src-tauri/plugin/` 下的文件，重新编译。`hook_config.rs` 的 `write_if_changed` 检测变化后覆盖。

## 采集的事件与数据

所有事件提取定义集中在 `src-tauri/src/hook_events.rs`，前端类型在 `src/types/hook.ts`，一一对应。

### 已注册的 11 个事件

| 事件 | 提取数据 | 推导状态 |
|------|----------|----------|
| `SessionStart` | model | → idle |
| `SessionEnd` | — | → idle |
| `UserPromptSubmit` | — | → thinking |
| `PreToolUse` | — | → tool_executing |
| `PostToolUse` | — | → thinking |
| `PostToolUseFailure` | — | → thinking |
| `Stop` | — | → idle |
| `StopFailure` | — | → error |
| `Notification` | — | → waiting_permission / waiting_input |
| `SubagentStart` | — | → subagent_running |
| `SubagentStop` | — | → thinking |

仅 `SessionStart` 提取 `model` 字段。`derive_state` 还处理未注册但预留的 `PreCompact`/`PostCompact` 事件。

### 状态机

状态由 `derive_state()` 从事件类型无状态推导（不依赖前序状态），对丢事件具有容错性。

```
idle ──[UserPromptSubmit]──→ thinking
thinking ──[PreToolUse]──→ tool_executing ──[PostToolUse]──→ thinking
thinking ──[Notification:permission_prompt]──→ waiting_permission
thinking ──[Notification:idle_prompt]──→ waiting_input
thinking ──[Stop]──→ idle
any ──[SubagentStart]──→ subagent_running ──[SubagentStop]──→ thinking
any ──[StopFailure]──→ error
```

### 状态指示灯 UI

| 状态 | 颜色 | CSS 变量 | 动画 | 含义 |
|------|------|----------|------|------|
| thinking, tool_executing, subagent_running | 墨蓝 | `--status-info` | 温和脉冲 | Claude 正在处理 |
| waiting_permission, waiting_input | 琥珀金 | `--accent-gold` | 温和脉冲 | 等待用户操作 |
| idle, unknown | 墨绿 | `--status-success` | 温和脉冲 | 默认运行态 |
| error | 赭红 | `--status-error` | 温和脉冲 | 出错 |
| PTY 退出（当前活跃） | 浅灰 | `--text-tertiary` | 无 | 已停止 |

### 新增/修改事件的步骤

1. 在 `src-tauri/plugin/hooks/hooks.json` 注册新事件
2. 在 `src-tauri/src/hook_events.rs` 的 `extract_detail` + `derive_state` 添加分支
3. 在 `src/types/hook.ts` 添加对应类型
4. 如需影响 UI 指示灯，更新 `SessionItem.vue` 的 `dotClass` computed

## 前端数据结构

### HookEventPayload（从 Rust 后端 emit）

```typescript
interface HookEventPayload {
  ptyId: string | null
  sessionId: string | null
  eventName: string
  state: ClaudeState
  timestamp: number
  detail: HookEventDetail
}
```

### SessionHookState（Pinia store 按会话聚合）

```typescript
interface SessionHookState {
  ptyId: string
  sessionId?: string
  state: ClaudeState
  model?: string
  lastUpdatedAt: number
}
```

## 稳定性保障

**原则：Hook 是监控通道，不是核心功能。异常不能影响 CC-Box 和 Claude 正常运行。**

| 层级 | 保障措施 |
|------|----------|
| **脚本** | 始终 `exit 0`；curl 不可用时跳过；`--max-time 3` 限超时；hook `timeout: 5` |
| **HTTP 服务器** | 启动失败仅日志告警；handler 内 catch 所有错误；独立 tokio task |
| **PTY** | `--plugin-dir` 路径不存在时 Claude 忽略正常启动 |
| **前端** | 无 hook 事件时状态为 "unknown"，指示灯降级为默认样式 |
| **陈旧检测** | 2 分钟无更新的活跃状态自动降级为 idle（30 秒检测周期） |

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/hook_events.rs` | 事件数据结构、提取逻辑、状态推导 |
| `src-tauri/src/hook_server.rs` | HTTP 服务器：接收事件、路由、emit |
| `src-tauri/src/hook_config.rs` | Plugin 文件管理 |
| `src-tauri/plugin/` | Plugin 源文件（plugin.json + hooks.json + report-hook.sh） |
| `src/types/hook.ts` | 前端类型定义 |
| `src/stores/hook.ts` | Pinia store：监听事件、聚合状态、陈旧检测 |
| `src/components/sessions/SessionItem.vue` | 状态指示灯 UI |
