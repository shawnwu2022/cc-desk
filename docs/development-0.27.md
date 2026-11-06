# 0.27.0 开发与验证记录

目标是更新可控、失败可恢复、问题可定位，以及 CLI 原生优先的归档清理。此文档记录开发状态，不构成发布或真实用户验收证明。

## 基线与工作目录

- 集成目录为 `E:\Codex\2026-11-06\task-2\cc-desk`，分支为 `feat/v0.27.0`。
- 起点是 RC3 main `4d4674480849f1d4d8fb4edab095b388e769e189`。2026-11-06 只读核对远端 main 一致。
- 原修复与远程功能工作树全部保留。原修复分支中的粘贴、身份及 OpenCode v2 改动不整支合并。
- 本任务只本地提交已验证改动，不合并 main，不 bump 版本，不发布。

## 主链路与方案

现有 `UpdateSection → update store → Tauri command → UpdaterService` 负责更新。Rust 保留一次性安装准入，重新检查官方发行版、精确 tag/source、公钥、签名、资产 ID/大小/摘要和会话静止。历史版本有独立的文档绑定选择、准备及 Windows 安装管理器，不复用普通更新准入。

比较了两个方案。由前端选择任意 manifest URL 会扩大信任边界，且不能可靠阻止旧 stable 客户端收到 RC。采用后端持久化用户通道、后端从固定官方发行目录挑选版本，再使用精确 tag 的 manifest。现有 stable latest endpoint 和发布门禁保持稳定语义。前端只发送通道枚举和不透明安装准入 ID。

数据模型分别表示用户通道 `stable | test | preview`、已发布版本类别、产物隔离标记和安装生命周期。`candidate`、`test-only`、`publishable=false`、`updaterPublication=false` 永远不能证明晋升。test 只接受正式、明确 beta 和 RC；preview 额外接受明确 preview。未知 prerelease 拒绝。切回 stable 只选高于当前版本的正式版本，不自动降级。

## 阶段与完成条件

1. 通道闭环。默认 stable，设置持久化，正式/测试/预览选择与风险可见。发现与安装重验均遵守所选通道，原签名与来源门禁不变。验证跨通道矩阵、伪造/未发布/排除产物和旧 stable 行为。
2. 更新生命周期。检查、下载、验签、安装、重启分别可观察。安装前支持取消；安装效果开始后不承诺取消或重放。失败、断网和坏包不得触发安装。活动或未知会话阻止交接。
3. 恢复与诊断。复核现有历史目录及九个旧包范围。提供最小可靠恢复说明和本地脱敏摘要，不包含会话正文、路径、凭据、原始异常或令牌，不自动上传。真正 stable → 0.27 RC → stable 只能在隔离安装和合成数据完成。
4. 归档清理。核查三 CLI 当前原生接口。区分 Desk 列表归档与 CLI 数据删除，基于精确源身份、级联范围和当前状态预览，重新核验后执行。无已验证原生接口时明确不可用，不猜路径删除。
5. 审查与门禁。独立对抗审查通道、签名、取消及清理。运行受影响测试、类型、构建、现有聚合门禁。记录已过、失败、未运行和真实平台缺口后本地提交。

P1 包括忽略单个候选、复制摘要、更新记录及失败重试，不能推迟 P0 验收。

## 文件所有权与吞吐检查点

集成负责人持有前端、诊断、文档和最终集成。一个实现子代理先在独立 checkout 修改 Rust updater/policy/config 与对应 Rust 测试，收口后另一个实现子代理顺序复用该辅助分支补发布策略。两者没有并发写同一工作区，不修改前端和远程工作树。共享 IPC 先固定，偏离后同步本文与双方契约。阶段收口只读审查不写代码。

每阶段以可重跑的行为测试结束。第一检查点是通道矩阵和设置交互闭环；随后整合生命周期，再检查恢复和清理。不能把合成 IPC、单元测试或构建成功当作真实 CLI、安装版 WebView、用户双行现场升级或 AI 自动命名的验收。

## IPC 契约

- `get_updater_settings` 在现有 proxy 外返回 `channel: 'stable' | 'test' | 'preview'`。
- 新增 `save_updater_channel(channel)`，只修改所选通道并退休旧安装准入，不能改 proxy。
- `check_desktop_update` 无 URL/通道参数，读取后端保存的设置。保留现有 summary，新增 `selectedChannel`，`channel` 表示候选类别或不可用产物标记。
- `cancel_desktop_update(admissionId)` 返回 `{ status: 'requested' | 'too-late' }`。只有准确当前可取消准入才返回 requested；过期 ID 失败。
- `desktop-update-progress` 保留 `admissionId/downloaded/total`，phase 采用 `downloading | verifying | installing | restarting`。
- 取消以 `UPDATER_CANCELLED` 安全失败回执结束；安装结果未知继续保留现有不可重放保护。

## 验证状态

这是开发启动批次，尚未完成 0.27 整体验收。

| 范围 | 当前状态 | 证据与限制 |
| --- | --- | --- |
| 通道发现、设置与准入 | 源码已连接 | 用户通道与产物隔离分开；stable 保留旧 latest；安装前重验。 |
| 原发布链 Beta/preview | 本地策略与签名向量测试通过 | 共用版本 parser；全部原发布门禁保留；没有执行发布或实际签名构建。 |
| 下载、验签与取消 | 源码与行为验证通过 | 精确 ID、取消/安装 CAS、有界传输、分阶段；真实安装和重启未运行。 |
| 恢复交互 | 新增恢复指引，复用原历史版本流程 | 九个审核旧包范围不扩大；普通安装仍无一键返回；真实 stable→0.27 RC→stable 未运行。 |
| 脱敏摘要 | 已扩展通道、阶段、候选与固定错误码 | 不读原始日志、不自动上传；CLI 版本明确 not-collected，安全版本采集仍未实现。 |
| 归档清理 | [真实接口核查完成](native-session-cleanup-0.27.md) | 完整后代集合、预览预约及原生执行尚未实现。没有清理用户数据。 |

后端原实现提交 `9812ae0880b28ba0191591dcd6d7c4fb0887888f` 在集成分支成为 `65b6e7966635c615e02197bf5aaaff5466d39820`。发布策略原提交 `5a0a7b4619b29cfbfb682899197f87485f8f0fbe` 在集成分支成为 `6cf96b92fc83e9a0682c150465fe22ea1b3ea6fd`。前端和最终审查修复在后续本地提交，最终 SHA 以交付报告为准。

## 本地验证与失败记录

日志保存在本任务 `.audit/`，只说明本地源码与合成边界，不是托管 CI 或发布证明。

- 最终集成 Windows `cargo check --manifest-path src-tauri/Cargo.toml --locked --offline --lib` 及 `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --lib -- -D warnings` 通过。使用集成目录独有 target。
- `cargo test --manifest-path tests/updater-core/Cargo.toml --locked --offline -- --test-threads=1` 实际 31/31 通过。最初一次 `--exact` 缺少模块名前缀只执行零项，明确排除；修正完整名称后先复现验签错误阶段失败，再修复为 verify。
- `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib desktop_updater -- --test-threads=1` 在最终集成目录实际 33/33 通过，另 1725 项被该域过滤，不能称完整 Rust 库通过。全四 harness/16 片、all-target Clippy 和 hosted 聚合未运行。
- `npm run typecheck` 和 `npm run build` 已通过。最终审查两个竞态按测试先行复现，再通过受影响 6 文件 72 项。整体 Vitest 第一次 202 文件 3181 通过、2 原跳过；后来并发 Windows 编译时发生下面六项失败，复核结果另存，不覆盖失败。
- 现行 CI 的 23 个独立 Node policy 文件共 316 项，310 通过、6 原跳过、0 失败。完整命令见本任务验证记录。
- `npm run test:production` 中英文实际生产 CSP 首屏 2/2 通过。`npx playwright test tests/visual/unified-workspace.spec.ts -g 'catalogue, selection and blocked preparation remain usable'` 2/2 通过；同文件 `-g 'update channels and recovery guide remain reachable'` 2/2 通过。新增截图已实际查看，仍为未批准证据，原基线未改。

### 保留的超时失败

独立核心库一次默认并行测试为 30/31，`ProxyProbe_BodyTimeout_003` 的服务端十秒未接受连接。精确命令、输出转录和源码比较保存在 `.audit/updater-proxyprobe-failure.md` 与 `.audit/updater-proxyprobe-64262-transcript.txt`，转录不是原生 Cargo 日志。相关测试与实际读取路径和 RC3 相同；原因未定，有既有 fixture 负载敏感线索，不能声称已排除新回归。后续 31/31 与 33/33 串行通过分别保留。

一次 `npm run test:ci` 与本任务 Windows 主 crate 编译、Node 聚合及构建并行。结果为 202 文件中 196 通过、6 失败，3176 项通过、6 失败、2 跳过。六项都触发原 5000 ms 超时，文件本次未修改：

- `D01_Baseline_RecordWritesValidatedReport_05`。
- `D28_D20_BaselinePlanStillPasses_01`。
- `D20_Runner_ExecutesFourCellsWithoutProductionCredentialLeak_08`。
- `Observer_PluginCommandPath_006`。
- `VisualGate_ReviewedAssetLoadingIsFixtureOnly_006: enabled=false`。
- `VisualWorkflow_OfficialArchivePreservesTrust_003`。

原始输出 `.audit/v027-vitest-final.log` 保留。不能只凭并发背景断言根因，也没有修改超时、断言、工作量或并发配置。原生编译结束后使用相同命令复核，输出另存 `.audit/v027-vitest-reviewed.log`。

该次复核已完成，202 文件全部通过，3184 项通过、2 原跳过，共 3186。最终 typecheck 退出 0。完整命令和分层结果见 `.audit/v027-validation.md`；本地通过不覆盖之前两个超时事件，也不是托管 CI 通过。

## 审查和剩余工作

独立审查配置实际为 `gpt-6.1-sol / high`。审查发现的确认窗遮挡、忙时重挂载、未知保存全局准入及迟到取消回执均有对应行为回归。最终结论以收口复核为准，不能把一次初审当最终快照批准。

收口复核已独立确认上述四项关闭，未发现新的具体问题。最终两项的 RED/GREEN 输出及受审源码 hash 保存在本任务验证记录。原生及浏览器合成边界以外仍按下方未验收事项记录。

实际使用已安装的 pstack `poteto-mode`、`figure-it-out`、`show-me-your-work` 组织分工和决策证据，并按 domain/type 与去冗余指导保持边界。使用 superpowers `systematic-debugging` 追溯状态流，`test-driven-development` 为最终两个竞态先保留 RED，再验证 GREEN；`verification-before-completion` 区分本地通过、失败与未验收。没有安装新技能，没有重复另起方案流程。

剩余 P0 包括安全清理的完整级联预览及一次性执行、CLI 版本安全采集、签名 0.27 候选的隔离 Windows 往返。真实安装版 WebView、三 CLI D20、用户双行现场升级、身份关联及 AI 命名仍未验收，不能用本任务合成测试宣称已修复。P1 忽略单个候选、持久更新记录仍未实现。
