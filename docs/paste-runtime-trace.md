# CC Desk 粘贴链路诊断包

这是定位版本，不是修复版本，不应合并/发布为“已解决粘贴截断”。正常版本默认关闭这项诊断；只有同时设置构建变量 `VITE_CC_DESK_PASTE_TRACE=1` 与 `CC_DESK_PASTE_TRACE=1` 的诊断可执行文件会启用。

## 使用

保存正在进行的工作后退出原 CC Desk，解压本目录，运行 `cc-desk-paste-trace.exe`。这是未签名的测试可执行文件，不是安装包；不会安装/替换原版。仍使用现有 CC Desk 配置与日志目录，不新增用户全局 Hook 或修改粘贴快捷键。

进入受影响的 Claude 会话，在空草稿中直接粘贴能触发问题的文本一次，先不要提交。第一份粘贴开始后记录最长 60 秒、最多 256 个输入事件。随后退出诊断程序即可停止。复制当天 `~/.cc-box/logs/YYYY-MM-DD.log` 中包含 `[paste_trace]` 的行用于定位，不需要提供原始业务日志。

## 核对内容

同一次剪贴板读取在前端获得事务编号 `pasteId`。数据在原来的 `pty_input` IPC 中携带可选诊断元数据，不新增 await、IPC 往返、重试或传输分块。

在诊断模式下，换行规范化后的剪贴板参考文本随同一个 IPC 请求送到 Rust，在内存中与实际收到的正文严格比较；不是只比较长度，也没有哈希碰撞问题。参考正文只存在于该请求内存中，不写文件、不写日志、不随测试附件上传。出于有界诊断考虑，Rust 只比较不超过 2 MiB 的参考文本，超出时 `exact=None`，绝不能算完整性通过。原始 `data` 不被裁剪。

`[paste_trace] recv` 中：

- `paste`、`send_seq`、`recv_seq` 关联同一次粘贴及随后普通输入；`age_ms` 是从剪贴板读取开始计算的时间，并非纯读取耗时。
- `ipc_size_match` 比较前端 payload 与 Rust 数据字节数。
- `summary.exact=Some(true)` 才表示规范化剪贴板正文与 Rust 正文逐字一致。
- `first_difference` 和 `common_suffix` 仅记录字节位置/长度，不包含正文。
- `controls` 依次记录 LF、CR、TAB、ESC、Backspace、Delete、Ctrl+C、Ctrl+U、Ctrl+W 的数量。完整粘贴帧的外层标记不计入正文控制字符。

`[paste_trace] done` 记录原 writer 返回结果，不代表 Claude 已保留或提交了完整正文。

## 诊断边界

前端与 Rust 的序号体现 IPC 投递/到达顺序，不是 writer 锁获取顺序。输入法补发仍归为既有 `terminal-ondata` 来源，不能仅靠该标签识别具体 IME。诊断 CPU 与日志写入有开销，不能保证不影响时间敏感问题的复现。

这项诊断不捕获 `Ctrl+G` 文件或实际 `UserPromptSubmit` 正文。Rust 正文正确而草稿不正确时，仍需检查后续 Windows/ConPTY/CLI 输入处理。记事本回填后草稿完整但屏幕显示不全的现象，单独判断，不据此宣布直接粘贴已修复。

原有粘贴标记、LF 规范化、shell 选择和 writer 均未改动。没有自动重发、删除制表符、压缩 JSON 或强制提交。
