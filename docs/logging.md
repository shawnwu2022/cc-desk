# 日志系统

应用日志按日期保存到 `~/.cc-box/logs/`，前端和后端统一写入同一套文件。

## 文件结构

```
~/.cc-box/logs/
├── 2026-04-28.log           # 当日全部日志（INFO 及以上）
├── 2026-04-28.error.log     # 当日错误日志（WARN 及以上）
└── ...
```

## 日志级别

| 模式 | 保存级别 | stderr 输出 |
|------|---------|------------|
| debug | Debug 及以上 | 有 |
| release | Info 及以上 | 无 |

## 文件命名与轮转

- 文件名格式：`{YYYY-MM-DD}.log` / `{YYYY-MM-DD}.error.log`
- 每天自动使用新文件
- 启动时后台清理超过 **7 天** 的旧日志文件

## 日志格式

```
[2026-04-28 14:30:00.123][INFO] [cc_box::pty] Claude CLI spawned successfully
[2026-04-28 14:30:01.456][ERROR] [cc_box::checks] [Check Failed] Claude CLI: ...
[2026-04-28 14:30:02.789][WARN] [Frontend] startTab: blocked by isPtyStarting
```

格式：`[时间戳][级别] [模块路径] 消息内容`

前端日志的模块路径统一为 `[Frontend]`。

## 前端日志接口

```typescript
import { logMessage } from '@/api/tauri'

logMessage('error', `startTab failed: ${err}`)
logMessage('warn', `startNewSession: blocked`)
logMessage('info', 'user clicked new session')
logMessage('debug', 'debug info')
```

参数：`level: 'error' | 'warn' | 'info' | 'debug'`，`message: string`

## 代码结构

```
src-tauri/src/logger.rs      # FileLogger 实现、文件管理、旧日志清理
src-tauri/src/commands.rs    # log_message command（前端→后端桥接）
src-tauri/src/lib.rs         # setup 阶段调用 logger::init()
src/api/tauri.ts             # logMessage 前端封装
```

## 可靠性

- 每条日志写入后立即 `flush`
- 日志文件以追加模式打开，多次启动不覆盖
