# DevTools JSON 粘贴完整性修复

## 已确认的问题

旧代码将 JSON 压缩成单行，只是规避，不保证大对象完整进入 Claude 编辑器。
Windows Server 2022 / portable-pty 0.8.1 / Node 20 的真实输入探针中，原始
bracketed paste 起止标记被吞掉（期望 14012 字节，收到 14000）。
此后还复现了控制序列跨 ConPTY 内部读边界损坏；应用层按完整单元切块仍不足。
不能把调整 sleep、单行化、出现折叠标签或正文单独哈希通过当作修复验收。

## 实际修改

- buildPastePayload 不再调用 JSON 压缩。只保留原有 CRLF/CR -> LF 规范化；
  换行、缩进、大整数数字、字符串转义、首尾内容不被压缩或截断。
- Windows 完整粘贴帧中的 ESC 使用 Unicode 按键事件编码，每个事件独立写入；
  事件与正文之间真实排空 ConPTY 输入管道。正文按 UTF-8 边界分块，不复制整段。
- 精确 vendor portable-pty 0.8.1，仅包装 ConPTY input writer 的 flush。
  不修改全局 filedescriptor，不使用测试专属传输实现，不改 Unix 输出/输入。
  普通键和图片粘贴键不编码；Windows writer 的 flush 现在会等待管道读取。
- 每 PTY 独立 writer 锁保持不变；写入失败向上传播，不自动重发部分内容。

## 自动化证据及边界

tests/utils/devtoolsPaste.test.ts 对同一套黄金样本逐一验证 payload 构造、
commitPaste 快捷键入口、捕获阶段原生菜单入口，保留原始 JSON 格式。
src-tauri/src/tests/paste_framing.rs 调用实际生产 writer，经真实 ConPTY，
校验包含标记的完整 UTF-8 字节数及内容哈希。
样本含 64/256/800 行对象数组的合法嵌套 JSON、大整数、中文、emoji、
145 KB 日志、38 个 4 KiB 附近边界、800 个 ESC 与连续粘贴。

这些测试证明上述样本的前端投递与 Windows stdin 传输契约，**不等同于
用户真实 Chrome 剪贴板和具体 Claude Code 版本的编辑器已验收**。
旧 paste_claude_e2e 探针存在折叠标签直接判通过的历史逻辑，不作为本修复
的真实编辑器完整性证据。真实验收必须提取完整编辑器/提交正文进行比较。

Windows 10/11 不同构建、WSL/SSH 嵌套链路、原始 U+009B 等特殊控制字符
未由这一 Windows Server 2022 runner 自动验证。此分支尚未发布安装包。

## 复现记录

- 旧生产写入失败：Actions run 34451840475。
- 隔离 ESC + 真排空成功；关闭排空再次失败：Actions run 34454015996。
- 当前全部生产路径回归：Actions run 34455514488。

Microsoft 协议：microsoft/terminal 的 #4999 Improved keyboard handling in Conpty。
FlushFileBuffers 语义：Microsoft Learn /windows/win32/api/fileapi/nf-fileapi-flushfilebuffers。

## 真实 Claude 提交正文验收

- Windows Server 2022，Claude Code npm 包版本 `2.1.267`，Actions run `34456569102`。
- 使用真实 `buildPastePayload` 输出、生产 Rust writer、真实 Claude 交互输入框。
- 隔离的 UserPromptSubmit hook 捕获完整提交正文并阻止模型处理；dummy key + loopback endpoint 双重避免使用用户凭据和真实模型请求。
- 对 64/256/800 个嵌套对象的 JSON 逐字节比对全文，不允许折叠标签、只有首尾或只有长度一致代替验收。
- 此结果仍不覆盖用户自己的 Chrome 剪贴板和不同 Windows/Claude 版本；原始失败样本需要在同一构建上复核。


## 0.17.5 架构修正：停止模拟 Win32 键盘事件

同一类大 JSON 在 Windows Terminal、Windows 10 build 19045、native Claude Code
2.1.267/2.1.268 中会被正确识别为 `Pasted text`；CC Desk 0.17.3 和 0.17.4
则泄漏字面 `[201~`。诊断证明完整原文与两端标记在进入 Rust 前均未丢失，
因此故障来自 CC Desk 自行改写 ConPTY 输入协议。

0.17.5 删除 Win32 INPUT_RECORD ESC 编码、逐段 FlushFileBuffers 以及 portable-pty
本地 flush fork。Windows 粘贴与 Windows Terminal 对齐：将
`ESC[200~ + body + ESC[201~` 作为一个未经改写的逻辑 write_all 提交。

旧 Node raw-stdin 探针没有启用 Claude Code 的控制台输入模式，保留为 ignored
诊断而不再充当发布门禁。正式门禁使用真实 Claude Code、隔离配置和
UserPromptSubmit 全文捕获，对提交正文逐字节比较。
