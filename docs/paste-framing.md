# DevTools JSON object 粘贴截断：调查记录（尚未修复）

## 当前状态

**未修复、未发布。** 用户确认从 Chrome DevTools 复制的 JSON object 在 Claude 输入框中仍会截断；不能将问题缩小为非 JSON 日志，也不能将 JSON 压缩或 PTY 写入成功当成端到端修复。

本分支当前包含调查探针和回归样本，尚未修改生产 writer。`codex/focus-claude-workspace` 的 PR #6 不包含这里的粘贴修复；不得把它的 CI 成功用于证明本问题已经解决。

## 已有证据

- 原生产链路是 `buildPastePayload` -> `pty_input` -> `write_pty_data` -> ConPTY -> CLI。前端对合法多行 JSON 做结构性空白压缩，并按终端模式添加 `ESC[200~` / `ESC[201~`。
- Windows Server 2022 / portable-pty 0.8.1 / Node raw stdin 的既有对照中，12439 字节的成帧输入只收到 12427 字节，两端标记丢失（Actions run `34449890503`，job `102782992068`）。这证明了该测试环境的协议缺陷，尚不能证明用户的整个截断路径只有这一个原因。
- 旧 `paste_transport` 测试容忍标记消失，只校验正文，因此不能证明 CLI 接收到完整粘贴事务。
- 单独编码 ESC 的实验在短样本上成功，但不加真正的 pipe drain 时，分块边界 `escape-offset-4077` 出现等长内容哈希不一致（run `34454015996`，job `102796065118`）。该实验工作流显式收集各变体退出码，最后退出 0，因此工作流显示 success **不代表所有实验通过**；两个 no-drain 变体实际退出 101。
- 将整段输入改成 Unicode 按键记录的候选同样失败（run `34454572559`，job `102797860868`）：连续粘贴样本期望 43 字节，实际 49；其他 Unicode 和分块样本也失败。这个候选没有合入生产代码。
- 接收端诊断 run `34454932075` / job `102799000975` 进一步在生成样本中捕获了混入正文的协议片段。说明不能只看字符串长度，更不能仅靠增加 sleep 或扩大缓冲区宣布修复。

## 后续修复的硬性验收条件

1. 使用一份完整、合法的大 JSON object，包含嵌套对象和数组、长字符串、转义字符、中文、emoji 与大整数文本。不得通过重复多个 JSON 文档来代替单个合法 JSON，也不能通过 JS parse/stringify 往返改变数字。
2. 固定用户实际复制方式、Claude Code 版本、CC Desk 构建和 OS 构建；以脱敏原文文件及截断结果为真实回归样本。
3. 分别核对系统剪贴板文本、前端处理结果、实际接收正文和帧边界。诊断默认只输出生成样本数据或字节数/状态，不记录用户剪贴板正文及敏感信息。
4. 同一份 JSON 覆盖快捷键与系统菜单粘贴，覆盖 4 KiB 边界和超过 128 KiB 的内容；短文本、图片粘贴键和非 Windows 普通输入不能回归。
5. 必须核对真实 Claude 输入缓冲区，而不是用 Node stdin 完整替代最终验收；优先通过原生外部编辑器回读。不发送用户数据到真实 Provider，不自动提交 Enter，不修改原生认证配置。
6. `[Pasted text]` 折叠显示本身不算丢失，但不能因此推翻用户实际观察到的截断；必须比较完整内容。

## 临时无损绕行

使用 Claude Code 的原生外部编辑器入口（默认 Ctrl+G），在编辑器中粘贴后保存关闭；或者将系统剪贴板的原始文本以 UTF-8 保存为文件，再由 Claude Code 按正常权限读取。不要先将 JSON parse/stringify，也不要把“压缩成一行”当作可靠方案。

这只是绕行，不是 CC Desk 的直接粘贴修复。

## 参考

- Claude Code interactive mode / terminal configuration 官方文档。
- Microsoft `doc/specs/#4999 - Improved keyboard handling in Conpty.md`。
- Microsoft `InputStateMachineEngine.cpp` 的 Generic 与 Win32KeyboardInput 分支。
- libuv `src/win/tty.c` 的 ReadConsoleInputW / UnicodeChar -> UTF-8 输入路径。
