# 日志系统

## 更新诊断摘要

设置 → 关于 → 复制安全诊断摘要会生成白名单 JSON。新增更新通道、当前阶段、候选版本和最近固定错误码。原始异常、代理认证、路径、会话身份、正文和终端输出不进入摘要，不自动上传。CLI 版本当前明确标记 `not-collected`，尚未实现基于精确配置程序的安全版本采集；不能把本机开发探测版本当作用户运行版本，也不能据此阻止原生功能。

更新器只记录固定 `code/stage`。安装包大小和摘要或签名复核错误为 `verify`，传输与流长度错误为 `download`；官方发行及签名资产证据错误为 `provenance`。取消仅在后端确认安装前完成后显示成功，未知安装回执不会自动重放。

完整日志文件可能包含原有工程诊断，复制摘要不会读取或附加这些文件。不要将原始日志直接作为默认支持包上传。

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
[2026-04-28 14:30:00.123][INFO] [cc_desk::pty] Claude CLI spawned successfully
[2026-04-28 14:30:01.456][ERROR] [cc_desk::checks] [Check Failed] Claude CLI: ...
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
