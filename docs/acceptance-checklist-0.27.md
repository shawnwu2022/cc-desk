# 0.27 实现与验收清单

本清单区分实现、隔离验证和发布验收。独立分支 `feat/v0.27.0` 已在新授权下把正式 main `0aaaf2a171ced55ff2b4d7a3e1731027c45809df` 合入开发侧；整合前 merge-base 为 RC3 `4d4674480849f1d4d8fb4edab095b388e769e189`，整合后该正式 main 已是开发分支祖先。实际 merge 为 `89e30ff37ca51b29239c17abbc52f0dc85106390`，无冲突；受影响验证后的测试提交为 `68436f31dc8a6313e9d87ca9ab35717671edfcfd`。永久删除窄实现提交 `6de019a2ee22528d1c391e76c449c968c9846140`，最后验证见 [本轮审计](../.audit/v027-stage8-permanent-delete.md)。当前应用身份仍0.26.0，未自行 bump 0.27、未推送/发布/用户安装；整合前后证据见 [整合记录](../.audit/v027-stage6-main-integration.md)。

| 规划 | 实际实现和代码证据 | 验收边界 |
| --- | --- | --- |
| P0 stable/test/preview | `src-tauri/src/desktop_updater.rs`、`updater_policy.rs`、`scripts/release-version.mjs`、`src/stores/update.ts`、`UpdateSection.vue`：默认 stable、持久选择、官方目录与精确 tag 筛选；保留 release/source/资产/摘要/签名绑定及 candidate/test-only 隔离 | 本地源码/行为验证；没有真实签名 0.27 三通道候选。旧 stable latest 不收 RC、切回 stable 不自动降级为已实现策略，真实发布闭环尚未演练。 |
| P0 可靠更新 | `desktop_updater.rs`、`updater_http.rs`、`UpdateSection.vue`：可观测下载/验签/安装/重启阶段、准确取消与安装 CAS、活动/未知会话 gate、出错撤销旧准入 | 取消/坏包/签名/网络/竞态合成验证；没有在真实安装上模拟失败。安装或重启交接不代表安装成功。 |
| P0 历史选择/签名准备 | 既有 `HistoricalVersionsPanel.vue` → store/API → `version_history/catalog.rs/download.rs/verified_package.rs`；已接通分页、筛选、选择及下载验签；`historicalVersions.test.ts`、Rust catalog/download 测试 | 只选比当前旧的官方 stable，Windows x64 NSIS；元数据可选和发布者验签不等于执行准入，准备不建返回点。macOS/Linux 执行未支持。 |
| P0 历史安装/恢复执行 | 已有两条独立能力：普通签名历史安装不受九包表限制，完整源安装/注册/Desk/WebView 备份后 FreshSettings 交接安装器，人工恢复；严格切换/返回限 `payload_policy.rs` 九个精确版本/包/输出清单。`historicalInstall.test.ts`、`historicalSwitch.test.ts` 与 Rust ordinary/payload/context/return 回归 | 普通模式无自动 Return 或一键人工恢复，交接不代表安装完成；九包测量不等于真实往返。0.26 正式版不在九包严格返回表内。两条路径均保留未提权、Job-free、唯一安装/会话/所有权门禁。真实 stable→0.27 RC→stable 缺正式签名候选与隔离安装验收。 |
| P0 恢复指南 | 本轮更新页新增手动恢复指南，`UpdateRecovery_ManualGuideHasNoInstallationEffects_005` 验证无安装副作用 | 指南不创建快照、不执行恢复、不扩大上述两类能力，也不能代替历史执行验收。 |
| P0 本地诊断 | `diagnostic_versions.rs`、`cliVersionDiagnostics.ts`、`useCliVersionDiagnostics.ts`、`AboutSection.vue`：所选 profile 固定参数/有界探测、取消/迟到/revision、未知进程 owner、匿名来源和白名单复制 | 合成后端测试；本轮另完成三个真实 CLI 的隔离只读版本探测，不能混称生产后端端到端。未自动上传或扫描用户认证/聊天。 |
| P0 永久删除 | 用户明确取代隐藏/软归档。`NativeSessionDeletePanel.vue`、nativeSessionDeletes store/API、`native_delete.rs`/projection service：Windows 已归档 Codex 根、只读预览、独立二次确认/双 ack、一次性票据、受控原生 thread/delete、Job/来源保护和只读结果核查 | **已交付 Codex 窄实现及合成验证**，范围为同 profile 原生根及动态子树，count=null，不声称冻结/完整枚举或独立逐后代结果。OpenCode/Claude 执行各自关闭；阶段10已完成真实0.167.0单根CLI隔离后端验收；document IPC、可见安装版WebView、Unix和完整子树仍未验收。详见 [新证据与边界](permanent-delete-0.27.md)。 |
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

上段完整套件是旧源 `68fa6b3…` 的证据。最新整合树 `68436f31…` 另通过 Rust 91、前端14文件169、Node310/6原跳过、固定工具链all-target Clippy/fmt、typecheck/build和生产CSP2/2；没有重跑新源完整16片或完整Vitest。已有足够本地证据继续候选审阅材料和隔离矩阵准备，没有创建发布。`nativeAll` 仍未验证，本地结果不能替代 exact-main hosted CI 或真实安装验收。当前无运行中验证或等待批准的操作。

0.27 整版验收仍需真实签名候选的隔离升降级往返、目标平台安装与 WebView、D20/真实会话及现场回归证据。18 项 Job-free 专项按既有 [发布契约](release-process.md) 可在成功确认宿主限制后非阻断披露，不另立新的发布阻断，也不计为通过。当前 Codex 窄删除有产品实现，不能用它补齐其他提供方、逐后代结果或真实 CLI 证据。此处没有向 main 合入、签名、发布或真实用户安装授权。

## 明确未交付或未运行

- 永久删除完整子会话枚举/冻结集合及独立逐后代结果未交付；当前 Codex 按用户新授权的根及原生子树语义执行，不一概要求上游任意 expected IDs API。OpenCode 缺已绑定持久服务来源和执行期间拒忙/阻止并发启动的约束；Claude private junction+受限 SDK 原件映射已在合成实验成立，产品仍缺动态 namespace/整个执行期间活动保护，不能沿用 hardlink rename 或把 fixture DACL 当生产恢复方案。见 [分别核实的事实](permanent-delete-0.27.md) 和 [阶段9反例](../.audit/v027-stage9-provider-delete.md)。不是等待用户意图确认或自动审批拒绝。
- 真实签名 0.27 候选的 stable→RC→stable：缺正式晋升产物和可销毁的隔离安装环境/输入；此任务未获新发布或用户安装授权，详见 [待执行矩阵](acceptance-0.27.md)。未用测试签名代替。
- Unix 验证：`wsl.exe --list --quiet` 被操作系统以 `E_ACCESS_DENIED` 拒绝，未证明可用隔离环境；未重试提权、安装或新建。
- 当前源码的完整 Linux 受准像素 CI、macOS/Linux 安装、安装版 Windows WebView、真实 profile→诊断 IPC→CLI 端到端、三 CLI D20、双行现场/身份关联/AI 命名：环境、测试账号或真实现场输入未具备。定向 Windows 界面、只读版本及合成 Wry fixture 均不冒充这些验收。

可选后续为在合格 job-free 宿主补原始 native All，或按提供方具体契约继续其永久删除支持；不把当前未运行项改写为通过。0.26 main 已按后续授权整合到开发侧，不推送/发布。

完整可读候选变更、两类历史执行能力、实现提交索引、验收摘要和不得发布条件见 [候选审阅材料](candidate-review-0.27.md)。本轮新增永久删除窄能力，最终 Rust 新域7、前端10文件408、Node安全9、类型/生产构建和中英CSP2/2通过；没有重跑旧完整16片/完整Vitest，也不把其旧源证据冒称新源全量结果。最终来源及失败保留见本轮审计记录。

阶段10增量：真实Codex 0.167.0在任务合成原件上完成initialize/read/delete，固定二进制摘要、系统配置准入、独立sqlite_home、无认证/模型和实际Job回收均有记录。自动认证、合法状态通知及未发送错误阶段有RED→GREEN；最终18项Rust（5个手动入口按规定ignored、真实入口另显式运行）和3文件24项前端、严格all-target Clippy/typecheck通过。原B001 unknown仍保留，未重放；只读101和新B002各有独立证据。没有把Service caller当作真实document IPC或可见双确认，没有重跑旧16片/全量Vitest、安装版或签名往返。见[阶段10审计](../.audit/v027-stage10-codex-live.md)。Claude/OpenCode具体维护路线只是设计，生产删除保持关闭。
