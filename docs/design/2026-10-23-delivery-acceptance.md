# 交付分解、验收与回退设计

日期：2026-10-23（2026-10-24 按对抗审查修订）。状态：后续实施计划，本文没有宣布任何新增功能或目标验收已完成。配套：[核心设计](2026-10-23-workspace-evolution.md)、[Git/worktree 设计](2026-10-23-git-workspaces.md)、[原研究规划](../project-optimization-plan-2026-10-23.md)。

## 1. 排期原则与重新估算

本地工作区与当前 main 相差 77 个提交（实测本地为 main 纯祖先、无独有提交），本轮不切分支；实施时可直接快进到届时 main，仅需先行保留本文档等未跟踪产物。实际编码从重新确认的 main 开始，先重读 diff 和本目录设计，再做范围修正；禁止按旧分支的功能缺口直接补写。

| 工作包 | 更新估算 | 变化原因 / 完成边界 |
| --- | --- | --- |
| W0 能力与事实目录 | 3–4 人日 | 增加生成、证据 schema 与历史/current 区分 |
| W1 兼容与目标证据 | 8–12 人日 | 保留原量级；外部账户/目标等待另计。B02 开工前冻结版本/平台格数与 driver 工作量，格数显著超出时重估 |
| W2 依赖处置 | 4–7 人日 | 仅处理届时仍存在的项，major 迁移另估 |
| W3 上手与恢复 | 4–6 人日 | 复用现有启动流程和固定错误码 |
| W4 关注入口 | 6–8 人日 | 纳入 Legacy 精确确认、快捷键迁移和所有确认入口 |
| W5 测量及已证明优化 | 5–8 人日 | 允许只交付基线，未证明收益不做重构 |
| W6 Git 审阅 | 12–18 人日 | 含 G01 的 3–5 日验证、scope、raw snapshot、UI 与负面测试 |
| W7 worktree | 10–16 人日 | 仅接入/准备/登记，含持久操作记录与故障注入；不含自动 checkout/物理删除 |
| W8 文档与用户反馈 | 3–5 人日 | 含演示和 D01a/D01b 两轮各 5 人任务观察；招募等待另计 |

合计 **55–84 工程人日**。这是工作包级下界估算：变更单级工时在排期时逐单补充；W1 的版本/平台格数与真实 driver 工作量在 B02 开工前冻结；W6/W7 总量在 G01 选定实现路线后按结论重估，当前数值视为条件性容量区间。1 名主要维护者、可预约的独立评审/QA/目标机下，可按约 13–20 周的滚动容量考虑，不能将它承诺为固定上线日期。原 43–66 人日/10–14 周属于较粗的规划估算，由本表替代；技术验证 No-Go、上游 major 迁移或新增平台机制须重新估算。

W0–W5＋W8 合计 33–50 人日，可独立形成有价值的一轮改进。资源不足时先完成这一组，暂不排 W7；Git 审阅属于有退出条件的验证工作，不捆绑基础改进。

## 2. 可逐项交付的变更单

下列编号用于任务和证据追踪，不是已经创建的 GitHub issue/PR。每项按“定义 → 后端/调用方 → UI → 行为验证 → 文档”推进；新增文件名是建议落点，可以在边界不变时调整。所有实现者先读根及目标目录 AGENTS，涉及 CLI 读 native-cli-v3，UI 读 DESIGN。

| 编号 | 目标与主要文件 | 前置 | 独立完成条件 |
| --- | --- | --- | --- |
| B01 | W0：新增 `resources/cli-compatibility.v1.json`、validator/generator、`src/generated/cliCompatibility.ts`；`sourceRevision` 沿用 `CC_DESK_BUILD_SHA`，无该变量的构建标 working-tree；同步 README/roadmap 的能力说明 | 当前 main 基线确认 | 同源生成、非法证据被拒绝、历史 PASS 不显示 current PASS、working-tree 构建不产生 current 声明；不改原测试库存 |
| B02 | W1：在 `docs/testing/cli-evidence/` 增加证据索引和已有目标 driver 的明确清单 | B01、授权目标 | 每个目标格有精确身份与结果；不能执行的格明确 blocked/not-run，无空白隐含支持 |
| B03 | W2：逐项锁文件可达性/处置报告，必要时小版本补丁与对应回归 | 当前 main、官方公告 | 每 advisory 有处置与证据；不以 override/ignore 绕门禁 |
| B04 | W3：`cli/compatibility.rs`、`version_probe.rs`、commands/runtime/bridge；`recoveryGuidance.ts`、现有启动组件 | B01 | 显式 probe 无任意执行入口；重复 request/切配置/超时正确；不隐式开始 CLI |
| B05 | W4：`unifiedAttention.ts`、Legacy attention 精确 revision、Native adapter、`useUnifiedWindowAttention.ts` | B01、现有状态语义核对 | 旧确认不能清新事件；所有自动/人工确认入口共享同一规则；无双 owner |
| B06 | W4/W3：现有 AppShell/context slot、关注面板、`appShortcuts.ts`、快捷键持久化调用方 | B04、B05 | 旧五字段配置保留、默认不占快捷键、真实焦点后确认、终端实例不变 |
| B07 | W5：扩展既有性能 fixture/报告及有限 diagnostics；有数据后另做局部优化 | B01、目标 WebView | 固定场景/基线/样本/失败记录齐全；优化与基线同口径，不动完整压力工作量 |
| G01 | W6：隔离 fixture 技术验证，Git 版本/布局/执行策略准入报告，并列比较 raw 引擎 / Git 枚举 + 内存 diff 库 / staged-commit-only 三条路线并选择 | Git 设计审查 | 三类输出：可支持目标、失败案例、明确 No-Go；未通过的目标按降级阶梯逐级关闭 |
| G02 | W6：`workspace_git` scope/process/snapshot/diff（按 G01 选定路线实现，blob 读取必须 `cat-file --batch` 单进程）、commands/lib、typed API 与 Rust 测试 | G01 对目标 Go | 只读范围、内容/时间预算、未知 owner、path/filter/network 负例全部有证据 |
| G03 | W6：`workspaceGit.ts` store、`ProjectChangesPanel.vue`、AppShell 变更模式 | G02、B06 | 空/忙/部分/过期/异常都可理解，切项目无串数据；真实 WebView 无终端回归 |
| G04 | W7：workspaces/journal、sidecar CAS、admission 辅助类型、恢复/retire、项目注销屏障及全部 remove 调用方 | G02；维护门禁审查 | 故障注入通过；重启先恢复阻塞；无法证明时不放行；注销不丢恢复入口 |
| G05 | W7：接入已有 worktree、关系展示、仅解除 Desk 关联 | G04 | 多 worktree 身份/登记对账正确；detach 保留用户文件、project、Git 状态 |
| G06 | W7：准备新分支/worktree、初始化指引、完成登记 | G04、G05、G01 对写路径 Go | 每次确认最多一项 Git 效果；中断不重放、不回滚用户数据，真实双副本可独立启动 |
| D01a | W8 第一批：快速上手、恢复/关注演示，5 人任务观察 | B04/B06 | 完成启动/关注/恢复观察；不等待 Git，也不验证尚未交付的审阅 |
| D01b | W8 第二批：Git 变更演示，5 人审阅观察；可回访同批用户 | G03、D01a | 用户理解 raw/tracked-only 限制，结果用于决定 W7；招募/等待另计 |

不要将 B02 解释为“一次 PR 必须完成所有外部目标”；框架和逐格证据可分批合入，但产品声明只能提升已验证格。B03、B07、D01a 可在满足各自前置后独立推进，不允许多个实现者同时修改同一共享工作区。

## 3. 里程碑和决策门

| 里程碑 | 包含 | Go 条件 | No-Go / 延后时保留什么 |
| --- | --- | --- | --- |
| M0 可解释现状 | B01、B02 目标清单、B03 处置清单 | 当前/历史能力不混淆，版本/目标资源明确 | 目录和文档可交付；缺目标不宣称认证 |
| M1 现有使用闭环 | B04–B07、D01a | 启动恢复无隐式副作用，关注精确确认，压力/终端回归通过 | 保留原 UI 路径；未测性能不宣布提升 |
| M2 Git 可用性试验 | G01–G03、D01b | 支持平台/布局通过负例，用户能辨认 raw diff 局限并完成审阅 | G01 No-Go 则不提供 Git 按钮；继续 M1 |
| M3 可选工作副本 | G04–G06 | 幂等/持久恢复/双副本真实验收通过，用户愿意接受手动初始化 | 仅交付 G05，或继续用原 Projects 手动登记 |
| M4 发布收口 | 目标源码 required checks、签名/安装/升级回退 | 满足原 release-process；所有声明与证据一致 | 保留候选，不放宽签名/CI 或使用历史 run 代替 |

“仅接入已有 worktree”也需 G01 的只读身份检查，不允许为赶工绕过；如果它也不通过，只保留现有手动注册普通项目路径。

## 4. 行为验收矩阵

建议测试名遵循 `Feature_SubFeature_SeqNum`，括号后的中文是目标。下表是最低行为集合，不要求按文档逐行做镜像断言。

| 组 | 正常 / 边界 / 失败场景 | 必须成立的不变量 |
| --- | --- | --- |
| `Compatibility_Match_*` | 精确版本、未来版本、缺版本、旧 SHA 的 PASS、重复 case/evidence ID、未知 layer | 实现声明与验证结果分离；非法索引失败；不误启用附加能力 |
| `Compatibility_Probe_*` | 选定程序成功、并发重复 ID、profile/program 换代、超时、stdout 伪造/超限、reap 不明 | 只执行确切程序一次；原始输出不泄露；旧结果不覆盖新配置 |
| `Setup_Continuation_*` | 程序确认后继续、取消/切项目/切 CLI、CAS 冲突、launch unknown、输入 partial | exact intent continuation；没有二次 start、输入重发或回退默认 root |
| `Attention_Receipt_*` | 快速连续两通知、聚焦前后新事件、Native generation 换代、历史 claim 失效、Legacy PTY 复用 | 旧确认不清新回执；同 ID 跨来源不串；活动等待状态仍保留 |
| `Attention_Lifecycle_*` | owner 关闭/替换/移动、未知时间、4,096 项超限、列表打开但未聚焦、同 cause 稳定刷新 | 释放监听、旧新项目都正确刷新、未展示项不被确认、无排序抖动 |
| `Shortcut_Migration_*` | 旧五字段/自定义/显式 null/未知字段、IME、终端焦点、模态冲突 | 原绑定完整保留；新增默认 null；不窃取 CLI 输入 |
| `GitScope_Admission_*` | 普通/linked worktree、裸仓库、外部 .git、换 document、隐藏/注销项目、程序换代 | 每次 admission；ID/path 不能复活撤销 scope |
| `GitScope_Path_*` | 根/父目录替换、junction/symlink、object alternates、畸形 gitdir、恶意 local include、异名字节同显示 | 无越界读取、无误绑定；不支持布局明确拒绝 |
| `GitProcess_Policy_*` | external diff/textconv、clean/smudge/process、fsmonitor、hook、pager、SSH/代理、promisor 缺对象 | fixture 的执行/联网哨兵计数为 0；没有原 index/ref 写入 |
| `GitSnapshot_Content_*` | unborn、删除/新增/模式、冲突 stages、gitlink、binary、CRLF、LFS、CJK/空格/控制字符路径 | 正确说明 raw/tracked-only；不跟随链接，不将截断项标 clean |
| `GitSnapshot_Race_*` | HEAD/index 切换、文件读取中变更、过期 cursor、切项目迟到结果、预算边缘和超限 | stale/partial 可见；单个 diff 只取冻结字节；不混快照、不串项目 |
| `Workspace_Adopt_*` | 已登记/未登记 worktree、detached/unborn/失效分支拒绝、同路径不同身份、注册成功 receipt 丢失、detach 有运行会话 | 收敛到准确项目；不终止会话，不删除项目/文件/分支 |
| `Workspace_Prepare_*` | 新分支、名称冲突、过期计划、父根替换、重复 confirm、Git 非零但有残留 | 最多一次尝试产生效果；不 force、不自动 checkout、不以失败推断无效果 |
| `Workspace_Recovery_*` | journal 损坏/未知版本、持久化阶段崩溃、跨进程竞争、外部移动、PID 复用、维护冻结 | 保留未知记录；只读对账；旧 PID 不变成控制权；原 CLI 仍可用 |
| `Workspace_Register_*` | Git 成功/Desk 未写、项目已写/sidecar 未写、CAS 竞争、重启重复完成登记 | 不重跑 Git；不丢原 workspace 字段；有证据才复用 projectId |
| `Workspace_Retire_*` | 终止已证明/未证明、坏 journal、错 digest、重复 retire、外部注销父项目 | 只有确切 proof 能解除 owner 阻塞；保留原记录与文件；恢复入口不授予项目读取 |
| `Workspace_ParentGuard_*` | plan 与 remove 并发、未解除关系、隐藏、回执丢失、sidecar/journal 损坏/未知 schema、完全未初始化、显式 resolution 隔离后重开 | 屏障消除竞争；损坏默认全局拒绝注销及新副本计划但保留 CLI 使用；resolution 隔离后可核对重开；正常空集合不误阻塞 |

进程策略测试必须真实执行隔离 Git fixture，通过 sentinel 文件/计数器与禁网 harness 检查副作用；仅 mock argv 数组不证明未触发 filter。权限/路径 fixture 不修改用户仓库，不执行用户配置中的程序。行为测试同时捕获日志/IPC，断言无原始 secret marker、env 或异常输出。

### 4.1 worktree 故障注入检查点

至少在下列动作的前/后分别注入 crash、I/O 失败或 receipt 丢失：持久 planned、持久 executing、创建目标空目录、目录身份写入 journal、spawn 开始、spawn 返回、Git 写 branch、Git 建 admin/worktree、子进程退出、真实 reap、保存 needs-initialization、用户完成初始化检查、原 workspace CAS、保存 sidecar、保存 ready、返回 UI。

每个样例重启两次，读取真实文件/ref/project/journal 核对：不产生第二个 Git 效果、无用户文件丢失、状态收敛或明确 reconciliation-required。exit 0、进程不存在、文件存在都不能单独证明事务完成。测试库存保存“注入点 → 预期允许状态 → 实际状态/证据”，超时和未执行不能算 PASS。

## 5. 验证命令与证据层

这些是实施时应运行的入口，不表示本次已运行。当前纯文档修改只检查设计、引用和一致性。

| 修改范围 | 最小本地入口 | 扩大验证的条件 |
| --- | --- | --- |
| 能力 schema/生成 | 新 validator/generator；`npm run typecheck`；新能力数据行为测试 | 生成影响首屏/构建资源时 `npm run build`、生产入口检查 |
| 启动/关注/UI | `npm test -- <实际受影响测试文件>`；`npm run typecheck` | 跨 adapter/shell 后 `npm run test:fast`、压力、视觉、production |
| Git/工作副本 Rust | 根目录 `node scripts/prepare-conpty.mjs`，然后 `src-tauri` 内 `cargo test --locked --lib <实际过滤名>` | 共享 admission/存储/commands 变化补相关现有测试；交付前原 CI 全库存 |
| 真实 Git/helper | 新进程测试加显式受支持版本 fixture | 不能被普通 DOM/mock 测试替代；Windows/macOS/Linux 分别记录 |
| 终端/会话压力 | `npm run test:stress` | 完整 5,000 会话、30 owner、120 次变化，保留原工作量 |
| 视觉/生产 | `npm run test:visual:unit`、`npm run test:visual`、`npm run test:production` | 均不能替代安装包 WebView2/系统 WebView、缩放和真实聚焦 |

Rust 过滤前用 `-- --list` 确认实际匹配的名称/数量，0 个匹配不算通过。若项目未提供独立 npm lint 入口，使用已有前端检查与 CI static/Rust Clippy，报告实际入口，不编造 `npm run lint` 成功。运行长测试前说明范围和成本；通过最小环后，只有跨模块风险、失败或新改动才重复扩大。

证据层明确区分：A 是静态/单元与宿主契约；B 是真实进程、CI/浏览器或安装包宿主证据，必须写具体 target；C 是授权真实 CLI 账户的实际交互。B 内浏览器与安装包也不能互换。现有 D20 Layer-C、nativeAll、Windows Job-free/elevated 专项沿用原定义；本目录不创造更宽松的等价项。

### 5.1 首轮 C 层必测

对每个对外声明已验证的 CLI/version/platform：首次启动、原生权限提示、正常文本、完整 bracketed paste、取消/中断、resize、会话恢复、历史来源、活动/关注、关闭与重启。实现声明为 unsupported 的功能记录 N/A 及原因，不伪造通过；可用的 image/rename/unarchive 等附加能力按 featureId 加案例。

每条证据绑定同一源码、安装包/driver/CLI binary chain 摘要、目标平台、时间、run/attempt 和结果。真实 `UserPromptSubmit` 正文相等仍需它自己的证据；模拟 echo、浏览器按键、Unit PASS 不能替代。

## 6. 性能与可用性验收

下列阈值是建议验收预算，B07 首次基线报告同时冻结机器/样本方案；修改预算必须记录原因与用户可见影响，不在失败后直接放宽。

| 场景 | 工作负载 / 建议阈值 | 失败处置 |
| --- | --- | --- |
| 会话切换 | 同机已运行终端，p95 ≤100 ms；对基线不得回退 >10% 且 >10 ms | 找实际瓶颈；不重建实例、减少缓冲或跳过 fit |
| 合成输入显示 | 固定 echo payload，p95 ≤50 ms；同等负载回退不得 >10% 且 >5 ms | 单独区分进程/传输/呈现，不能归因模型推理 |
| 首屏 | 正常生产入口可交互 p95 ≤2 s；资源/语言/缓存条件固定 | 保留原始失败；不只拿 dev 或预热结果 |
| Git 小仓库 | ≤1,000 tracked 文件、≤10 MiB 捕获，snapshot p95 ≤2 s（要求 `cat-file --batch` 单进程读取；逐 OID 启动进程视为未达标） | 超限/超时有明确状态，不展示空结果 |
| 内存 | 原完整压力场景，关闭/GC 后无持续增长；与基线稳定 RSS/heap 比较 | 分别列 JS、Rust、终端/子进程，不能拿单次峰值宣称泄漏修复 |
| 首次使用 | 已安装/已认证 CLI，5 人中至少 4 人无需帮助完成新建并进入终端，目标 ≤3 min | 分类记录卡点，先修流程再扩能力；小样本不宣称统计显著 |
| 关注/恢复/审阅 | 5 人中至少 4 人定位待办并说明下一步；能辨认 raw diff 的限制 | 若不能理解，调整文案/交互；不足以支持 W7 则延后 |

延迟场景每目标预热 10 次、采样 100 次；冷首屏单独做 30 次新进程启动、不预热 renderer，冷热结果不可混。外部失败、timeout 计入结果和分母。真实账户调用不套用 100 次合成样本方案，先限定授权预算。报告 p50/p95、失败率、样本范围、硬件与 workload ID；依赖 CI 时间单独报告，不归入应用交互性能。

## 7. 功能开关、迁移与回退

新增后端构建策略明确列 `gitReview`、`worktreeAdopt`、`worktreePrepare` 的支持平台/布局与 enabled 状态。未完成验收默认为 false；前端读取 capability DTO 显示入口，后端 admission 仍执行相同政策。不能只隐藏按钮而让 IPC 写入口可调用；用户设置也不能跳过准入。

能力目录和 probe 无用户数据迁移；关注投影不持久化。快捷键迁移只补 optional key，并通过现有串行 writer 保留旧键。sidecar 新增独立版本，不修改 CLI workspace schema，不替换用户 CLI 配置。

回退顺序：关闭新增操作准入 → 等待/对账现有 owner → 保留 journal/项目/用户文件 → 按原升级/降级门禁执行。已经创建的 worktree 不因回退被删除；旧版本忽略 sidecar，但现有普通项目仍能访问。Git 状态与 sidecar 不能整体恢复到旧快照以“抵消”外部 Git 写入。

此保证仅适用于新版本执行维护预检后进行的受支持回退。旧二进制不理解新 journal，不能声称用户直接并行启动旧版本也会拦截未知 helper；该情形不提供跨版本互斥保证。实施需将新增 journal 纳入现有维护/备份的明确库存，不能让恢复旧 workspace 快照抹掉 Git 已产生效果的记录。

如果 sidecar 损坏，Git/worktree 增强功能 unavailable，现有会话与项目列表继续工作；依赖集合不完整时所有项目注销及新副本计划返回 `WORKSPACE_DEPENDENCIES_UNAVAILABLE`，普通注册/隐藏/CLI 仍可用；用户可在安全诊断通过显式 resolution 把损坏文件隔离（保留原字节与 digest）后重新核对开放注销。如果存在未确认 helper 或 mutation，需要静默状态的维护动作继续 blocked。按 Git 设计的 recovery/retire 协议，在 owner/文件完整性有确切证明后可停止管理并保留数据；证据不足继续 blocked，不能改名/删 journal 或口头确认绕过。完全不可恢复的证据损坏保留离线诊断风险。

## 8. 发布完成定义与外部资源

每个实施变更单完成定义为：代码/调用方/文档同步；相关行为测试通过；类型/静态检查通过；新增边界有负例；原有结果无回归；所有未运行目标明示。M4 仍要求 [发布流程](../release-process.md) 的精确 protected-main、普通 CI 原库存/coverage、产物字节和三平台签名，不能拿本地成功代替。

18 条 Job-free、4 条 elevated 专项以及现有真实 CLI/安装包缺口只按实际政策披露；本方案不新增跳过项，也不把“证据缺口”当作已证明产品缺陷。发布前重新取精确 source/run/attempt 证据，不沿用 2026-10-23 研究时看到的 CI 结果。

实施时需要的外部资源：三平台隔离目标、各 CLI 的授权测试账户、确定版本的 Git/CLI 二进制、测试费用上限、额外 CI 校准窗口、5 名目标用户。凭据不进入聊天或仓库。缺少资源时对应验收格 blocked，独立工作可继续；不需要为完成本文暂停追问。

推荐启动顺序已经确定为 **B01 → B02/B03/B04/B07 → B05/B06 → G01**。是否进入 G02/G06 由技术证据和用户任务观察决定。当前无需再选择产品架构；实际投入时间、账户与费用在具体实施前确定。

## 9. 本次设计交付的验证记录

本节记录 2026-10-23 设计细化的实际检查，不是上文未来功能的验收结果。本轮新增三份设计并同步原规划，未修改产品代码、依赖、用户配置或 Git 分支。

| 实际检查 | 结果与范围 |
| --- | --- |
| Node 文档校验（PowerShell here-string 管道执行 `node`） | 5 份文档：96 处链接，其中 38 处本地目标存在、58 处外部 URL 格式有效；11 组代码围栏闭合，表格列数、尾部空白、UTF-8 替换字符检查通过；15 个交付任务 ID 唯一 |
| `git diff --check` | exit 0；新增文件尚未跟踪，其内容由上一项直接读取检查覆盖，不能只靠该命令证明 |
| 定向源码检查 | 核对 document bridge、关注确认、项目 remove 调用链、maintenance admission 与存储边界；没有执行 Git/worktree 产品操作 |
| 独立 builder-check 与复核 | 6 项影响实施的问题已补齐并复核关闭：未知操作收口、父项目注销、adopt 状态/HEAD 范围、异步 snapshot、用户研究依赖、损坏账本下的注销策略；结论为没有剩余重要实施澄清项 |

外部链接没有全部逐条重新联网；本轮重新核对了 main 基线和有关 Git 官方协议说明。前序规划记录的 typecheck/39 项测试属于原分析阶段证据，本轮纯文档不重复跑产品类型、lint、Rust、构建或真实目标测试。新增功能性能、平台、恢复与 C 层验收仍全部待实施，不能从本节的文档检查推出通过。

## 10. 对抗审查裁决记录（2026-10-24）

两段独立审查：主代理自审（源码核验设计对现有代码的断言）+ Codex 独立对抗（源码与公开 API 核查，未读主代理结论）。以下为逐条裁决，全部条目均已回写文档；未发现需要驳回的伪事实。

| 编号 | 问题 | 严重度 | 裁决 | 落点 |
| --- | --- | --- | --- | --- |
| C1 | confirm 复核"空目标目录身份"与"创建目录在持久 executing 之后"顺序互斥 | P1 | 采纳 | Git 设计 §6.1 固定为"复核→持久意图→建目录→存身份→spawn"，并补各崩溃窗口恢复态 |
| C2 | 坏账本无限期阻止全部项目注销，无解除路径 | P2 | 采纳 | Git 设计 §6.4 增显式 resolution（隔离保留原字节）与分级说明；验收表同步 |
| C3 | W6 选高成本 raw 方案未比较更小安全交付 | P2 | 采纳 | 总规划 W6 + Git 设计 §8：G01 并列三路线 |
| C4 | 快照 TTL 120 秒与人工审阅时长不匹配 | P2 | 采纳 | Git 设计 §4.3 分离新鲜度与存活期 |
| C5 | 1,024 账本上限成累计寿命上限，终态无回收 | P2 | 采纳 | Git 设计 §6.2 增终态归档摘要 |
| C6 | 验收阈值跨文档漂移（150ms vs 100ms）、采样口径不一 | P2 | 采纳 | 总规划 §6 统一为交付文档口径 |
| C7 | `MaintenanceGate` 符号不存在，实为 `AdmissionGate` | P2 | 采纳 | Git 设计 §3.3 更正 |
| M1 | G01 在 Windows 可能 No-Go 但无降级形态 | P0 | 采纳 | 总规划 W6 + Git 设计 §8 增三级降级阶梯 |
| M2 | 未写死 `cat-file --batch`，2 秒 p95 不可达 | P1 | 采纳 | Git 设计 §3.2、验收表、G02 |
| M3 | W7 手动初始化无渐进路径 | P1 | 采纳 | Git 设计 §1 声明为保守选择，G01 证实后可 opt-in 受控 checkout |
| M4 | 未评估内存 diff 库，攻击面可减半 | P1 | 采纳 | Git 设计 §3.2 内存库为首选，外部 diff 为回退 |
| M5 | `sourceRevision` 注入仅 release workflow 有值 | P2 | 采纳 | 演进设计 §4.1 + B01 |
| M6 | 多 worktree 登记为普通 project 后 UI 身份缺口 | P2 | 采纳 | Git 设计 §7 定义显示名与父项目标注 |
| M7 | 本地分支为 main 纯祖先，"保留原状"表述误导 | P2 | 采纳 | 总规划 §1、交付 §1 更正为可快进 |

未采纳为独立条目但已并入上述修订：Codex 指出的"直接受控 `git diff` 不能未经证明称为同级安全替代"写入 G01 约束；`--no-index` 非 plumbing 且仍读 attributes 的限制写入 §3.2 回退分支。
