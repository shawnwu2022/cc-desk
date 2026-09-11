# DevTools JSON 粘贴完整性修复

## 问题

Windows 版 CC Desk 在 Claude Code 输入框粘贴较大的 Chrome DevTools JSON 时，曾出现三类现象：

- 内容被逐行解释，无法作为一次粘贴事务处理；
- 尾部看似截断；
- 输入框泄漏字面量 `[201~`。

诊断确认，剪贴板读取、行尾规范化、前端 payload、Tauri IPC 与 Rust `pty_input` 入口均能收到完整正文和 `ESC[200~` / `ESC[201~`。故障来自 CC Desk 对 Windows ConPTY 输入协议的额外改写。

## 根因

0.17.3 和 0.17.4 曾把 bracketed-paste 标记中的 ESC 改写成 Win32 `INPUT_RECORD` 编码，并在标记和正文之间执行 `FlushFileBuffers`。该编码依赖 win32-input-mode 协商；CC Desk 不应在未建立完整协商状态的情况下模拟另一套键盘协议。

在 Windows 10 build 19045 上，这会导致结束标记的 ESC 被消费，而 `[201~` 作为普通文本泄漏到 Claude Code 输入框。

## 0.17.5 最终实现

- `buildPastePayload` 只把 CRLF/CR 规范为 LF，不压缩、不解析重写 JSON。
- Windows 完整 bracketed-paste frame 使用一次原始 `write_all` 投递：

  ```text
  ESC[200~ + 完整正文 + ESC[201~
  ```

- 不再改写 ESC，不在 frame 内主动分块或插入 flush，不维护本地 `portable-pty` fork。
- 普通键盘输入、图片粘贴快捷键和非 Windows PTY 保持原路径。
- 每个 PTY 继续使用独立 writer 锁；写入错误向上传播，不自动重发可能已写入的前缀。

## 自动化验证

- 前端测试覆盖 `buildPastePayload`、Ctrl+V 路径和捕获阶段 DOM paste 路径。
- Rust 测试要求 Windows 完整 frame 只产生一次未经改写的写入。
- 真实 Claude Code 验收使用隔离配置和 `UserPromptSubmit` hook 捕获完整提交正文，阻止模型处理，不使用用户凭据。
- 合成样本覆盖合法嵌套 JSON、大整数文本、中文、emoji、转义字符、超过 128 KiB 的正文、4 KiB 临界位置和连续粘贴。

## Windows 10 实机验收

用户环境：

```text
Windows 10 22H2 / build 19045
Claude Code 2.1.268 native claude.exe
CC Desk 0.17.5
```

原始业务 JSON 不进入仓库；仅记录隐私安全的结构度量：

```text
规范化正文：106002 UTF-8 bytes
字符数：105002
LF：3578
完整 frame：106014 bytes
opening offset：0
closing offset：106008
transport：windows-conpty-raw-frame
write：success
```

实机结果：

- 不再出现 `[200~` / `[201~` 泄漏；
- 不再逐行执行 JSON；
- Claude Code 正确显示一个或多个原生 `[Pasted text #N +X lines]` 占位符；
- 展开后可到达 JSON 尾部、最终字段和闭合花括号；
- 同一次 Ctrl+V 只有一组 `input` / `write` 诊断记录，没有前端重复投递。

Claude Code 可能将一次大型粘贴按 stdin 内部读取块显示成多个连续的 `[Pasted text #N]`。这是原生 TUI 的展示行为，不代表 CC Desk 重复粘贴。

## 验收边界

上述证据确认本次报告的 ConPTY 起止标记泄漏、逐行执行和可见尾部截断问题已经修复。不同 Windows 构建、WSL/SSH 嵌套链路以及未来 Claude Code 版本仍需依靠回归测试持续验证；CC Desk 不应重新实现 Claude Code 的输入编辑器或依赖其私有内部结构。
