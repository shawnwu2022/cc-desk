# 0.27 实现与验收清单

本清单区分实现、隔离验证和发布验收。独立分支 `feat/v0.27.0` 沿用 RC3 `4d4674480849f1d4d8fb4edab095b388e769e189`；未同步并行 0.26 main，未推送/发布/安装。各阶段精确提交与命令见 `.audit/v027-validation.md`、`.audit/v027-p0-validation.md` 及本轮最终验证记录。

| 规划 | 实际实现和代码证据 | 验收边界 |
| --- | --- | --- |
| P0 stable/test/preview | `src-tauri/src/desktop_updater.rs`、`updater_policy.rs`、`scripts/release-version.mjs`、`src/stores/update.ts`、`UpdateSection.vue`：默认 stable、持久选择、官方目录与精确 tag 筛选；保留 release/source/资产/摘要/签名绑定及 candidate/test-only 隔离 | 本地源码/行为验证；没有真实签名 0.27 三通道候选。旧 stable latest 不收 RC、切回 stable 不自动降级为已实现策略，真实发布闭环尚未演练。 |
| P0 可靠更新 | `desktop_updater.rs`、`updater_http.rs`、`UpdateSection.vue`：可观测下载/验签/安装/重启阶段、准确取消与安装 CAS、活动/未知会话 gate、出错撤销旧准入 | 取消/坏包/签名/网络/竞态合成验证；没有在真实安装上模拟失败。安装或重启交接不代表安装成功。 |
| P0 历史恢复 | 既有 `src-tauri/src/version_history/`、`HistoricalVersionsPanel.vue`，本轮复用而未增加回滚引擎；更新页补手动恢复指南 | 已审核旧包范围仍为 Windows x64 九包；通用历史安装不是一键返回。真实 stable→0.27 RC→stable 仍缺正式签名候选及隔离安装验收。 |
| P0 本地诊断 | `diagnostic_versions.rs`、`cliVersionDiagnostics.ts`、`useCliVersionDiagnostics.ts`、`AboutSection.vue`：所选 profile 固定参数/有界探测、取消/迟到/revision、未知进程 owner、匿名来源和白名单复制 | 合成后端测试；本轮另完成三个真实 CLI 的隔离只读版本探测，不能混称生产后端端到端。未自动上传或扫描用户认证/聊天。 |
| P0 原生归档清理 | `NativeCleanupAvailability.vue`：已观察身份、缺失/活动/恢复状态和明确能力拒绝；原生按钮关闭，无 mutation | **完整级联预览与执行未交付**。原生接口缺少预览集合与活动状态的原子条件，详见 `native-cleanup-blocker-0.27.md`；不是降低文案后算完成。 |
| P0 会话回归 | 既有新建/首事件/刷新/关闭/恢复/重命名/身份关联测试；本轮固定 1.98.1/CI profile 的本地四 harness/16 片及聚合通过 | 合成 fixture 或独立 Wry 边界，不等同真实安装版、真实 CLI 会话、用户双行现场或 AI 自动命名。未读取用户真实会话来验收。 |
| P1 单候选忽略 | `updatePreferences.ts`、update store / 更新页：持久保存一个精确 version/releaseId/sourceSha 身份；忽略会撤销前端旧准入、隐藏更新提醒；后续版本仍提示，取消忽略需重新检查 | 只影响通知/选择，不产生后端安装权限；不按版本白名单限制 CLI。损坏数据丢弃并提示，存储失败仍可在内存使用。 |
| P1 一键脱敏摘要 | 关于页显式复制应用/更新/所选 CLI 版本摘要 | 不含配置名、路径、原生 ID、凭据、正文、原始错误，无自动采集/上传。 |
| P1 更新记录 | `updatePreferences.ts`、update store / 更新页：最近 20 条白名单检查/阶段/错误事件，支持本地清空 | 本机浏览器偏好，最佳努力保存；不是可信审计或安装成功证明。不记录 URL、admission、会话数据或原始日志。 |
| P1 失败重试 | 更新页显式“重新检查后再重试”只调用新检查；旧安装准入不复活 | 未知安装/重启不能盲重放；取得新候选后仍需明确安装确认与后端重验。 |

## 可独立执行的补充验收

三个已安装 CLI 仅执行固定 `--version`，使用任务内全新 HOME/USERPROFILE/APPDATA/XDG/各 CLI 配置根，子进程不继承认证环境；关闭更新/非必要流量，记录可执行文件前后哈希相同。结果：Claude `2.1.318`、Codex `0.166.0`、OpenCode `2.2.5`。没有创建/恢复/归档/删除会话，也没有通过该脚本调用生产诊断 IPC。具体输入与白名单结果保留在 `.audit/v027-stage3-cli-versions.mjs`、`.json`。

Claude 当前官方 [配置路径说明](https://code.claude.com/docs/en/settings#find-or-create-your-settings-files) 支持 `CLAUDE_CONFIG_DIR`，官方 [禁用更新说明](https://code.claude.com/docs/en/setup#disable-auto-updates) 区分禁用后台更新与 `DISABLE_UPDATES`。同页提供 `--version` 的带产品名输出例子；这补充了早先仅按兼容向量记录的依据，不证明所有历史构建格式。

`wsl.exe --list --quiet` 返回 `Wsl/EnumerateDistros/Service/E_ACCESS_DENIED`。因此无法确认可用隔离 Unix 环境，本轮不安装/新建环境，也不把此结果写成“机器没有 WSL”。

真实 D20 按 `docs/superpowers/execution/D20.md` 需要明确授权测试账户、位于隔离根的真实二进制及两条 lane driver，并收集真实 hook 四格证据。本任务没有这些账户/驱动输入；不借用用户凭据，也不把只读版本或合成 hook 当作 PASS。

## 候选准备与发布门槛

实现提交 **`68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3`** 已完成本轮可隔离推进的验证：固定 Rust 1.98.1、CI dev/test debug=0，四 harness/16 片及原聚合通过（1744 pass、36 原 ignore、18 宿主排除、0 fail/timeout）；最终完整前端 207 文件、3206 pass、2 原跳过。Node 政策 310 pass/6 原跳过、all-target Clippy/fmt、类型/构建、生产 CSP 中英 2/2、P1 界面中英 2/2 均通过。完整命令、各实现 SHA、独立复核与保留的原始失败见 [本轮验证记录](../.audit/v027-stage3-validation.md)。

已有足够本地证据进入候选审阅材料和隔离矩阵准备；没有创建发布。本地四 harness 结果是 normal required scope，`nativeAll` 仍未验证，不能替代 exact-main hosted CI 或真实安装验收。当前没有正在运行的验证或等待批准的操作。

0.27 整版验收仍需真实签名候选的隔离升降级往返、目标平台安装与 WebView、D20/真实会话及现场回归证据。18 项 Job-free 专项按既有 [发布契约](release-process.md) 可在成功确认宿主限制后非阻断披露，不将其另立为新的发布阻断，也不计为通过；若要宣称原始 native All 已验收，仍需合格宿主的独立证据。原生清理完整功能仍被接口语义阻塞，必须继续明确为关闭/未交付范围，不以未来假设补齐。此处没有合并、签名、发布或真实用户安装授权。

## 明确未交付或未运行

- 原生清理完整级联预览、一次性范围预约、活动保护下的条件 mutation、部分失败与超时协调：原生协议不能原子比较确认集合并拒绝活动；当前安全边界为关闭，见 [固定官方源码与反例](native-cleanup-blocker-0.27.md)。不是等待用户确认或自动审批拒绝。
- 真实签名 0.27 候选的 stable→RC→stable：缺正式晋升产物和可销毁的隔离安装环境/输入；此任务未获新发布或用户安装授权，详见 [待执行矩阵](acceptance-0.27.md)。未用测试签名代替。
- Unix 验证：`wsl.exe --list --quiet` 被操作系统以 `E_ACCESS_DENIED` 拒绝，未证明可用隔离环境；未重试提权、安装或新建。
- 当前源码的完整 Linux 受准像素 CI、macOS/Linux 安装、安装版 Windows WebView、真实 profile→诊断 IPC→CLI 端到端、三 CLI D20、双行现场/身份关联/AI 命名：环境、测试账号或真实现场输入未具备。定向 Windows 界面、只读版本及合成 Wry fixture 均不冒充这些验收。

可选后续为在合格 job-free 宿主补原始 native All，或在上游提供已证明的条件清理接口后恢复该功能研发；均不把当前未运行项改写为通过。当前分支不自动整合 0.26 main，不推送/发布。
