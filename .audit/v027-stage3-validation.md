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

## 尚待最终更新

完整 **pinned** 本地 Rust 分片/聚合结果待实际完成写入。将以另一个本地命名空间 `2026110602/1`、固定工具链/profile 和精确实现提交重新编译并事先保存四 harness 文件摘要、plan摘要与 runner参数；不修改原 runner、不混合前一轮证据。

完整 P0/P1 状态、固定源码证据、真实安装/签名往返等阻塞见 `docs/acceptance-checklist-0.27.md`。原生清理仍关闭且完整预览/执行未交付；不会把能力说明当作原功能验收。
