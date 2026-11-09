# 0.27 本地整合正式 main 与针对性验证

本轮明确获准把 main 合入开发分支，取代此前“仅分析”的局部限制；没有把开发分支合入 main。目录 `E:\Codex\2026-11-06\task-2\cc-desk`，分支 `feat/v0.27.0`。开工 HEAD `aaad3d2bf1c05c40c30ab03d9d46c38d05f7b4cc`，受跟踪工作区干净；原 todo 与九项未跟踪证据保留。

## 固定引用与实际合并

```text
git fetch --no-tags origin main
目标仍为 0aaaf2a171ced55ff2b4d7a3e1731027c45809df
git diff --stat 4d4674480849f1d4d8fb4edab095b388e769e189 origin/main
仍为原十文件，57 additions / 13 deletions，没有新增实质代码

git update-ref refs/codex/v0.27.0/pre-main-aaad3d2b aaad3d2bf1c05c40c30ab03d9d46c38d05f7b4cc 0000000000000000000000000000000000000000
git merge --no-ff --no-commit 0aaaf2a171ced55ff2b4d7a3e1731027c45809df
Automatic merge went well; stopped before committing as requested
```

检查无 unmerged entry、无未暂存改动，staged diff 仅该十文件。六项应用版本一致为 0.26.0，依赖没有漂移，开发侧发布/通道安全代码保持原样。一次版本核对命令因内联 Node 正则在 PowerShell 中的引号解析失败，整块命令未执行；改用单引号 here-string 后成功，不计为应用或测试失败。

实际 merge commit **`89e30ff37ca51b29239c17abbc52f0dc85106390`**，parents：

1. `aaad3d2bf1c05c40c30ab03d9d46c38d05f7b4cc`
2. `0aaaf2a171ced55ff2b4d7a3e1731027c45809df`

**实际无冲突**，不是沿用静态“预期无冲突”。恢复 ref 仍精确指向合并前 HEAD。没有改写历史、本地 main、其他工作树、远端 branch、tag 或 Release。

## 新回归与验证源

新增 `UpdateChannel_StableMainVersionTransition_009`，只改 `src-tauri/src/tests/desktop_updater_policy.rs`：RC3→0.26 stable 可选，同版本拒绝，0.27 RC→0.26 不降级；stable 不收未来 RC，test/preview 可收。两个 release 使用不同 ID，复用生产目录选择逻辑，没有修改生产代码或伪造 bug RED。

先暂存测试并固定实际验证树 **`acc7ec63f914b23f4388586e8d6f254d02ebbfd9`**；原始执行前记录为 `v027-stage6-source.json`，mergeSha=89e30ff3，scope=targeted-not-full。测试文件 SHA256 `019181b03ac85a95c2f063f1710c4eeeb349939221fd888b514acaef5bff93dd`。验证期间代码与暂存树未改变。

全部验证通过后保存测试提交 **`68436f31dc8a6313e9d87ca9ab35717671edfcfd`**，其完整 Git tree 实际仍为上述 `acc7…`；已测 Rust subtree 为 **`f8720157b56bfa57f47a426aac494614cf707f94`**。后续只补文档，不将旧源 68fa6b3 的完整16片结果迁移给新源。

## 实际命令与结果

原生环境在执行子进程内固定：

```powershell
$env:RUSTUP_TOOLCHAIN='1.98.1-x86_64-pc-windows-msvc'
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_NET_OFFLINE='true'
rustc -Vv
node scripts/prepare-conpty.mjs
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib desktop_updater -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib version_history_catalog -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib version_history::ordinary_install -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib version_history::payload_policy -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib tests::version_history_scope:: -- --test-threads=1
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

rustc 实际为 1.98.1 (48a229cea 2026-09-01)。五个 Rust 域分别 **34、17、2、15、23** 项通过，共 **91 pass、0 fail、0 ignore**；各自过滤 1746/1763/1778/1765/1757 项，不能称全库通过。包括新版本转换、通道矩阵/隔离/签名/取消/HTTP、实际编译版本的历史身份、普通签名准入和九包严格策略。all-target Clippy 和 fmt 均退出 0。日志 `v027-stage6-{compiler,conpty}.log`、`v027-stage6-rust-{updater,history-catalog,history-ordinary,history-payload,history-scope}.log`、`v027-stage6-{clippy,fmt}.log`。

原生结束后串行执行以下前端/政策检查，避免重负载争用：

```text
node --test tests/scripts/releaseRecovery.node.cjs tests/scripts/nativeReleasePolicy.node.mjs tests/scripts/releaseCoverage.node.mjs tests/scripts/releaseDraftRecovery.node.mjs tests/scripts/windowsNativeValidation.node.mjs tests/scripts/historySignatureFixtures.node.mjs tests/scripts/historyRoundtripBuild.node.mjs tests/scripts/historyRoundtripDriver.node.mjs tests/scripts/historyPayloadPolicy.node.mjs tests/scripts/pasteAcceptance.node.cjs tests/scripts/pasteReleaseGate.node.cjs tests/scripts/pasteTrace.node.cjs tests/scripts/nativeLaunchDiagnostics.node.mjs tests/scripts/releaseBuildAdmission.node.mjs tests/scripts/windowsRustShards.node.mjs tests/scripts/windowsRustShardRunner.node.mjs tests/scripts/windowsRustCalibration.node.mjs tests/scripts/ownedJobDrain.node.mjs tests/scripts/ciRustWorkflow.node.mjs tests/scripts/ciBuildMetrics.node.mjs tests/scripts/sessionRenameSafety.node.mjs tests/scripts/opencodeObserver.node.mjs tests/scripts/terminalDropPaths.node.mjs
310 pass / 6 original skip / 0 fail，316 total

npx --no-install vitest run tests/config/packageVersion.test.ts tests/scripts/generateUpdaterManifest.test.ts tests/scripts/releaseManifestUrls.test.ts tests/utils/updatePolicy.test.ts tests/utils/updateDiagnostics.test.ts tests/stores/update.test.ts tests/stores/updatePreferences.test.ts tests/components/updateChannels.test.ts tests/components/backgroundUpdates.test.ts tests/components/remainingSettings.test.ts tests/components/historicalVersions.test.ts tests/components/historicalSwitch.test.ts tests/components/historicalInstall.test.ts tests/config/versionHistoryContracts.test.ts
14 files / 169 pass / 0 fail

npm run typecheck
npm run build
npm run test:production
全部退出0；生产中英CSP 2/2
```

日志为 `v027-stage6-{node-policy,frontend-targeted,typecheck,build,production}.log`。既有 npm auth/python 配置警告、构建 chunk 提示和 FORCE_COLOR/NO_COLOR 提示保留，未改用户配置消除它们。生产 CSP 使用本次正常构建的入口、合成 IPC，不是安装版 WebView/真实 CLI。

## 审查、版本差异与未跑项

原独立审查者 `cleanup_safety_design_review`（实际既有 gpt-6.1-sol/high）只读核对实际 merge parents/ref/十文件/无依赖漂移、009 测试和范围；无新确定性回归，不建议重跑16片。建议包含 packageVersion 和 generateUpdaterManifest，实际14文件组均已包含。

合并后的应用身份是 **0.26.0**，不是0.27；六项版本和对应测试/说明来自已授权 main 合并，没有自行 bump。正式0.26不再是当前版本的可用更新或历史目标；未来0.27下能否历史安装0.26仍须实际准入。普通签名安装不受九包表限制，但只交接安装器并依赖人工恢复；严格返回仍限原九包，0.26未加入。固定0.18.0 acceptance binding、签名和旧stable latest边界不变。

本轮没有新源完整 Rust 四 harness/16片、完整 Vitest、完整像素 CI、hosted CI、三平台签名包或真实安装/往返、D20/现场回归。实际变动仅主版本元数据/说明和一个回归测试，受影响域、类型/构建/政策及独立复核没有发现扩大验证的依据；故本轮不重复整套。旧源68fa6b3完整结果继续保留，不能称新源全量通过。

同源研究新增 OS 官方语义与“双DB同进程”最小反例，见 `docs/opencode-service-source-proof-0.27.md`；未接真实服务、读取真实DB/注册/凭据/正文、启删除或伪造issuer。原 todo 和此前所有失败/跳过记录未改写。本轮没有运行中验证、工具拒绝或未知服务操作。

文档/来源核对退出0，`v027-stage6-doc-check.log` 保留6份文档、33个本地链接、merge parents/恢复ref、已测tree与五域实际日志计数的校验；验证后生产源未再变化。之后仅保存文档和小型source元数据。后续分层归档功能已另获实现授权，将作为独立切片TDD与验证，不由本次merge结果宣称已交付。
