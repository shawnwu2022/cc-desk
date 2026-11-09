# 0.27 后续 P0 验证记录

执行环境 WSK-MSI，独立 clone `E:\Codex\2026-11-06\task-2\cc-desk`，分支 `feat/v0.27.0`。本回合从 `da8aaa6e0d2b8d4ea7ed198110e39bf0bb6f2dfd` 开始，不整合并行 0.26 main 更新，不推送、不发布、不改真实安装/CLI 数据。原有 `.audit/v027.todo.md` 和上一阶段失败材料保留。

## 实现提交

- `50b353886fd21cbe6fb5c7f5acc1e82f225c35ff`：安全 CLI 版本后端；辅助原提交 `0439a07379050f860dc969462ab86edf020fc73a`。
- `5d230917a938b50a0aaa489ea2e653b6006b7dea`：profile 独占探测 owner；辅助原提交 `496bcded0a528e38c30c77f5bd87dd517db2c2f7`。
- 前端、拒绝说明和最终证据提交 SHA 在交付中列出；此文件不嵌入自己的提交 SHA。

## 已通过

实际集成 checkout：

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib CliVersion_ -- --test-threads=1
21 passed, 0 failed, 1758 filtered out

cargo check --manifest-path src-tauri/Cargo.toml --locked --offline --lib
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --lib -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
均退出 0
```

日志 `v027-cli-versions-integrated.log`、`v027-p0-cargo-check.log`、`v027-p0-clippy.log`、`v027-p0-fmt.log`。沿用此前已准备的 ConPTY SDK。合成测试包含真实 Windows 子进程及安全 Cmd Shim；不是用户 CLI、真实终止失败或 Unix 验收。

独立只读审查配置为 `gpt-6.1-sol / high`。已修配置读取失败保留旧 known 的 P1；按补强后的契约增加未知 owner 的后端独占和前端原请求协调。末次复核基于 `5d230917` 加前端差异，未发现新的确定问题；审查者没有重跑测试，测试证据以本文件最终命令为准。

最终前端和生产验证：

```text
npm run test:ci
206 files passed; 3200 tests passed; 2 original skipped (3202 total)

npx vitest run tests/api/cliVersionDiagnostics.test.ts tests/composables/cliVersionDiagnostics.test.ts tests/components/cliVersionDiagnostics.test.ts tests/components/nativeCleanupAvailability.test.ts tests/utils/updateDiagnostics.test.ts tests/components/remainingSettings.test.ts
6 files; 45 passed

npm run typecheck
npm run build
均退出 0；build 仍有既有 500kB chunk 提示

npm run test:production
2/2 中英文真实生产入口与 CSP 通过，合成 IPC

npx playwright test tests/visual/unified-workspace.spec.ts -g 'diagnostics and conservative cleanup stay reachable'
2/2 中英文 1024x640 界面通过，四张截图已实际查看，无基线更新
```

日志分别为 `v027-p0-vitest-full.log`、`v027-p0-targeted-final.log`、`v027-p0-typecheck-corrected.log`、`v027-p0-build-final.log`、`v027-p0-production-final.log`、`v027-p0-visual.log`。全量 Vitest 之后仅做等价测试索引兼容修正和 Desk/Codex 归档恢复文案精确化，最后 45 项、构建、生产及视觉验证覆盖该最终状态；没有重复运行整库制造更多通过记录。

现行 CI 的 Node 政策聚合命令：

```text
node --test tests/scripts/releaseRecovery.node.cjs tests/scripts/nativeReleasePolicy.node.mjs tests/scripts/releaseCoverage.node.mjs tests/scripts/releaseDraftRecovery.node.mjs tests/scripts/windowsNativeValidation.node.mjs tests/scripts/historySignatureFixtures.node.mjs tests/scripts/historyRoundtripBuild.node.mjs tests/scripts/historyRoundtripDriver.node.mjs tests/scripts/historyPayloadPolicy.node.mjs tests/scripts/pasteAcceptance.node.cjs tests/scripts/pasteReleaseGate.node.cjs tests/scripts/pasteTrace.node.cjs tests/scripts/nativeLaunchDiagnostics.node.mjs tests/scripts/releaseBuildAdmission.node.mjs tests/scripts/windowsRustShards.node.mjs tests/scripts/windowsRustShardRunner.node.mjs tests/scripts/windowsRustCalibration.node.mjs tests/scripts/ownedJobDrain.node.mjs tests/scripts/ciRustWorkflow.node.mjs tests/scripts/ciBuildMetrics.node.mjs tests/scripts/sessionRenameSafety.node.mjs tests/scripts/opencodeObserver.node.mjs tests/scripts/terminalDropPaths.node.mjs
```

23 文件，共 316 项：310 通过、6 原有跳过、0 失败，日志 `v027-p0-node-policy.log`。

## 保留的失败与 RED

- `version-parser-red.log`：输出解析真实断言 RED；`version-parser-green.log` 保留后续通过。
- `version-check.log`：辅助 checkout 首次 check 缺生成的 SDK 资源，准备后通过；不是源码检查通过记录。
- `version-tests-final-18.log` / `version-cmd-repro.log` / `version-cmd-slash-repro.log`：新增 Cmd fixture 曾失败。最小复现显示 `System32/cmd.exe` 的混合分隔符使 cmd 自身退出 1，规范反斜杠为 0；只修 fixture 路径构造，没有放松时限、断言或生产解析。真实配置若使用不可工作的拼写仍保守失败，不 fallback。
- `version-profile-owner-red.log`：受控 pending owner 存在时新 ID 原先返回 known，019 断言失败；`version-profile-owner-green.log` 21 项通过。未伪造真实 OS kill/reap 失败的验收。
- 新前端模块最初缺失时 import 失败、零测试执行，属于功能缺失基线，不算行为断言 RED。首次 composable 实现 6 项失败及 7 个未处理拒绝来自 DOM fixture 缺 `crypto.randomUUID`；补 Node crypto seam，同时生产缺 UUID 时保守 unknown。
- `v027-version-stale-red.log` 与 `v027-p0-targeted-green.log` 的早期配置失败 fixture 注入了错误 seam，不能单独证明业务缺陷。后者实际 **15 pass / 2 fail**，名称不代表通过。第二项失败是测试把正常 DOM input/change 冒泡误当业务 emit；改为比较禁用点击前后事件集合，没有放松零 mutation 要求。
- 修正实际 `cli_list_profiles` seam 后，临时还原旧处理复现 **known 旧版本仍被复制**，见 `v027-version-stale-corrected-red.log`；随后恢复失效修复。
- `v027-version-unknown-red.log`：终止未知回执后 busy 错误为 false 的真实断言 RED；修复后 `v027-p0-reviewed-green.log` 5 文件 18/18 通过，末次 `v027-p0-targeted-final.log` 6 文件 45/45 覆盖 corrected RED 之后的最终源码。
- `v027-p0-typecheck.log`：新测试两处 `Array.at` 超出项目既有 TS 目标，TS2550 失败。改成等价索引，未调整 tsconfig、检查范围或断言；`v027-p0-typecheck-corrected.log` 退出 0，末次 45 项通过。

上一阶段的 `ProxyProbe_BodyTimeout_003` 30/31、六项原 5000ms process timeout 和后续通过记录继续保留在 `v027-validation.md`，不被本回合结果覆盖。

## 未运行、受阻或待外部输入

- 完整 Rust 四 harness/16 片、all-target Clippy、hosted CI 未运行；域内 21 项不能写成全 Rust 通过。
- 原生完整级联预览/执行仍被已核实的条件接口语义阻塞，当前只交付明确拒绝说明，见 `docs/native-cleanup-blocker-0.27.md`。不是工具自动审批拒绝，也不是等待用户确认。
- 真实签名 0.27 候选和隔离 stable→RC→stable 往返未运行；缺真实晋升发布产物，当前没有发布授权。准备矩阵见 `docs/acceptance-0.27.md`。
- 真实用户 CLI、Unix、安装版 WebView、三 CLI D20、双行现场、身份关联及 AI 命名未验收；真实 OS 终止失败未注入。

本回合合理使用已有 pstack 的任务/证据组织、domain/type 边界指导；superpowers TDD、systematic-debugging、requesting-code-review 和 verification-before-completion。没有安装新技能，也没有重复启动完整方案流程。
