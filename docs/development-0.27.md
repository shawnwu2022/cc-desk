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

## 后续 P0 执行

本轮从 `da8aaa6e0d2b8d4ea7ed198110e39bf0bb6f2dfd` 继续，保留原 todo 与失败材料。先完成选定 profile 的版本探测，再完成清理预约、级联预览、确认、执行与结果协调。签名候选与平台验收只准备隔离脚本及故障矩阵，不能阻断这两项独立开发。

根代理负责前端、清理方案及集成；最多一个实现代理在新的独立辅助 checkout 顺序负责版本探测后端，再衔接清理后端。先固定下列版本探测接口，不同时修改同一工作区。阶段收口增加一个只读安全审查。

### 版本探测接口

- `native_probe_cli_version` 经原 authenticated document bridge 接收 `{ requestId, profileId, expectedProfileRevision }`，没有 argv、脚本、程序路径、URL 或环境输入。requestId 是 UUID，每次采集都绑定精确 profile revision。
- 返回 `{ requestId, profileId, profileRevision, cli, state, version, reason }`。state 为 `known | unknown | cancelled`；只有 known 带严格版本字符串，unknown 带固定原因。原始输出及异常不返回前端。
- `native_cancel_cli_version_probe` 接收 `{ requestId }`，返回 `{ status: 'requested' | 'settled' }`。取消先到也必须建立同文档 request fence；迟到开始不得启动进程。只有实际 child 已收口才发布 cancelled 终态。
- 只使用已选 profile 受控冻结环境与已验证程序路线执行固定 `--version`，不带 profile 自定义参数、用户 prompt、observer 或 hooks，不解析任意脚本字符串。不能安全证明启动路线时返回 unknown 原因，不阻止原生启动能力。
- 后端保留子进程/Job 所有权，设置超时和输出硬上限，取消后必须停止并回收准确 child；未知终止不能伪装 cancelled。文档撤销、profile 修改和退出会撤销新结果发布，但不能丢弃未收口 child。
- UI 仅在用户明确采集后读取当前选定 profile，按 CLI 和精确 profile/revision 区分结果，切换后丢弃迟到结果。复制摘要只输出 CLI、临时 profile 槽位、版本或固定 unknown 原因，不输出 profile 名称、ID、路径、凭据或原始输出。版本不能作为功能白名单。

## 后续 P0 本回合收口（2026-11-06）

本节更新上方阶段性“尚未采集”状态；0.27 整版仍未验收。已集成 CLI 版本安全采集后端 `50b353886fd21cbe6fb5c7f5acc1e82f225c35ff` 和 profile 独占 owner 修复 `5d230917a938b50a0aaa489ea2e653b6006b7dea`，原辅助提交分别为 `0439a07379050f860dc969462ab86edf020fc73a`、`496bcded0a528e38c30c77f5bd87dd517db2c2f7`。

关于页面由用户显式采集已启用 CLI 的所选 profile；刷新失败、来源/revision 变化和迟到结果均失效。固定版本参数、有界输出和超时、准确取消与未知 owner 保留、匿名来源白名单复制已接通。具体协议、固定源码依据及未证明项见 [CLI 诊断](cli-version-diagnostics-0.27.md)。

当时完成清理官方文档/固定源码交叉核查及归档抽屉拒绝说明。此早期阶段没有开放 mutation；后来已查明 Claude 官方 SDK 确有 `deleteSession`，缺口在真实来源/活动适配，旧“无接口”结论已纠正。用户随后明确永久删除需求，当前 Codex 窄场景及其原生 writer/reference 保护、二次确认、动态子树风险见 [最新实现与证据](permanent-delete-0.27.md)，不以 purge、rm 或猜路径替代。

隔离签名验收准备见 [矩阵](acceptance-0.27.md)，不是已运行记录。真实 0.27 签名候选仍缺，未新发布/安装。实际集成已通过 Rust 诊断域 21/21（1758 过滤）、check、strict Clippy、fmt；全量 Vitest 206 文件，3200 通过、2 原有跳过。最终构建/生产与界面结果和全部失败材料见 [.audit/v027-p0-validation.md](../.audit/v027-p0-validation.md)。

只读记录远端 main 已到 `0aaaf2a171ced55ff2b4d7a3e1731027c45809df`，未 fetch/合入其 0.26 发布改动；本任务仍保留 RC3 基线与独立开发分支。原 todo、BodyTimeout 和六项旧超时失败未删除。

## 第三阶段：P1 与完整验证

当前完整清单见 [验收清单](acceptance-checklist-0.27.md)，原始失败及命令见 [第三阶段记录](../.audit/v027-stage3-validation.md)。已补齐精确候选忽略、20 条本地脱敏更新记录及失败重新检查；旧准入不会因取消忽略恢复，未来版本仍可提示。独立审查的损坏存储提示和真实截图发现的忽略原因文案均有 RED/GREEN。最终前端全量 3206 通过/2 原跳过，末次更新域 57/57、专用视觉单元 51/51、生产 CSP 2/2、类型/构建及 Node 政策310通过/6原跳过。

首轮完整原生16片原始1744通过/36原ignore/18Job-free排除，但用了stable1.95，聚合按固定1.98.1要求拒绝；保留失败，不计合格整体验收。all-target还发现新增parser测试重复模块，已改引用生产模块而非豁免lint。固定1.98.1和CI profile下all-target Clippy、fmt已通过，下一轮完整原生验证将绑定新实现提交及不可混用的编译包。没有安装新环境或改真实用户数据。

### 第三阶段最终检查点

实现提交 `68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3` 的固定 **1.98.1 / CI dev/test debug=0** 四 harness、16/16 片与原聚合已全部退出 0：1744 通过、36 原 ignore、18 项 external Job 专项未验证、0 失败/超时。执行前已核对 source、compiler/profile、四文件 SHA256、plan 和 runner 参数；未改变原门槛，未混入 1.95 证据。七项必选 native/Wry 用例和四项 unelevated 专项均执行。原始 native All 和真实安装验收仍不成立。

原生结束后最后一次完整前端对同一实现提交为 207 文件、3206 通过、2 原跳过，退出 0。Node 310/6、类型/构建、all-target Clippy/fmt、CSP 2/2 和最终 P1 界面 2/2 均已有通过记录，未重复源码未变的耗时检查。完整命令、预检摘要、实现 SHA 索引、独立审查和原始失败保留在 [最终验证记录](../.audit/v027-stage3-validation.md)。原 todo、隔离 CLI 数据、两轮编译包/分片和超时 fixture 记录全部保留；仅追加验收文档与小型元数据提交。

已有足够证据准备候选审阅材料和隔离矩阵；整版 P0 尚未验收。原生清理完整预览/执行因上游条件语义缺口仍未交付，真实签名 0.27 往返、安装版/Unix/现场回归和真实 CLI D20 缺口见 [验收清单](acceptance-checklist-0.27.md)。没有合并 main、推送、发布、改用户安装或操作真实会话；当前无运行中长命令。

## 候选审阅与只读能力分解

后续文档收口基于 `3f60853a70ccf279c500c99d3cebb424e282f06a`，没有改已测实现。新增 [候选审阅材料](candidate-review-0.27.md)，将 P0/P1 实际交付、代码/测试、未完成与解阻条件展开。历史恢复明确拆分：官方目录/签名准备、普通签名安装及人工恢复备份、九包严格切换/返回，以及本轮无副作用的说明指南。普通安装不受九包表限制但不保证安装成功或自动返回，九包实际往返也未验收；0.26 不在严格九包范围。

新增 [只读预览复核](native-cleanup-readonly-preview-0.27.md)，复用固定源码，仅核实关系/list/status/来源。Codex 公开持久关系不等于包含 live 子树的原生级联；OpenCode 单次有界、同源且授权的全量查询有可行读取边界，但当前未证明 held DB 与无初始化副作用服务的同源通道。读取缺口与删除原子条件独立，不以删除不安全推断所有读取不可做。本轮不新增伪完整子列表，不启动 server 或查询真实用户数据；现有 OpenCode v2 来源支持也已准确记录。

本轮只做文档引用、事实与实现一致性核对及独立只读复核，不重跑已通过全套，不改版本号、tag、PR、main 或 Release。完整原始验证仍绑定 `68fa6b3…`，不能由后续文档提交改写。

## 有限切片：服务同源与正式 main 风险

从 `1c018f24bf52e6de421a84a397e28d052e1c5b8e` 继续，A/B 并行只读分析。A 的 [信任根核查](opencode-service-source-proof-0.27.md) 确认 OpenCode 有随机 instanceID、私有注册、认证与替换自停；但当前链路没有将实际服务进程/生命周期、实际打开 DB 对象和 held 来源关联。未读真实注册/凭据/会话、启动服务、写 DB nonce 或增加不能合法签发的 Proof 模型。独立安全复核无新的确定性错误，补入调度/恢复可能副作用的固定源码证据；当前不接预览或 mutation。

B 获准只刷新本 clone origin/main 并获取对象：[风险分析](main-0.26-integration-risk-0.27.md) 固定采样 main/tag 为 `0aaaf2a171ced55ff2b4d7a3e1731027c45809df`，merge-base 仍 RC3，开工 HEAD/main 独有 9/2 提交，改动文件 81/10、交集 0。仅预期文本冲突低，未做任何整合。正式 Release/CI 元数据及公开 coverage 字节绑定已核实，未重新下载/验签全部平台包；不会借用 0.26 证据晋升 0.27。

当前版本身份、历史 scope、发布/消费 parser 范围与未来整合验证顺序均已列明。本回合只做文档及一致性验证；不重跑完整套件，不改版本/发布策略或任何其他工作树。

## 正式 main 已整合到开发侧

新授权明确允许 main→feat 本地合并。刷新后仍为正式 main `0aaaf2a171ced55ff2b4d7a3e1731027c45809df`、原十文件；恢复 ref `refs/codex/v0.27.0/pre-main-aaad3d2b` 保留合并前 `aaad3d2bf1c05c40c30ab03d9d46c38d05f7b4cc`。实际 merge **`89e30ff37ca51b29239c17abbc52f0dc85106390`** 无冲突，没有改写历史或向main合入开发代码。

验证后测试提交 **`68436f31dc8a6313e9d87ca9ab35717671edfcfd`** / tree `acc7ec63f914b23f4388586e8d6f254d02ebbfd9` 增加0.26版本转换/通道回归；固定1.98.1及CI profile下Rust91、前端169、Node310/6原跳过、all-target Clippy/fmt、typecheck/build、生产CSP2/2均通过。六处应用身份0.26.0；历史普通安装与九包严格返回边界不变。新源完整16片/完整Vitest/hosted CI未跑，旧68fa全套证据仍按旧源保留。完整命令与未跑项见 [本轮验证](../.audit/v027-stage6-main-integration.md)。

同源切片新增进程/连接及file witness的官方语义和双DB同进程反例，没有接真实服务。此前优先隐藏/软归档的授权方向现已被永久删除要求替代；未提交片段完整保留为 `.audit/v027-stage7-superseded-*.patch`，再仅撤出自身改动。当前实际永久删除的窄场景、Claude SDK 合成反例与未交付边界见 [新文档](permanent-delete-0.27.md)。

## 永久删除窄实现已本地提交

实现提交 **`6de019a2ee22528d1c391e76c449c968c9846140`** 接通Windows授权profile下已原生归档Codex根的三阶段API、独立二次确认与双勾选、一次性票据、startup codexHome/rollout/header实核、容器句柄及整个own Job生命周期、unknown只读协调。确认范围是根及其原生动态子树，计数未知不编造；CLI关联维护、可能保留的数据与实际未跑边界明确。OpenCode/Claude执行和Unix分别关闭，不用旧隐藏/软归档或猜路径删除替代。

最终固定1.98.1/CI debug0新Rust域7pass（1780过滤）、all-target Clippy/fmt、前端10文件408pass、Node安全9pass、typecheck及重建生产入口中英CSP2/2通过。独审P1派生进程逃逸、发送前失权、迟到预览、跨目标重新预览等均先RED再修复；原失败未覆盖。准确命令、源码tree/hash、失败/未跑项见 [完整审计](../.audit/v027-stage8-permanent-delete.md)；后续仅文档记录，不再更改已验证产品源码。没有新源完整16片/完整Vitest、真实CLI/WebView删除、真实签名候选往返或发布。
