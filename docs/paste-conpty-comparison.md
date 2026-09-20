# ConPTY 控制台后端单变量对照包

## 状态

这是候选兼容性方案的实机验证包，不是已经确认修复的正式版本。目标仍是直接 Ctrl+V 完整粘贴，不使用文件回填代替修复。

新证据：2026-09-20 14:28 的实机追踪显示 28037 字节参考正文与 Rust 接收正文严格相同（230 LF，其余统计控制字符为 0）；write 耗时 86112 微秒。下一次可见输入在 write 完成 8.658 秒后，为 3 字节、包含一个 ESC；没有原始字节，不能认定具体按键。日志尚未包含该次草稿或提交正文，不能仅据此宣称 ConPTY 或 Claude 的确切根因。

## 对照变量

`cc-desk-paste-trace.exe` 与上一份 0be6078 诊断包逐字相同（SHA-256 见 MANIFEST.json），不修改剪贴板、payload、分块、行尾、shell、CLI 版本或配置。此包仅在应用目录附带微软官方发布的一对 x64 `conpty.dll` / `OpenConsole.exe`。

portable-pty 0.8.1 上游的加载器优先查找 conpty.dll，找不到时回退 kernel32 系统接口。这与 Windows Terminal 使用自带控制台宿主的场景并不等价。对照目的：验证旧系统控制台宿主是否参与该实机故障，而不是预先认定它一定是根因。

源码依据：https://github.com/wezterm/wezterm/blob/4afedd626dadd15d9c2929bab0e2063b54f61393/pty/src/win/psuedocon.rs
微软组件：https://github.com/microsoft/terminal/releases/tag/v1.24.11911.0

## 使用（不安装、不替换系统文件）

1. 保存正在进行的工作，退出原版及上一份诊断程序。将此包解压到新的本地目录。三个二进制文件必须保持在同一目录，不要覆盖正式安装目录，更不要复制到 Windows/System32。
2. 双击 `cc-desk-paste-trace.exe`，打开受影响的项目/Claude 会话。仍使用原有配置；不要升级 CLI 或同时修改其他变量。
3. 保持会话打开，双击同目录 `Check-Backend.cmd`。必须出现 `conpty_backend=local_verified` 才是有效的新后端对照；否则保留错误信息，不要声称已切换。
4. 在空草稿中直接 Ctrl+V 粘贴原本触发故障的文本一次，不按 Enter、不在外部编辑器中修改。随后 Ctrl+G 只检查草稿全文是否完整。屏幕只显示部分行不能单独判失败。
5. 反馈 `backend-check.txt` 的结果和“直接粘贴后草稿完整/缺损”。出现缺损时同时保留本次 paste_trace 行。退出此程序即可回到原版；本包不会写入系统控制台文件或全局 Hook。

程序本体未签名。本包包含微软 ConPTY 的 MIT 许可证和来源/哈希清单；未包含其他 Windows Terminal 组件或字体。无需管理员权限，不要为了运行它关闭安全软件。

## 验证边界

打包工作流固定并校验官方 nupkg 的 SHA-256、基础 EXE 的 SHA-256，检查 portable-pty 所需三个旧版导出函数，并在 Windows Server 2022 实际执行创建（flags=6）、调整大小、关闭伪控制台。此检查不是 Windows 10 的 CLI 正文验收，也不是界面/性能回归测试。

实机 A/B 成功前不修改正式分发、不合并 PR 为修复。若确认本地后端加载后正文恢复，再补齐正式捆绑、版本/许可证、加载路径安全和失败回退的测试与发布流程；若仍缺损，否定本次后端假设，继续定位真实 CLI 输入读取/草稿处理，而不是叠加延时或删字符。
