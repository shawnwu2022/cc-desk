# 下一版本本地实施与验收边界（2026-10-22）

基线为 v0.24.0/main `24316ad9069df76f2de669fe7b17eeca2a2228be`。
实施分支 `feat/next-version-stability`，独立工作树
`.claude/worktrees/next-version-stability`。用户后续已批准候选推送、draft PR、原 CI 和一次最多约 32 runner-minutes 的校准，
并明确授权验证无问题后合并 main、正式发布。因新增快捷键，本轮准备版本为 0.25.0，
而非规划原称的纯修复 0.24.1。远端验证结果须按精确 SHA/run/attempt 后续记录。

原获批计划位于另一个保留工作树的
`../next-version-planning/docs/superpowers/plans/2026-10-22-v0241-stability.md`。
该路径是本机规划来源，不是当前发布树的可移植链接。原始失败与详细命令日志保留于
本工作树 `.cache/next-version-execution/`，未上传。

| 工作包 | 本轮实际交付 | 状态 / 退出条件 |
| --- | --- | --- |
| Ctrl+B | Workspace Mod+B（Windows/Linux Ctrl+B、macOS Cmd+B），复用既有宽/窄布局各自侧栏状态；不增加配置格式或视觉变化 | 本地实现完成。终端、输入、IME、重复 keydown、模态和既有绑定优先；不抢 CLI 键位。尚无原生 WebView/CLI 焦点验收。 |
| 历史状态与耗时基线 | roadmap、原执行账本、五组精确源码/run/attempt 的实际 job/step 记录 | 本地完成；已交付的活跃筛选、默认值、开关、改名/状态观察未重做。 |
| 测试时序 | OpenCode redirect fixture 的真实延迟控制先 RED，再改为有界轮询 | 本地完成一项确证缺陷；生产 POST/PATCH 超时、未知结果不重试和安全校验不变。BodyTimeout 阶段覆盖候选未证实为产品故障，未据猜测改产品。 |
| npm 15 条初始组件公告 | Vue/i18n 及兼容传递依赖、Vite 6/esbuild、Vitest 4 分批迁移 | 当前 lock 的 npm audit 零匹配；不等于所有生态或将来数据库无漏洞。 |
| Rust 新查公告 | Tauri/TLS/根证书、OpenSSL、anyhow/serde_with/tar 兼容修补；独立 crate locks 同步；生成 ACL 与显式 deny 保持授权面 | 本地检查完成，剩余 glib 等输入逐项见 [依赖核查](dependency-review.md)。完整安全/平台验收尚未关闭。 |
| PR43 / CI | 当前源码绑定的可选精确名称采集、负向控制、离线权重比较入口；原普通分片策略不变 | 候选完成。没有完整可采纳的 hosted 校准或受控收益，未采用旧权重，十分钟目标未达到或声称达到。 |
| 真实目标验收 | 原始 nativeAll 宿主探测、D20 官方命令的安全阻断证据、正常生产 CSP 浏览器回归 | D20、原始 nativeAll、安装版 WebView 仍阻塞，具体如下。 |
| 可选诊断 | 保留现有安全 reason code/observer 边界 | 没有证实必须新加诊断字段，故没有扩张产品；用户已撤回 Codex 角标疑似故障，未当作已确认 bug。 |

本地提交按职责拆分：

| 提交 | 内容 |
| --- | --- |
| `df849c8` | 焦点优先的侧栏快捷键 |
| `879f22e` | v0.24.0 状态与精确运行耗时基线 |
| `09b94e6` | Vue 与兼容公告修补 |
| `452594c` | Vite/esbuild 迁移与准确资源断言 |
| `633794b` | OpenCode redirect 测试观察时序 |
| `e850215` | Vitest 4 迁移与原测试库存保留 |
| `fed5ce6` | Rust 修补、生成权限文件与升级授权边界 |
| `d655bb3` | 可选 CI 校准候选，未采用时序权重 |
| `d4a754d` | 正常生产 CSP 浏览器中的快捷键正向/输入焦点回归 |
| `8beace0` | 校准采集/读取两端拒绝错名或不完整完成记录 |
| `3d34bd7` | 浏览器图标准确资源断言与显式隔离测试偏好 |
| `e1631ad` | Windows autocrlf 检出保持校准源码精确字节 |

## 已取得的验证

这些结果按实际执行源记录，不将不同工具或阶段的测试数加成一个虚假的全量通过率。

2026-10-22 首轮远端候选为 `a3f5dd65` / [PR73](https://github.com/shawnwu2022/cc-desk/pull/73)，
普通 CI run `39805351619`（PR 合并测试源码 `1dda80e1`，attempt 1）。附加校准 group 2
拒绝真实 CrashRecovery 父子进程的嵌套 libtest 输出，尽管该进程 exit 0；原始普通测试成功。
原 ZIP artifact `12438641441` 的 SHA-256 为
`4d564772dbb0b6da64b2f579deeab99249d8b0c51844f1893da6d4431d8a9909`。
新增嵌套输出回归先 RED；补修仍要求目标仅完成一次、最后完成行紧邻末尾完整摘要，
错名、重复目标、截断及尾部子 worker 均拒绝。校准/分片策略 14 项 GREEN，实际原始日志
提取函数回放及独立审查确认；这不把失败回执改成成功，不导入不完整权重，也不重跑第二轮校准。
PR 标题已移除 opt-in，后续必要普通 CI 不再请求测量。最终 CI 与截图结果仍须按新 head 核对。

| 入口 / 实际范围 | 结果与证据边界 |
| --- | --- |
| 快捷键原回归与新增失败 | 原 45 通过；RED 两项预期失败；GREEN 54 通过；跨组件 170 通过。重复键、输入/终端、绑定冲突、宽窄布局状态分别覆盖。 |
| `npm test`，完成 Vitest 4 迁移后 | 198 文件，3128 通过、2 项原有跳过；保留 process/stress/DOM 分组与原压力负载。之后新增的两个 ACL 测试单独验证，未伪称已包含在该次全量。 |
| `npm run test:visual:unit` | 66 通过；没有更新截图基线。 |
| CI 原 Node policy 入口 + 新校准文件 | 最终 278 项：272 通过、6 项原有跳过，包含实际 Git checkout、rustc/libtest 与进程类控制；未删除原文件清单。 |
| `npm run typecheck`、生产构建 | 通过；构建保留大于 500kB chunk 的既有警告，未放宽阈值。 |
| `npm run test:production`，新增快捷键后 | en/zh 两项通过，真实生产 CSP、真实 DOM focus、按钮焦点折叠/展开、搜索输入焦点不抢键。隔离 headless Chrome 自动退出成功。早期 restricted-token 运行需清理其确切 owned preview，原记录保留。 |
| `npm run test:visual`，完整现有浏览器库存 | 实际 256 项中 233 通过、23 失败（2.4 分钟）。10 个图标用例因 Vite 内联 SVG 与文件名假设不符；修正为浏览器实际导入的精确资源后，首轮 9 通过、2 个 OpenCode surface 因测试未显式启用 provider 失败；仅为该隔离用例设置明确偏好后 2 通过。没有改产品默认值或跳过图标断言。其余 13 张截图像素差异仍未通过，原 expected/actual/diff/trace 均保留于 `test-results/visual`，没有更新 PNG 基线或容差；需要相同平台精确 head CI 核验，不能笼统归为无害宿主差异。 |
| pinned SDK 本地元数据回归 | 正常任务权限下 1 通过；仅隔离 synthetic JSONL/owned helper，没有真实会话标题或模型调用。restricted-token 的先前失败另列，未覆盖。 |
| Rust 原生应用定向范围 | 58 通过：document/manager/startup、updater HTTP/policy/redirect/官方来源拒绝、SDK、OpenCode preparation。没有原始 nativeAll 的全量结论。 |
| 五个独立 Rust crate，完整 `--locked` 测试 | scope 233、observer 51（另有 1 项原有 ignored）、input 9、transport 13、updater 21；共 327 通过、1 项原有 ignored。 |
| `cargo clippy --locked --all-targets -- -D warnings` / fmt | 通过；没有添加 allow 来消除 lint。Cargo 的全局缓存清理 `nul` 文件警告非致命，未清理用户全局缓存或修改 ACL。 |
| Tauri 权限回归 / 实际重新生成 | RED 发现上游 default 扩大；显式 deny 后 ACL 与测试发现两文件共 4 通过；`cargo check --locked --all-targets` 通过并同步生成文件。 |
| 校准源库存 | 本次 Rust 1.95.0 Windows 编译 executable 实际 `--list` 共 1696 名称，16 个候选均存在；记录 executable SHA-256。这只是 listing，不是同 hosted 条件测量，不给时序权重。 |
| Windows 精确源码检出 | 临时独立 Git index/checkout，`core.autocrlf=true` 的真实 RED 证明 CRLF 会使 blob 检查失败。仅为校准代码/目标清单声明 LF 后 GREEN；相关 45 项中 39 通过、6 项原有跳过。没有放宽哈希校验或修改全局 Git 配置。 |

独立审查展开实际 ACL 后，main 在基线与升级后均为 62 个可调用命令，manager 均为 0；
另执行前端 58、Node 25、OpenCode 8、document 19、启动 URL parity 1 项全部通过。
这些是独立复核，不与前表重复累计。审查指出校准 reader 只比较汇总计数会接受
错名/摘要后残缺；本地先复现 RED，再在采集/读取两端共用精确完成检查。
四项负控在同步更新 hash 后仍须拒绝错名、尾部 partial、截断和双摘要；相关 44 项
38 通过、6 项原有跳过，审查者另独立执行校准 3 项通过并确认该问题已解决。
该缺口限离线测量，不曾证明普通覆盖或发布门禁可被绕过。

## 尚不能自行关闭的门槛

| 具体操作 / 目标 | 当前确定结果 | 后续最小条件 |
| --- | --- | --- |
| 发布候选分支到现有 GitHub 仓库并执行精确源码普通 CI/专项平台检查 | **已执行，PR73 验证中**。`a86b6b46` 的普通 CI `39808396116` 已 23/23 成功，认证 coverage ZIP 与原始日志验证了 1658 执行通过、35 原忽略、22 明确未验证；后续 `8de7876` 的普通 CI 和六项专项/Windows 包同样通过，但视觉仍有一项失败。不同 SHA 的结果不替代最终候选。 | 当前 head 的全部适用检查通过后才合并 main；正式发布仍须新 main 的精确 CI/coverage 与三平台签名。视觉原基线/零容差保持不变，失败不能用人工像素审查替代。 |
| hosted 校准与优化收益比较 | **唯一获批轮次已执行，但全组证据不完整**；run `39805351619` 的三个 pair 成功，另一 pair 因合法子进程嵌套摘要被 reader 拒绝，尚有 group 未测；实际测量步骤共 1018 秒，未采用任何新权重。已先 RED 再修 reader，但没有重跑校准。 | 现有原四 harness、16 片、25/30 分钟预算、名称/忽略集合和精确 coverage 不变。任何追加校准或受控 control/candidate 收益比较都需要另行批准资源；不宣称十分钟目标已达到。 |
| 原始 `pwsh -NoProfile -File ../scripts/windows-rust-tests.ps1 -Action All`，WSK-MSI | **确定 exit 1，测试正文未执行**：`inJob=True elevated=False`，入口报 `Ordinary PowerShell is contained in an external Job; direct native tests are blocked`。这不是自动审批拒绝，也不是 nativeAll 通过。 | 项目许可的独立 Job-free、相应 token/权限目标；不从受限宿主 breakaway、不改门禁/ACL。先记录真实 Probe 再跑原入口。 |
| D20 官方 `run-real-cli-certification.mjs --config …` | **确定 exit 2 / BLOCKED**：Claude、Codex 均 `AUTHORIZED_TEST_ACCOUNT_UNAVAILABLE`，recordPaths 空；在读取账户/env/binary 前停止，无模型调用。 | 明确授权的独立测试账户、实际四 lane driver/每 CLI 与预算；用户目前禁止未授权登录/付费调用。OpenCode 单独证据矩阵，不伪装成双 CLI D20。 |
| 安装版 Windows WebView2、CLI/IME/粘贴/resize/多会话 | **未运行**；没有授权隔离用户/profile/目标安装安排，不动用户当前人工测试窗口。普通浏览器和 loader 测试不能代替。 | 专用可丢弃 profile/目标与相应安装/交互准入；保留用户正式版和真实会话。完整原始平台库存及当前三平台 required CI 仍需同源通过。 |

已获批准的候选远端操作（**已执行，以下保留原边界**）：将核对过的本地 HEAD 推送到现有
`shawnwu2022/cc-desk` 的 `refs/heads/feat/next-version-stability`（非 force），
创建 base `main` 的 draft PR，标题带 `[calibrate-rust]`。PR 首轮保留现有所有普通
CI/专项 workflow，在前四个原 shard 各自完成原始证明和上传后才进行校准；
8 组总计 16 个精确测试，最多 4 个并行采集，每 pair 8 分钟，额外测量上限约
32 runner-minutes，另有 artifact/协调耗时，仍受原 30 分钟 job 上限约束。
这使用已有 GitHub hosted runner 配额，不创建付费模型请求、凭据或外部资源；
是否产生 GitHub 计费取决于仓库现有配额，不能声称无成本。候选验证阶段不会写入
main、tag 或 updater；用户另已批准验证后合并 main、正式发布。普通 checks 失败
仍阻止准入，正式发布继续使用既有 protected-main promotion，不另建凭据。

D20 最小目标需两套受授权的独立 Claude/Codex 账户及 CLI、真实 Desk/system-terminal
lane driver，每 CLI 四格（off/on × Desk/系统终端），新鲜 testRoot 内包含 binary/driver/
HOME/config/project；凭据只在目标环境以显式变量引用或获准隔离登录提供，不能发到聊天或提交。
具体会话/费用上限须在该验收动作前批准，目前未设预算、未调用。
原始 nativeAll 则需实际 Probe 证明不在外部 Job 的正常宿主，以及原库存要求的
Job-free/unelevated 条件；这不是请求对当前 agent 的 Job 或 ACL 做修改。
WebView 最小目标需获准的独立可丢弃 Windows profile/隔离安装目录和 WebView2，
只验证候选包，不安装覆盖稳定版、不关闭当前人工测试窗口，账户模型测试另计。

没有再次提出已经完成的产品修复；没有为了“全绿”减少检查、放宽安全断言、
关闭未知进程或安装工具。当前结论是“本地候选已实现，发布级门槛有明确阻塞”，
不是“下一版已达到发布标准”。所有用户测试工作树、窗口和独有分支继续保留。
