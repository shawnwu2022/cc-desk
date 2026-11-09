# 0.27 第三阶段：完整本地验证与 P1 收口

开始 HEAD `09a7d4224c357b5fc0d85830b2844b5b1c12ae3f`，分支 `feat/v0.27.0`，目录 `E:\Codex\2026-11-06\task-2\cc-desk`。开工只有原 `.audit/v027.todo.md` 未跟踪，未改写它。未同步 main、推送、发布、安装新版本或操作真实用户 CLI 数据。

## 首轮 Rust 验证与保留的聚合失败

使用仓库现行四 harness / 16 分片 runner，没有改测试选择、ignore、断言或超时。实际 Probe 为 `inJob=True elevated=False`；保留原政策 18 项 Job-free 的排除披露，不假装 job-free。原 4 项 unelevated 不排除。

```text
# cwd src-tauri；CARGO_NET_OFFLINE=true 仅当前子进程
pwsh -NoProfile -File ../scripts/windows-rust-tests.ps1 -Action Probe
pwsh -NoProfile -File ../scripts/windows-rust-tests.ps1 -Action Compile

# cwd repo；本地适配脚本，不修改 scripts/windows-rust-* 生产 runner
node .audit/v027-stage3-rust-driver.mjs
```

编译脚本实际执行 `cargo test --locked --no-run --message-format=json`；离线复用既有 ConPTY/Cargo 资源，编译 3m19s。随后原样无过滤 doctest `cargo test --locked --offline --doc` 和原 runner `createPlan / verifyBundle / runShard / aggregateResults`。每 harness 上限仍 20 分钟、每片 25 分钟。16 片在本机串行运行，原始结果和每片 receipt 保留。

runner 强制 numeric run 绑定；本地适配仅为满足其数据格式使用 **本地合成命名空间 `2026110601/1`**，并显式保留 `localOnly/notHostedCi`。它不是 GitHub run，不能用于发布 admission、CI coverage 晋升或冒充 hosted CI。没有设置/写入真实 GITHUB_OUTPUT 或上传 evidence。

完整库存：`cc_desk` lib 1779（选 1761、原 ignore 33、Job-free 排除 18）；`cc-desk` bin 6；`paste_transport` 8；`paste_claude_e2e` 5（原 ignore 3）。编译 plan hash 为 `d8002b708fa4a2c14a5ead99d19f949e6ae3cd6d8e44e71a416de99599134016`。

首次本地适配漏传强制 `artifactName`，全部 16 片均在测试启动前被 runner 拒绝；没有片结果或测试执行。原日志 `v027-stage3-rust-run.log` 保留。补入已校验 plan 的实际名称后复用同一编译包/库存/doctest，没有重编或重跑已完成片；正确执行日志 `v027-stage3-rust-run-corrected.log`。本地原始包/片/coverage 留在 `.audit/v027-stage3-rust-*`，不纳入源码提交。

首轮 16/16 片均执行成功：lib 1728 通过 / 33 原 ignore；bin 6、paste_transport 8、paste_claude_e2e 2 通过 / 3 原 ignore。合计 **1744 通过、36 原忽略、18 Job-free 排除、0 测试失败/超时**。但聚合命令退出 1：`Rust shards: unexpected pinned compiler`。实际默认 stable 是 **rustc 1.95.0**、debug profile 未覆盖，而 CI 固定 **1.98.1**、dev/test debug=0。前置核查遗漏工具链及 profile，不能称首轮通过现行聚合门禁，不能改元数据把它晋升为合格证据。

随后同一默认工具链的 `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings` 退出 101：新增 `native_cli_version_parser.rs` 用 `#[path]` 再次加载生产 parser，触发 `clippy::duplicate_mod`。这是本次新增测试的问题，改为 `use crate::cli::diagnostic_version_parser as parser`，没有豁免 lint 或改变断言。原日志 `v027-stage3-clippy-all-targets.log` 保留。此前 `--lib` Clippy 未检查测试目标，不能拿它替代 all-target 通过。

已只读确认 **1.98.1、clippy、rustfmt 均已安装**，不安装环境。后续统一使用现行 CI 的 `RUSTUP_TOOLCHAIN=1.98.1-x86_64-pc-windows-msvc`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`。固定版本/profile 改变以及真实测试引用修复，是重新构建和验证完整原生套件的新依据；不重跑旧 stable 套件、不放宽聚合校验。原编译包/片全部保留，固定工具链使用另一个本地命名空间和目录，不能混合两轮。

## 实际只读 CLI 与平台

```text
node .audit/v027-stage3-cli-versions.mjs
```

固定 `--version`，三个原生程序直启，不执行 PowerShell/npm shim 字符串。每种 CLI 使用任务内独立新 HOME、USERPROFILE、APPDATA、LOCALAPPDATA、XDG 与 CLI 配置根，完整显式环境，无继承 API/token；关闭自动更新，stdout/stderr 合计 8KiB、5 秒限时。没有创建/恢复/归档/删除会话，没有提供或扫描认证数据。

结果 Claude 2.1.318、Codex 0.166.0、OpenCode 2.2.5，均 known，所选可执行文件 SHA256 前后一致。脚本和白名单报告 `v027-stage3-cli-versions.mjs/.json`，原始输出不落盘。此证据 **未调用生产诊断后端 IPC**，不可声称 profile→WebView→真实 CLI 端到端通过。

`wsl.exe --list --quiet` 返回 `Wsl/EnumerateDistros/Service/E_ACCESS_DENIED`，无法确认可用隔离 Unix；未重试提权/安装或新建环境。不是自动审批拒绝。D20 真实认证还缺授权测试账户、隔离二进制和两条 lane driver，参照 `docs/superpowers/execution/D20.md`，未读取真实用户凭据替代。

## P1 实现与独立审查

新增精确候选忽略（version/releaseId/sourceSha 单项，未来版本不屏蔽）、解除忽略撤销旧准入、20 条本地白名单更新事件、明确失败重新检查按钮。历史不存 URL、admission、会话内容或原始错误；安装/重启阶段只记交接，不记已安装。损坏或不可用存储不阻塞正常更新，并有提示。

独立审查配置 `gpt-6.1-sol / high`，全分支及本轮 P1 只读审查，无写入/测试/CLI 执行。未发现新 P0/P1；发现 P2 内部损坏清洗后不提示，根增加行为断言 RED 并修复。随后补充“自动检查成功不得抹去读入丢失提示”的 RED→GREEN。本窗口保留初始读取异常，下次新 store 完整正常读取后恢复。

保留日志：

- `v027-stage3-preferences-red.log`：四项缺失行为失败；随后 store 与旧检查域 13 通过。
- `v027-stage3-preferences-ui-red.log`：缺少忽略控件，1 fail / 8 过滤；随后受影响 4 文件 49 通过。
- `v027-stage3-preferences-final-targeted.log`：补存储失败与真实重新检查调用验证，4 文件 50 通过。
- `v027-stage3-corrupt-red.log`：P2 未提示真实断言失败；`corrupt-green` 14 通过。
- `v027-stage3-corrupt-sticky-red.log`：读入异常被后续成功写入抹去，真实断言失败；`corrupt-sticky-green` 为后续验证。不会用最终通过覆盖这些失败。
- 常规 Vitest 配置排除了 `tests/visual/fixture.test.ts`，一次同时传入该文件的命令实际只跑 2 文件 14 项，不能记作 visual fixture 通过；单独配置与浏览器验证以最终记录为准。

## P1 最终前端验证

`npm run test:ci` 首次为 **3205 pass / 1 fail / 2 原跳过**（207 文件）。唯一失败 `Update_BackgroundDelayAndInterval_001` 在原卸载零 timer 断言发现 1 个 timer。已查本地 jsdom **25.0.1** 的 `Storage-impl.js:74`，其 setItem 为宿主 storage 事件创建 JS setTimeout；不是应用后台 interval 未清理。只在这个计时 case 的 Storage getItem/setItem 边界使用 Map 值语义，原 29,999/30,000ms、六小时、安装零调用、卸载 timer=0 均保留，并新增断言记录实际写入；其他存储测试仍使用真实 jsdom Storage。独立审查明确确认没有吞掉应用 timer/取消行为。原失败 `v027-stage3-frontend-full.log` 保留，定向 12 通过；随后相同完整命令为 **207 文件、3206 pass / 2 原跳过**，见 `v027-stage3-frontend-corrected.log`。

实际查看四张 P1 中英文截图又发现“忽略已核验候选”误报来源未核验，新增断言真实 RED `v027-stage3-ignore-hint-red.log`，改为准确的忽略原因；负面来源回执仍显示未核验。末次受影响 **5 文件 57/57** 通过，见 `v027-stage3-ignore-hint-green.log`。完整前端通过早于这个局部显示修正，不将旧完整记录改写为新记录。

```text
npm run test:ci
207 files; 3206 passed; 2 original skipped

npx vitest run tests/stores/updatePreferences.test.ts tests/stores/update.test.ts tests/components/updateChannels.test.ts tests/components/remainingSettings.test.ts tests/components/backgroundUpdates.test.ts
5 files; 57 passed

npx vitest run --config vitest.visual.config.ts tests/visual/fixture.test.ts
1 file; 51 passed

npm run typecheck
npm run build
npm run test:production
退出 0；最终真实生产入口/CSP 中英2/2；既有chunk体积提示保留

npx playwright test tests/visual/unified-workspace.spec.ts -g 'update ignore and local history stay reachable|diagnostics and conservative cleanup stay reachable'
4/4；最后显示修正后单独P1中英文2/2，四张最终P1截图已实际查看
```

Node 政策沿用 `.audit/v027-p0-validation.md` 记载的完整 23 文件命令，本轮再次 **310 pass / 6 原跳过 / 0 fail**，见 `v027-stage3-node-policy.log`。最终类型/构建/CSP/视觉日志为 `v027-stage3-{typecheck,build,production,visual}-delivery.log`。未改任何像素基线、阈值或原截图断言；没有当前源码的完整 Linux 受准像素 CI 证据，本机 Windows 定向截图不能替代。

固定 1.98.1 / CI dev/test debug=0 的 `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings` 和 `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` 均退出 0，见 `v027-stage3-pinned-{compiler,clippy,fmt}.log`。完整 pinned 四 harness/16片仍待下节收口，不以此替代。

## 固定工具链完整验证：已完成

本轮全部测试绑定实现提交 **`68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3`**，测试期间没有修改源码或提交。实际编译器为 `rustc 1.98.1 (48a229cea 2026-09-01)`、host `x86_64-pc-windows-msvc`，CI dev/test debug 均为 0，只使用已安装工具链和缓存依赖。实际命令如下，环境仅在执行子进程设置：

```powershell
# cwd repo
$env:RUSTUP_TOOLCHAIN = '1.98.1-x86_64-pc-windows-msvc'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_NET_OFFLINE = 'true'
rustc -Vv
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
node scripts/prepare-conpty.mjs
# cwd src-tauri
pwsh -NoProfile -File ../scripts/windows-rust-tests.ps1 -Action Probe
pwsh -NoProfile -File ../scripts/windows-rust-tests.ps1 -Action Compile
# Compile 实际执行 cargo test --locked --no-run --message-format=json
# cwd repo
node .audit/v027-stage3-pinned-rust-driver.mjs 68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3
```

编译 2m29s；无过滤 doctest 0 项、退出 0。执行前已生成并读取 [preflight JSON](v027-stage3-pinned-rust-preflight.json)，核对 source、compiler/profile、四 harness bytes/SHA256、Cargo JSON SHA256、原 runner 文件摘要、plan hash 与参数。另见 [执行前核对说明](v027-stage3-pinned-preflight-check.md)。一次额外校验的日志重定向被默认文件沙箱拒绝，未产生文件；未重试写入，改为无重定向只读确认成功。不是自动审批拒绝，不影响已有 preflight 或原 runner 校验。

本地合成命名空间 **`2026110602/1`** 与旧轮分离，始终标明 `localOnly/notHostedCi`，不能用于发布 admission 或晋升成 hosted CI。plan SHA256 为 `9bf9c2e8b8ad5e945f9a7de91d5d84abc0e943c4fe4f528068e2a9380ac0349f`。harness 上限 1200000ms、分片上限 1500000ms、原 ignore、18 项宿主排除和断言均未改变；每片前原 runner 重新核对 source 及编译包摘要。

**16/16 片和最终聚合均退出 0，无失败或超时。**

| harness | 通过 | 原 ignore | 宿主排除 |
| --- | ---: | ---: | ---: |
| `cc_desk` lib | 1728 | 33 | 18 |
| `cc-desk` bin | 6 | 0 | 0 |
| `paste_transport` | 8 | 0 | 0 |
| `paste_claude_e2e` | 2 | 3 | 0 |
| 合计 | **1744** | **36** | **18** |

host 查询成功，`inJob=true/elevated=false`。七项 `requiredSelectedTests` 在原执行日志全部为 `ok`，包括 Wry/native document/channel/launch/projection 和历史 WebView 退出；四项 unelevated 专项已执行。18 项 Job-free 因 external Job 未验证，按既有发布契约非阻断披露，没有计为通过。报告仍准确保留 `nativeAll.status=unverified`、`nativeAcceptanceProven=false`，不等于真实安装或现场验收。

原始记录保留在 `v027-stage3-pinned-rust-run.log`、`v027-stage3-pinned-{source,host-probe,compile}.log`、`v027-stage3-pinned-rust-bundle/`、`v027-stage3-pinned-rust-shards/` 和 `v027-stage3-pinned-rust-coverage/`。聚合 `windows-native-coverage.json` 为 817906 bytes，SHA256 **`a481357b6ce39bf6b7b5052ee8e1471dbd986ce24b474ff020943814339cf01c`**。大二进制、库存和分片保留在任务目录，不纳入源码提交。

原生结束后串行再执行 `npm run test:ci`，覆盖晚于前次完整前端检查的忽略提示修正；实现提交仍为 `68fa6b3…`。最终 **207 文件通过、3206 项通过、2 原跳过、0 失败、退出 0**，63.55s，原日志 `v027-stage3-final-frontend.log`。没有重复源码未变且已通过的 Node 23 文件、构建、生产 CSP 或 P1 界面检查。npm 既有未知 `auth/python` 配置警告及构建 chunk 体积提示保留，未修改用户配置。

## 实现提交索引与交付边界

| 精确提交 | 内容 |
| --- | --- |
| `65b6e7966635c615e02197bf5aaaff5466d39820` | 受信通道和可取消、有界更新后端 |
| `6cf96b92fc83e9a0682c150465fe22ea1b3ea6fd` | 正式签名 beta/RC/preview 准入，保持隔离标签边界 |
| `da8aaa6e0d2b8d4ea7ed198110e39bf0bb6f2dfd` | 更新 UI/竞态保护、诊断摘要和手动恢复指南 |
| `50b353886fd21cbe6fb5c7f5acc1e82f225c35ff` | 所选 profile 的安全版本采集后端 |
| `5d230917a938b50a0aaa489ea2e653b6006b7dea` | 未回收进程持有 profile 独占权 |
| `09a7d4224c357b5fc0d85830b2844b5b1c12ae3f` | 诊断 UI、取消/迟到保护、清理拒绝及固定源码证据 |
| `68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3` | P1 忽略/记录/重试、最终 UI、计时 fixture 和 parser 测试修正 |

之后仅提交验收文档和小型元数据，不改实现；已测 Rust subtree 为 `648bedfca595becfaf65af3c22f1f3e5dbdef01d`。最终交付 HEAD 见提交记录；不能把 source=68fa 的本地证据改写成新 HEAD 的 hosted CI。

独立审查实际配置为 `gpt-6.1-sol/high`；全分支加 P1 审查未发现新 P0/P1，已修 P2 损坏存储提示，并独立复核 Storage seam 不吞应用 timer/取消。实际使用已有 superpowers TDD、系统调试、请求审查和完成前验证，以及 pstack 任务/证据组织和 domain/type 边界指导；没有安装新技能或重复完整方案流程。

完整 P0/P1、固定源码证据与阻塞见 [验收清单](../docs/acceptance-checklist-0.27.md)。已有足够本地证据准备候选审阅材料和隔离矩阵；整版 P0 和发布验收尚未闭环。原生清理完整预览/执行仍关闭且未交付。当前无运行中长命令；没有推送、发布、main 合并、用户安装或真实会话 mutation。
