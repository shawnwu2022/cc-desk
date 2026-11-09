# GitHub AI 工作台调研与 CC Desk 优化方向

调研日期：2026-10-23。本文是项目规划的外部证据附件，不是发布认证、竞品安全审计或实现清单。

## 结论

CC Desk 适合继续服务于同时使用 Claude Code、Codex 和 OpenCode 的重度开发者，强化“知道哪个会话需要我、准确回到该会话、看懂产物、可靠恢复”的工作链路。多 agent 数量、聊天界面、Provider 管理和云端 IDE 的功能堆叠不构成适合本项目的优化目标。

建议按五个方向推进：**CLI 版本兼容与恢复证据 → 扩展现有关注管理 → Git 只读审查与受控 worktree → 有限产物预览 → 远程只读观察验证**。优先级是本次分析判断，不是 GitHub 热度排名。主规划应先核对本地分支与最新主线，已有 quick switch、关注队列、诊断、状态通知等能力应做完善，不能重复立项。

## 调研方法和边界

- 共选取六个相关同行或互补工具，加 Codex、OpenCode 两个上游参照。筛选依据是产品机制与 CC Desk 的相关性、公开关注度、近期仓库活动；不是“GitHub 全站前八名”。
- 仓库数据直接来自 GitHub connector 的 GitHub REST API；README 使用固定 commit 链接，并重新核对固定 commit 返回的文件 blob SHA 与读取内容一致。星数、最后推送和归档状态是本次查询快照，不保证持续不变。
- “近期活动”仅指 `pushed_at`；不等于稳定版本发布、维护承诺、质量或实际用户数。星数只代表关注度。网页可能使用缓存，官网数字与 API 小幅不一致时采用 API。
- 功能证据来自项目自己的 README、文档和源码；没有安装、运行或压测这些产品。Issue 只作为需求或风险样本，不把用户报告当作已复现缺陷，也不据少数反馈推断市场规模。
- 本项目边界以 [项目约束](../project-contracts.md) 和 [Native CLI v3](../native-cli-v3.md) 为准：真实 CLI 拥有交互、认证、权限和配置语义；CC Desk 负责宿主、生命周期、传输、恢复和受鉴权的只读投影。本文不改变这些约束，也不把旧账本的 `BLOCKED` 自动当作当前发布状态。

## 公开关注度和活动快照

以下推送日期均为 UTC；八个仓库的 API 均返回 `archived=false`。

| 项目 / 仓库 | Star | 最后推送日期 | 许可证信息 | 对比角色 |
| --- | ---: | --- | --- | --- |
| [Emdash](https://github.com/generalaction/emdash) | 6,060 | 2026-10-23 | Apache-2.0 | 任务、worktree、Git 审查；[API](https://api.github.com/repos/generalaction/emdash) |
| [cmux](https://github.com/manaflow-ai/cmux) | 28,369 | 2026-10-23 | 主体 GPL-3.0-or-later，部分服务目录另用 BUSL-1.1 | 终端优先、关注管理；[API](https://api.github.com/repos/manaflow-ai/cmux)、[LICENSE](https://github.com/manaflow-ai/cmux/blob/main/LICENSE) |
| [AionUi](https://github.com/iOfficeAI/AionUi) | 33,727 | 2026-10-19 | Apache-2.0 | 多 agent、产物预览、远程入口；[API](https://api.github.com/repos/iOfficeAI/AionUi) |
| [Happy](https://github.com/slopus/happy) | 24,279 | 2026-10-23 | MIT | 跨设备会话和移动端；[API](https://api.github.com/repos/slopus/happy) |
| [CloudCLI / Claude Code UI](https://github.com/siteboon/claudecodeui) | 14,255 | 2026-10-22 | AGPL-3.0 | Web / 移动端、文件与 Git；[API](https://api.github.com/repos/siteboon/claudecodeui) |
| [CC Switch](https://github.com/farion1231/cc-switch) | 147,967 | 2026-10-23 | MIT | 配置与 Provider 的互补工具；[API](https://api.github.com/repos/farion1231/cc-switch) |
| [Codex](https://github.com/openai/codex) | 130,585 | 2026-10-23 | Apache-2.0 | 上游 CLI 与公开协议；[API](https://api.github.com/repos/openai/codex) |
| [OpenCode](https://github.com/anomalyco/opencode) | 220,458 | 2026-10-23 | MIT | 上游 CLI、版本演进与公开协议；[API](https://api.github.com/repos/anomalyco/opencode) |

cmux 的 API 许可证字段为 `NOASSERTION`，上表采用仓库 LICENSE 的具体声明。这里比较交互和架构机制，不据许可证摘要作代码移植决定。

## 值得借鉴的机制，以及适配边界

| 项目 | 一手资料确认的相关机制 | 对 CC Desk 的启发 | 不直接照搬的部分 |
| --- | --- | --- | --- |
| Emdash | 任务可从分支、issue、PR 开始；默认独立 worktree，也能选择已有工作区。Diff 视图区分 Changed / Staged / PR，并展示检查状态。官方明确保存的对话和运行进程具有不同生命周期。[任务文档][emdash-tasks]、[Diff 文档][emdash-diff] | 把会话、任务、分支和可审查产物关联起来；先实现受范围约束的只读差异，再评估显式 worktree 操作。 | GUI 模型选择、全套 Git 写操作、自动同步全局 CLI hooks、任务调度都扩大所有权。其 README 提到向用户级配置安装标记 hooks，不应成为 Desk 的默认策略。[README][emdash-readme] |
| cmux | 终端标签呈现分支、PR、目录等上下文；需要关注的面板和标签提示；通知面板聚合未读并可直接跳转。恢复区分布局/滚动记录和活进程，支持有原生 session ID 的 agent 恢复。[README][cmux-readme]、[通知文档][cmux-notify] | 完善现有关注队列：对来源、未读、待输入、完成、失败作清晰区分；从提醒直接定位正确 owner/run；让恢复类型和效果可理解。 | macOS 原生实现和 Ghostty 的性能叙述不能证明 Tauri/WebView 的优劣；自动执行恢复命令、socket 控制、云端能力不能绕过 Desk 的文档鉴权、精确身份和不重放规则。 |
| AionUi | 自动发现 CLI、并行会话；预览代码、Markdown、图片、Diff 等产物；另有 WebUI、聊天平台入口、定时任务和 team 编排。[README][aion-readme] | 学习发现入口、结果就地预览和状态标识，缩短“运行完成到确认结果”的距离。 | 内置 agent engine、Team MCP/共享可写目录、统一配置写入和无人值守调度超出本项目宿主边界；共享文件夹也不等于并行文件隔离。 |
| Happy | 当前主仓包含移动应用、原始 CLI 和加密同步 relay；新产品已有独立 desktop 与 Happy Agent runtime。原始 CLI 仍包装 Claude Code / Codex，但已进入维护模式。[README][happy-readme]、[原始 CLI 文档][happy-security] | 验证用户离开桌面时是否只需要低频关注提醒与安全只读状态；若需求成立，再单独设计设备配对和授权生命周期。 | 新 Happy Harness 是运行时方向，不能借“移动伴侣”之名移植为 Desk 的第二个 agent。官方 E2E 设计是参考，不等于本次已审计其实现。 |
| CloudCLI | 响应式 Web/移动 UI、会话恢复、shell、文件树、Git Explorer、浏览器及插件体系；支持本地与远程环境。[README][cloudcli-readme] | 参考跨设备的信息密度与项目/会话定位；验证用户真正需要的远程操作集合。 | 结构化聊天、可执行服务插件、完整文件编辑器、MCP 配置写入、托管云环境增加显著产品与授权面，不宜一起导入；直接复用代码还需单独核对 AGPL 条件。 |
| CC Switch | Provider 切换、配置预览与备份、MCP/Skills 管理、会话检索、CLI 版本与重复安装诊断；已是独立配置工具。[README][ccswitch-readme] | 借鉴“当前状态可见、错误可诊断、外部工具分工清楚”；Desk 的启动诊断可告知所选 CLI/路径/版本，配置操作仍交给外部工具。 | 不进入 Provider/API Key、请求路由、模型聚合和第二套 CLI 配置写入器；不把现有只读资源面板改造成配置中心。 |

### 上游不是普通竞品：按具体版本理解能力

Codex 官方仓库已有 CLI、IDE、desktop 和 cloud 入口。当前 app-server 文档的实验性 `readState` 使用线程级 revision，读取快照不会确认未读，更新必须匹配用户实际看见的 revision，冲突不能盲目重试。这为关注状态的并发语义提供参考；不意味着 Desk 需要改用 app-server 接管原生交互。[README][codex-readme]、[app-server 文档][codex-appserver]

OpenCode 的**开发分支 `dev`** README 已转向 `@opencode/cli` 与 v2 文档，但 GitHub `releases/latest` 本次仍返回 **v1.19.4**（2026-10-22 发布，`prerelease=false`）；其 tag README 仍使用 `opencode-ai`。这不能被表述为所有稳定版用户已经升级 v2。v1.19.4 自带 server 文档公开了 OpenAPI、健康/版本 API 和 server password 保护方式，README 也列出官方 desktop beta。[开发线 README][opencode-readme]、[最新非预发布版本][opencode-release]、[v1.19.4 README][opencode-stable-readme]、[v1.19.4 server 文档][opencode-server]

**不能因为品牌和命令名称相同，就假设不同版本的存储、恢复、插件和 API 契约相同。** 应分别记录已支持版本、最新稳定版本和开发线实验结果，针对实际 binary/version 做能力探测与真实验收。附加能力不兼容时标明不可用，不自动改配置、重建数据库或宣称其他版本也受支持。

两者都在扩展官方 GUI，因此 Desk 的长期价值应来自跨 CLI、跨项目的可靠宿主体验，而不是追逐某一家官方 UI 的全部功能。这是基于上述产品范围作出的定位判断。

## 历史样本：Vibe Kanban 不能继续列为活跃商业样板

[BloopAI/vibe-kanban](https://github.com/BloopAI/vibe-kanban) 本次 API 返回 28,438 stars、最后推送 2026-09-19、未归档；但 README 已标注 sunsetting。官方在 **2026-04-10** 宣布关闭 bloop，公司远程服务随后退出，本地工作区继续运行，项目交由社区维护。因此“未归档、有星数、有最近推送”不充分证明一个产品仍由原团队持续运营。[API](https://api.github.com/repos/BloopAI/vibe-kanban)、[官方公告][vibe-shutdown]

它的任务看板、独立工作区、Diff 行内反馈仍可作为工作流设计案例；更直接的启发是本地核心能力应可独立工作，用户状态可迁移、可导出，不把恢复会话绑定到必须在线的自营服务。[README](https://github.com/BloopAI/vibe-kanban/blob/main/README.md)

另一个需要去重的名称是 [slopus/happy-cli](https://github.com/slopus/happy-cli)：该仓已归档，README 明确代码和 issue 已迁到 `slopus/happy`，不能按两个活跃产品计数。[迁移说明](https://github.com/slopus/happy-cli/blob/main/README.md)

## 少量用户信号如何转成验收约束

| 公开反馈样本 | 本次读取状态 | 能支持的结论 | 不能支持的结论 |
| --- | --- | --- | --- |
| [cmux #330：Linux support](https://github.com/manaflow-ai/cmux/issues/330) | Open；57 条评论，169 个赞 | 至少有一批用户明确关心非 macOS 平台；跨平台是值得验证的定位方向。 | 无法推出 CC Desk 用户规模、转化率或付费意愿。 |
| [Emdash #2882：大量工作区索引阻塞 UI](https://github.com/generalaction/emdash/issues/2882) | Closed，2026-08-28 关闭；报告涉及 24 个工作区 | 多项目文件扫描/索引应有规模与主线程阻塞验收；新增 Git/预览能力须继续保持按受影响项目刷新和有界读取。 | 不能说当前 Emdash 仍有该缺陷，也不能把报告数据当成我们的性能基线。 |
| [Emdash #2458：SSH 自动端口转发](https://github.com/generalaction/emdash/issues/2458) | Closed，2026-06-17 关闭 | 远程开发不止传终端：预览、动态端口、身份映射和关闭清理都会进入产品范围。 | 不能只凭这条 issue 就优先建设远程执行平台。 |

## 给主规划的五个优先方向

以下是建议的递进范围与验收原则，实施前仍需对照最新主线，定位具体增量。时间成本和性能阈值应在代码分析、原型和真实测量后确定。

| 顺序 | 建议增量 | 对应外部证据 | 最小可审查交付与验收 |
| --- | --- | --- | --- |
| P0 | **版本兼容、诊断与恢复证据** | Codex/OpenCode 公开协议持续演进；cmux/Emdash 区分进程与会话寿命 | 建立按 OS、实际 CLI binary/version、动作划分的能力矩阵；对新建、恢复、停止、重启后继续、升级迁移分别记证据。复用现有安全诊断；未确认能力明确显示，不把宿主测试替代真实 CLI/安装包结果。 |
| P1 | **完善现有关注与恢复体验** | cmux 的通知生命周期和直接跳转；Codex 的 revision 语义 | 聚合现有 owner 事件，明确来源、未读和 Unknown；从关注项跳到正确项目/会话。并发新结果不能被旧“已读”动作吞掉；失败/恢复提示不自动重放输入或偷偷启动第二个 run。用已有压力工作量验证排序、刷新和监听释放。 |
| P2 | **Git 只读审阅，再评估显式 worktree** | Emdash 任务与差异链路；Vibe Kanban 的反馈工作流 | 先提供受后端范围约束的分支、状态、变更文件和有界 diff，支持打开现有编辑器。然后设计用户明确创建的 worktree 与会话绑定；保存脏工作区、保护路径、退出/失败清理和原生恢复需单独验收。worktree 只隔离 Git 工作副本，不宣称安全沙箱；Git 写入和删除不混入第一阶段。 |
| P2 后续 | **有限产物预览与结果交接** | AionUi 预览；Emdash Diff / preview | 首批限 Markdown、图片、文本和 diff，明确文件来源和读取预算，复用 held scope / safe DTO。恶意链接、路径替换、超大文件和失效 scope 都有边界；HTML/脚本不能进入可执行内容 sink。不要先建全功能 IDE。 |
| P3，需需求证据 | **远程只读关注验证** | Happy 移动端、cmux 通知、CloudCLI 远程入口 | 先用原型验证“离桌查看状态/收到提醒”的真实频率。成立后再单独设计显式配对、可撤销授权、最少数据和断线状态；不要将本地 document token 直接复用到远程。远程输入、审批和进程控制属于后续独立协议与威胁模型。 |

性能工作与上述每个增量一起验收：先建立真实 WebView、目标平台和多 owner 的测量，再决定优化；不可用 cmux 的实现语言、竞品宣传或浏览器跑分推导本项目必须换栈。保持本地优先和数据可迁移属于贯穿约束，不必单独扩成云同步项目。

## 证据库存与未验证项

- 已读：本仓根 `AGENTS.md`、`README_CN.md`、`docs/project-contracts.md` 产品定位章节、`docs/native-cli-v3.md` 产品和授权边界；`docs/` 下未发现附加 `AGENTS.md`。
- 已核对：八个活跃参照仓库的 API 元数据；八份固定 commit README 的 blob SHA；OpenCode latest 非预发布版本及其 tag 文档；三条 issue 的状态/日期；Vibe Kanban 官方停运公告；Happy CLI 迁移与维护状态。
- 没有做：竞品安装、功能运行、安全审计、真实平台性能测试、商业模式/用户规模统计。官方描述不等于独立验证结果。
- OpenCode v2 server 网页通过浏览工具访问失败，分别读取官方开发分支和 v1.19.4 tag 的文档；正文协议例子引用稳定 tag，未确认 v2 binary 的对应兼容性。其他主数据通过 GitHub connector 正常取得。
- 本文件仅提供机制选择和证据；本项目当前功能完成度、主线增量、发布证据和最终阶段排期由总规划核对，不由旧文档状态推断。

[emdash-readme]: https://github.com/generalaction/emdash/blob/d76ec99aaf72277d2786e210491318856742996a/README.md
[emdash-tasks]: https://emdash.com/docs/tasks
[emdash-diff]: https://emdash.com/docs/diff-view
[cmux-readme]: https://github.com/manaflow-ai/cmux/blob/2b7f410b7155e7f4234178d13b43fc800fab1e82/README.md
[cmux-notify]: https://cmux.com/docs/notifications
[aion-readme]: https://github.com/iOfficeAI/AionUi/blob/886dcbdeabeee77867daf47ed99548140f05b378/readme.md
[happy-readme]: https://github.com/slopus/happy/blob/99f415fa0d12d233a80a77fd88122bfb19b17477/README.md
[happy-security]: https://happy.engineering/docs/security/
[cloudcli-readme]: https://github.com/siteboon/claudecodeui/blob/94da1c221f15a377e9624753d8009ca35c3c06f7/README.md
[ccswitch-readme]: https://github.com/farion1231/cc-switch/blob/e85ade714ad6208c79d85f915036ed61b38658fc/README.md
[codex-readme]: https://github.com/openai/codex/blob/39a2438d16514d0d6f88105d17b0f747994af487/README.md
[codex-appserver]: https://github.com/openai/codex/blob/b559e6a3988489b636e1e79672eff330bf0679dc/codex-rs/app-server/README.md
[opencode-readme]: https://github.com/anomalyco/opencode/blob/35adacb8f74ac1114175170f52311bd367a417e7/README.md
[opencode-release]: https://github.com/anomalyco/opencode/releases/tag/v1.19.4
[opencode-stable-readme]: https://github.com/anomalyco/opencode/blob/v1.19.4/README.md
[opencode-server]: https://github.com/anomalyco/opencode/blob/v1.19.4/packages/web/src/content/docs/server.mdx
[vibe-shutdown]: https://www.vibekanban.com/blog/shutdown
