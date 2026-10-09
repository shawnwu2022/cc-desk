# 本机到云端工程交接（2026-10-04）

后续开发转至 `shawnwu2022/cc-desk` 的 `feat/unified-workspace-ux`，继续使用 [Draft PR #34](https://github.com/shawnwu2022/cc-desk/pull/34)。PR 的基分支是 `feat/native-cli-finalization`。原始本机 `main` 不参与切换、合并或重置。

## 本次交接的变更

本交接基于已推送的 `aa6c1704b4b8c8ef130b12c684cc213eace09b00`，仅补交两处之前保留在本机的测试诊断，以及本说明和 AGENTS.md 指引。它们不是新的功能修复，不改变生产程序、工作流、版本、发布策略或安装状态。

- `src-tauri/src/tests/version_history_payload/token.rs`：只有原有 worker 退出码或 owned Job 计数检查失败时，输出 `HISTORY_CONFINED_WORKER_TERMINAL` 的退出码和活动进程数，随后继续返回原错误。
- `src-tauri/src/tests/version_history_restart_lifetime_windows.rs`：只有原有 10 秒 marker 期限耗尽时，观察精确原进程的终态/退出码与 Job 活动计数，然后继续原断言失败。没有延长期限、增加重试或更改清理协议。

这些输出仅包含固定分类、布尔值和数字；不新增路径、SID、PID、Job 名称、凭据、环境变量、会话或原始错误正文。两文件交接前的合并 binary diff SHA256 为 `f6167a69e193454dc7503829b978dcc19039359164bd4ed70740f1e7e6333abe`，与此前保留版本一致。

## 诊断证据及其局限

此前本机受控 RestartLifetime fixture 在启动期限到达时观察到 `exactRootTerminal=false`、`exitCode=null`、`activeProcesses=1`，随后 worker 退出 101，外层仍按原条件拒绝。这证明诊断输出可用，不证明恢复流程通过。

另一次未提交的显式标准句柄对照确认：受控子进程越过 CRT 钩子并收到匹配的三个文件句柄，但未观察到 libtest 的首次输出或启动 marker。它仍失败，不能据此认定是 marker 权限、某个标准句柄或特定 libtest 初始化位置导致阻塞。该实验性的句柄/初始化钩子改动已经撤回；本交接不带入这些实验。

两处保留诊断此前随本机测试构建及严格 all-target Clippy 编译通过；受控失败记录验证了新增字段。交接阶段只做格式、差异、范围和远端核验，不重跑安装/回退、受控进程实验或完整构建。不要将此前的失败观测描述为修复成功。

## 已验证远端基线 aa6c170

以下是准确提交 `aa6c170` 的结果，不是本交接提交新增 CI 的结果。推送自动触发的后续检查交由云端接续核对，本机不再等待或重跑。

- [D12](https://github.com/shawnwu2022/cc-desk/actions/runs/37183724112)、[D13](https://github.com/shawnwu2022/cc-desk/actions/runs/37183724084) 的 Windows/macOS/Linux 检查通过。
- [视觉检查](https://github.com/shawnwu2022/cc-desk/actions/runs/37183724098)、[历史恢复专项](https://github.com/shawnwu2022/cc-desk/actions/runs/37183724104)、[历史包证据](https://github.com/shawnwu2022/cc-desk/actions/runs/37183724087)、[Windows 测试包](https://github.com/shawnwu2022/cc-desk/actions/runs/37183724099) 通过。
- [普通 CI](https://github.com/shawnwu2022/cc-desk/actions/runs/37183724144) 于 07:26 UTC 结束，结果失败。独立进程门禁 0 通过/1 失败；其余 Rust 测试 1173 通过/11 失败/31 忽略/1 过滤。过滤项就是前面的独立门禁。Channel 单项复核 1 通过，不重复计入通过总数。
- 12 个不同失败测试都报告 `PermissionDenied: source host is job-contained`。诊断记录 `source_in_job=true`、`source_job_limits=8192`；失败集合与 `87a6c00` 基线一致，没有新增失败测试。前端、编译策略、格式和严格 lint 通过；普通 CI 的应用加载器检查被跳过，不能计为通过。
- GitHub Windows runner 的外层 Job 限制仍阻止这些独立管理器证明。不要放宽准入、把失败改为通过，或用合成证据代替实际进程所有权。

## 当前安装与源码的对应关系

本机仍安装 `103cf2b4c547af80ea1951269380eec767ba58fd` 的本地 release 构建，EXE SHA256 为 `A265B214918024E6EED6AF3B8A482A6F54C5EE76606F5BD98CB792046740570D`。`aa6c170` 只修独立测试工程兼容性，本交接也只有测试诊断/文档；均不需要再次替换该生产代码构建。

103cf2b 修复了 Claude 主历史枚举遇到官方 subagents 布局、以及大 Codex transcript 的有界历史摘要读取。局部验证包括 49 项 Rust 测试、126 项前端回归、类型检查、格式和严格 lint；本地打包及 staged/installed ConPTY loader/PTY 生命周期检查通过。这不代表真实历史列表、完整会话生命周期或历史版本往返已经验收。

用户已在较早的 87a6c00 构建确认首次程序绑定后两种真实 CLI 能打开；尚无 103cf2b 历史列表恢复的用户验收反馈。程序发现后的首次确认仍是当前 native profile 绑定流程，同一有效配置应复用。

## 云端剩余工作与边界

1. 保持 `SUPPORTED_ROUNDTRIP_ENABLED=false`，正式历史安装/回退入口与自动发布仍关闭。验收 driver 默认禁用并排除普通生产构建；编译策略检查成功不能开启该入口。
2. 在明确授权、一次性且能力预检通过的 Windows 环境，完成真实 `0.18 → 0.17.7 → 0.18` 安装、启动、返回、完整读回和最终重开验收。遵循 [历史版本准备与恢复契约](historical-version-preparation.md) 和 [一次性往返验收说明](testing/historical-roundtrip-disposable.md)。本机也不能仅因有工具链就假定具备安全验收能力。
3. 崩溃/重启恢复仍有缺口。重新取得实际恢复对象、独占所有权、真实终态及原 Job 为空等证据后，才可使用已封存且未领取的返回检查点；未知、领取后中断或证据缺失不能自动重放。named Job 消失或 PID 消失本身不能补造恢复权限。RestartLifetime 失败定位仍需可靠的受控测试入口/原生等待证据。
4. 保留历史摘要的部分结果标记和 absence guard。实际界面刷新结果、Native 创建/切换/关闭/恢复、真实 CLI Layer-C、Windows 缩放与可访问性仍须分别验收。
5. 早先两项 PTY 就绪超时已通过测试 harness 的 ConPTY DA1 回复修复进行验证；aa6c170 的失败集合中没有 PTY 超时。该结论不等于真实 CLI 的完整输入/退出认证。
6. 黑色辅助窗口归属外部 Codex 桌面/执行宿主：已观察到 `ChatGPT.exe`（OpenAI.Codex 应用包）→ `codex.exe` → `node_repl.exe` 的完整父链，以及另一组 Codex app-server-daemon 子进程；没有证据表明由 CC Desk 重复启动。窗口问题未修复，本交接不修改或重启外部宿主。

## 留在本机的材料

备份、安装包、构建产物、私人数据副本、进程观察、原始本机日志和撤回的实验补丁继续留在本机，不上传到仓库。它们不是云端构建或继续正常源码开发的依赖。需要进一步复核某项本机证据时，由拥有该环境的执行方按最小范围提取脱敏结果，不能把本机路径视为云端可读文件。

本机完成正常推送及远端 SHA/PR 核验后停止执行。后续从该 PR 分支的实际远端 HEAD 继续，不从旧本机 main 或历史安装包反推源码状态。
