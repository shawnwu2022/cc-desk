# 0.27 候选审阅材料

状态：开发审阅材料已准备，尚未生成或发布 0.27 候选安装包。本文基于独立分支 `feat/v0.27.0` 的文档 HEAD `3f60853a70ccf279c500c99d3cebb424e282f06a`，实现和完整验证绑定 `68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3`；后续仅补文档，不改已测实现。基线仍为 RC3 `4d4674480849f1d4d8fb4edab095b388e769e189`，未整合并行 0.26 main。版本元数据保持 `0.26.0-rc.3`，本文不是 Release notes 的已发布声明。

## 准确变更说明

- 增加默认 stable、用户明确保存的 test/preview 更新通道。stable 只接正式版本，test 接正式/beta/RC，preview 额外接明确 preview；官方已发布目录和精确 tag 准入保持源、发布、资产、摘要和签名绑定。candidate/test-only、不可发布或禁止 updater 发布属性仍只证明排除。切回 stable 不自动降级，旧客户端继续使用稳定 latest。
- 更新下载有界、分阶段反馈，准确取消与安装原子边界防止取消后安装和过期回执。通道/设置不确定时撤销候选并阻断旧准入，活动/未知会话不静默关闭。安装/重启交接不宣称安装成功。
- 增加用户显式触发的所选 profile CLI 版本采集、匿名来源和脱敏复制。固定 `--version`、5 秒/8 KiB 上限，不执行任意脚本或用户默认参数；取消、迟到、revision 改变和未回收进程保持原 owner。unknown 给固定原因，版本不作为功能白名单。
- 增加一个精确候选的持久忽略、最多 20 条白名单本地更新记录、清空记录及失败后重新检查。未来版本不屏蔽，解除忽略不能恢复旧安装准入，历史不保存原始异常、URL、admission、认证或会话内容。
- 复用已有官方历史版本目录、签名准备与两类历史安装能力；本轮新增恢复指南，没有新造回滚框架或一键恢复工具。原生清理仅交付身份、状态和明确拒绝说明，完整级联预览与执行没有交付。

## P0/P1 对照与证据

| 项目 | 实际交付与代码/测试证据 | 尚缺与解阻条件 |
| --- | --- | --- |
| P0 三通道 | `desktop_updater.rs`、`updater_policy.rs`、`src/stores/update.ts`、`UpdateSection.vue`；`desktop_updater_policy` 和 `updateChannels.test.ts` 覆盖准入、切换和失效；发布 parser/签名政策 Node 测试 | 真实签名 beta/RC/preview 分发和旧 stable latest 往返仍未做。须后续授权的 protected-main 候选与实际发布后验证，不能用 fixture 晋升。 |
| P0 可靠更新 | `desktop_updater.rs`、`updater_http.rs` 与更新 UI；坏包/签名/取消/网络、准确 admission、并发检查/取消/安装及设置未知回归均在已执行套件 | 真实安装器失败、断网/取消、重启和旧安装保留尚待可销毁隔离安装环境及真实候选。只收到启动回执不等于成功。 |
| P0 历史选择与准备 | 已有 `HistoricalVersionsPanel.vue` → `versionHistory.ts` store/API → `version_history/catalog.rs`、`download.rs`、`verified_package.rs`。可分页/过滤/选择官方历史 stable，Windows x64 精确 NSIS/签名身份后下载验签；`historicalVersions.test.ts`、Rust catalog/download/selection 测试 | 目录可选择不等于执行可用；准备只验发布者，不建返回点。macOS/Linux 执行关闭，未知/变更资产拒绝。执行仍须下一两行独立准入。 |
| P0 普通历史安装及手动恢复 | 已有 `inspect_historical_install/begin_historical_install`、`ordinary_install.rs` 和 manager：签名包不受九包 REVIEWED 表限制，但要求比当前版本旧的正式版本、Windows x64、合法源安装/Job-free/未提权/会话/所有权等门禁；完整源安装/注册/快捷方式/Desk/WebView 备份后 FreshSettings，仅交接安装器。`historicalInstall.test.ts`、`HistoryOrdinary_SignedIdentityWithoutVersionTable_001` 等 | 没有自动 Return/历史应用启动或一键人工恢复工具；备份不是可随意覆盖的目录。真实安装/人工恢复尚待合格独立宿主与隔离输入。当前 RC3 不会把较新的 0.26 正式版当历史；未来 0.27 版本需按实际版本重新准入。 |
| P0 已审核严格切换/返回 | 已有 `begin_switch`、`payload_policy.rs` 和 Windows context/manager/return checkpoint。九个精确包：0.14.0、0.15.0、0.16.0、0.17.0、0.17.1、0.17.2、0.17.5、0.17.6、0.17.7；绑定 digest/size/安装输出全清单，保存匹配上下文并在进程/job 真实终态后按确认返回。`historicalSwitch.test.ts`、payload/context/return/reopen/fault 测试 | 九包测量不是实际往返通过；上一 stable 0.26 不在九包严格返回范围内。0.27→0.26 可用性不得由此推出；需走普通安装/人工恢复的真实隔离验收，或另经授权审核扩展严格 payload 策略。保留当前共享数据模式关闭；外部 CLI 历史/认证/项目不回滚。 |
| P0 本地诊断 | `diagnostic_versions.rs`、`cliVersionDiagnostics.ts`、`useCliVersionDiagnostics.ts`、关于页；Rust 21 项版本域、API/composable/UI/owner 回归和安全复制；另实际隔离执行 Claude 2.1.318、Codex 0.166.0、OpenCode 2.2.5 的只读版本探测 | 真实 profile→生产诊断 IPC→CLI 端到端、Unix、真实 OS 终止未知注入尚未验收。只读脚本不冒充生产链路；须已证明隔离的对应平台输入，不读取真实凭据。 |
| P0 原生归档清理 | `NativeCleanupAvailability.vue` 显示观察身份、活动/恢复/未知提示，archive/delete 禁用且无 mutation；三项组件拒绝回归。只读关系能力另见 [窄范围复核](native-cleanup-readonly-preview-0.27.md) | 完整级联预览、范围预约、执行及部分失败/超时协调未交付。读取的完整性/同源契约与执行的原子条件缺口分别列明，不能因发现公开 list 就连 delete。 |
| P0 会话回归 | 全部现行前端及 Rust normal required scope；七项 required native/Wry 用例和四项 unelevated 专项执行通过 | 新建/首事件/刷新/关闭/恢复/重命名/身份关联的真实三 CLI D20、安装版 WebView、双行现场和 AI 命名仍未验收。须授权测试输入和对应现场；合成 fixture 不替代。 |
| P1 精确候选忽略 | `updatePreferences.ts`、update store/UI；精确 version/releaseId/sourceSha、未来版本继续提示、旧准入不复活、损坏/不可写存储回归 | 代码和本地测试完成；真实候选端到端跟随通道隔离验收。 |
| P1 一键脱敏摘要 | 关于页和 `updateDiagnostics.ts`；仅白名单应用/通道/阶段/CLI 版本和匿名来源；API/composable/UI/复制回归 | 已完成；无上传、会话正文或原始日志采集，也未扩为完整日志导出器。 |
| P1 更新记录与失败重试 | `updatePreferences.ts`、update store/UI；20 条上限、敏感字段丢弃、显式清空，重试只做 fresh check；`updatePreferences.test.ts`、`UpdatePreferences_IgnoreHistoryAndRetryRequireFreshCheck_009` | 已完成本地行为；安装未知不能盲重试，安装最终成功仍依赖真实安装验收。 |

本轮恢复指南位于 `UpdateSection.vue` 的 `data-update-recovery-dialog`，对应 `UpdateRecovery_ManualGuideHasNoInstallationEffects_005` 明确验证无安装副作用。它不创建快照、不触发恢复、不扩大上述两类执行能力；自动 updater 也不因此承诺完整上下文快照。已有恢复行为详见 [历史能力说明](historical-version-preparation.md)。

## 实现与文档提交索引

| SHA | 内容 |
| --- | --- |
| `65b6e7966635c615e02197bf5aaaff5466d39820` | 受信通道和可取消、有界更新后端 |
| `6cf96b92fc83e9a0682c150465fe22ea1b3ea6fd` | beta/RC/preview 发布版本准入，保留隔离和签名边界 |
| `da8aaa6e0d2b8d4ea7ed198110e39bf0bb6f2dfd` | 更新 UI/竞态保护、诊断摘要、手动恢复指南 |
| `50b353886fd21cbe6fb5c7f5acc1e82f225c35ff` | 所选 profile 安全版本采集后端 |
| `5d230917a938b50a0aaa489ea2e653b6006b7dea` | 未回收进程保留 profile 独占 |
| `09a7d4224c357b5fc0d85830b2844b5b1c12ae3f` | 诊断 UI/owner、原生清理拒绝和固定源码证据 |
| `68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3` | P1 忽略/记录/重试、UI 及 fixture/parser 修正 |
| `3f60853a70ccf279c500c99d3cebb424e282f06a` | 固定工具链最终验收文档和小型元数据；实现未变 |

## 固定工具链验收与保留失败

Rust 1.98.1、CI dev/test debug=0，source `68fa6b3…` 的四个原 harness、16/16 片和原聚合全部退出 0：lib 1728/bin 6/paste_transport 8/paste_claude_e2e 2，共 **1744 pass、36 原 ignore、0 fail/timeout**；无过滤 doctest 0 项。实际 host `inJob=true/elevated=false`，18 项 Job-free 以 `external_job` 未验证，四项 unelevated 执行。18 项依既有契约非阻断披露，不扩排除、不算通过；原始 `nativeAll=unverified`、`nativeAcceptanceProven=false` 保持原义。

执行前四 harness SHA256、compiler/profile/source、plan hash、原 runner 参数均有 [preflight](../.audit/v027-stage3-pinned-rust-preflight.json)。coverage 原报告 SHA256 `a481357b6ce39bf6b7b5052ee8e1471dbd986ce24b474ff020943814339cf01c`。绑定为明确的本地合成命名空间 `2026110602/1`，没有 hosted CI 或发布资格。

最终完整前端 **207 文件、3206 pass、2 原 skip**；Node 23 文件 **310 pass、6 原 skip**；固定工具链 all-target Clippy/fmt、类型检查、构建、生产 CSP 中英 2/2 和最终 P1 界面中英 2/2 通过。像素基线未改，Windows 定向截图不等于完整 Linux 受准像素 CI。全部实际命令和原日志位置见 [最终验证](../.audit/v027-stage3-validation.md)，本次仅文档一致性检查，不重跑这些套件。

原始失败继续保留：首轮误用 stable 1.95 的 16 片原始通过被聚合拒绝；all-target parser duplicate_mod 是本次新增测试问题且已修；jsdom Storage 宿主 timer 的单 case seam 经独立审查且保留卸载零 timer/应用取消与真实持久化覆盖；忽略原因和损坏存储 RED 均保留。更早 `ProxyProbe_BodyTimeout_003` 为 30/31，十秒未连接，读取路径与 RC3 一致，根因仍未定、不能认定已解决或确认既有 flaky；六项原 5000ms process timeout 与后来通过分别记录。见 [第一阶段](../.audit/v027-validation.md)、[第二阶段](../.audit/v027-p0-validation.md)、[第三阶段](../.audit/v027-stage3-validation.md)。

## 可接受范围与不得发布的条件

可接受为继续审阅的开发候选：以上已交付行为和限制明确，危险清理仍关闭，历史普通安装与严格返回分开陈述，所有原失败/skip/宿主限制可追溯；不宣称整版 P0、真实安装、D20 或 native All 已验收。候选产物准备前还须按实际新基线重新集成和核对，本文不自动授权合并。

当前不得发布：没有此次发布授权，代码尚未整合受保护 main，缺该精确 merged-main 的现行普通 CI/coverage、三平台实际签名候选与发布后验证，也缺真实 0.27 stable→RC→stable 隔离往返和目标平台/现场验收。完整原生清理仍是未交付 P0，不能用风险文案替代验收。后续是否明确缩减候选范围须由用户另行决定；本文不代为改变原规划。

最小外部输入是已授权发布流程产生的真实签名候选及可销毁的合格安装宿主、已授权 D20 测试输入，以及原生关系/活动/条件 mutation 的已证明契约。Unix `wsl.exe --list --quiet` 的 `E_ACCESS_DENIED` 不绕过。不申请测试账号或签名秘密，不变版本号、不建 tag、不推送、不发 PR/Release，不改用户安装或真实 CLI 数据。

后续有限切片的增量材料：[OpenCode 现有服务同源证明](opencode-service-source-proof-0.27.md) 明确缺失的 OS 进程/实际 DB/held source 信任边，未新增伪能力；[0.26 main 整合风险](main-0.26-integration-risk-0.27.md) 已获准仅刷新本 clone 的远端跟踪信息并比较真实提交树，尚未整合。两者都是来源与风险分析，不能晋升此前本地验证或替代后续整合 CI。
