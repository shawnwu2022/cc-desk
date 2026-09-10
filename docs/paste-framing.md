# Windows DevTools 粘贴截断：传输修复与验收边界

## 故障与证据

前端已通过 `buildPastePayload` 包装 `ESC[200~` / `ESC[201~`，但 Windows ConPTY 对使用 `ReadConsoleInputW` 的客户端会解析 CSI，而不是原样转发未知输入序列。两端的粘贴标记会被吞掉。这样正文即使完整到达 stdin，CLI 也可能把各个读取块当作独立键盘输入，而非一个粘贴事务。

2026-09-10 在 Windows Server 2022 / portable-pty 0.8.1 / Node 20 raw stdin 上，用实际前端构造函数进行了对照实验：

| 路径 | 期望 UTF-8 字节数（含标记） | 实收 | 起止标记 |
| --- | ---: | ---: | --- |
| 旧生产 payload 直接写入 | 12439 | 12427 | 均丢失 |
| ESC 使用 ConPTY Unicode 按键编码 | 12439 | 12439 | 均保留；完整哈希一致 |

复现记录：GitHub Actions run `34449890503`，job `102782992068`。

旧 `paste_transport` 测试为了兼容不同 ConPTY 行为，允许标记消失并只对正文求哈希。它能验证正文传输，但不能证明 CLI 收到正确的粘贴事务。新测试不能再接受这种降级。

## 修复位置

修复放在 `src-tauri/src/pty.rs::write_pty_data`，不新增一套前端编辑器，不绕过真实 Claude CLI：

1. 仅 Windows、仅同一写请求中完整的 bracketed paste 帧进入编码。
2. 帧中的 ESC 替换为 `ESC[0;0;27;1;0;1_`：虚拟键 0、扫描码 0、Unicode ESC、按下、无修饰键、重复一次。ConPTY 将其变为字符输入事件，子进程仍收到原始 ESC。
3. 正文的 UTF-8 字节、LF、空格、缩进不在 Rust 层改写；ESC 字符也作为原字符交付。保留既有 4 KiB 分块、完整写入和每 PTY 独立锁。
4. 普通键、方向键、终端应答、图片粘贴快捷键以及所有非 Windows 输入保持原路径。

前端现有的 CRLF/CR -> LF 规范化、合法 JSON 结构性空白压缩仍保留。这次不扩大范围重写它们；JSON 大整数不会经过 parse/stringify 往返。快捷键与原生菜单都通过同一个后端 writer 获得修复。

这不是“提高缓冲区上限”或“调大 sleep”。也不根据是否观察到 `?9001h` 决定是否修复：本次实际 ConPTY 环境没有发出该模式请求，仍能正确解析 Unicode 按键事件。

## 自动化契约

- `tests/utils/devtoolsPaste.test.ts`：真实前端构造函数和两种粘贴入口，对齐共享黄金样本。
- `src-tauri/src/tests/paste_framing.rs`：调用真实生产 writer，经真 ConPTY/Node raw stdin，同时校验 UTF-8 字节长度、完整内容哈希及起止标记。覆盖中文、emoji、CRLF 规范化结果、非 JSON 对象、JSON 大整数、ANSI 字面内容、超过 128 KiB 的多行日志、分块边界和连续粘贴。
- 子进程就绪后才发送；带独立 watchdog；失败与成功均回收子进程。
- 日志仅记录生成样本名称、字节数和验证结果，不记录用户剪贴板正文。

## 尚需真实环境验收

自动化探针不是 Claude 编辑器本身。最终应在用户实际 Windows 版本和 Claude Code 版本上，分别用 Ctrl+V、Shift+Insert/菜单粘贴真实 DevTools 日志，核对首部、尾部、换行和发送后的用户消息是否完整。仅显示 `[Pasted text ...]` 折叠摘要不代表截断。

本轮直接验证目标为 Windows Server 2022 的系统 ConPTY。Windows 10/11 不同构建、WSL/SSH 嵌套链路以及真实 Claude 编辑器需要各自验收，不能由一个 runner 结果直接宣称全平台通过。

## 参考

- Microsoft 输入协议：`https://github.com/microsoft/terminal/blob/main/doc/specs/%234999%20-%20Improved%20keyboard%20handling%20in%20Conpty.md`
- Microsoft `InputStateMachineEngine.cpp`：GenericKeys 与 Win32KeyboardInput 分支。
- libuv `src/win/tty.c`：ReadConsoleInputW / UnicodeChar 到 UTF-8 的输入路径。
