# 0.27 启动批次验证

目标机器为 WSK-MSI。集成目录 `E:\Codex\2026-11-06\task-2\cc-desk`，分支 `feat/v0.27.0`，基线 `4d4674480849f1d4d8fb4edab095b388e769e189`。以下为本地工作区源码在最终提交前的实测；没有托管 CI 或发布验收。

## 最终通过

| 完整命令或入口 | 结果 | 本地原始输出 |
| --- | --- | --- |
| `npm run typecheck` | 退出 0 | `v027-typecheck-reviewed.log` |
| `npm run test:ci` | 202 文件，3184 通过、2 原跳过，共 3186 | `v027-vitest-reviewed.log` |
| `npm test -- tests/components/updateChannels.test.ts tests/components/remainingSettings.test.ts tests/components/backgroundUpdates.test.ts tests/stores/update.test.ts tests/utils/updatePolicy.test.ts tests/utils/updateDiagnostics.test.ts` | 6 文件 72/72 | `v027-final-review-green.log` |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | 退出 0 | 与 core 命令同次 shell，fmt 失败会停止后续测试 |
| `cargo check --manifest-path src-tauri/Cargo.toml --locked --offline --lib` | 退出 0 | `v027-rust-check.log` |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --lib -- -D warnings` | 退出 0 | `v027-rust-clippy.log` |
| `cargo test --manifest-path tests/updater-core/Cargo.toml --locked --offline -- --test-threads=1` | 实际 31/31 | `v027-updater-core.log` |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib desktop_updater -- --test-threads=1` | 实际 33/33，1725 过滤 | `v027-native-updater.log` |
| 下方 23 文件独立 Node policy 命令 | 316 总计，310 通过、6 原跳过、0 失败 | `v027-node-policy-final.log` |
| `npm run build` | 退出 0 | `v027-build-final.log`，后续 production 入口再次构建最终前端 |
| `npm run test:production` | 中英文 2/2；使用正常生产入口与实际 CSP，合成 IPC | `v027-production-reviewed.log` |
| `npx playwright test tests/visual/unified-workspace.spec.ts -g 'catalogue, selection and blocked preparation remain usable'` | 2/2 | `v027-history-browser.log` |
| `npx playwright test tests/visual/unified-workspace.spec.ts -g 'update channels and recovery guide remain reachable'` | 2/2 | `v027-update-browser.log` |

直接 Cargo 前运行 `node scripts/prepare-conpty.mjs`，固定资源准备成功；SDK 由既有 `prepare:session-sdk` 生成。最终原生与 core 都使用集成 checkout 各自独有 target，不将辅助 checkout 的编译包当作最终证据。核心 crate 保留两个原有 non_snake_case 测试名 warning；没有新增禁止项或放宽编译策略。前端构建保留既有大 chunk 提示。

Node 聚合完整命令为：

```powershell
node --test tests/scripts/releaseRecovery.node.cjs tests/scripts/nativeReleasePolicy.node.mjs tests/scripts/releaseCoverage.node.mjs tests/scripts/releaseDraftRecovery.node.mjs tests/scripts/windowsNativeValidation.node.mjs tests/scripts/historySignatureFixtures.node.mjs tests/scripts/historyRoundtripBuild.node.mjs tests/scripts/historyRoundtripDriver.node.mjs tests/scripts/historyPayloadPolicy.node.mjs tests/scripts/pasteAcceptance.node.cjs tests/scripts/pasteReleaseGate.node.cjs tests/scripts/pasteTrace.node.cjs tests/scripts/nativeLaunchDiagnostics.node.mjs tests/scripts/releaseBuildAdmission.node.mjs tests/scripts/windowsRustShards.node.mjs tests/scripts/windowsRustShardRunner.node.mjs tests/scripts/windowsRustCalibration.node.mjs tests/scripts/ownedJobDrain.node.mjs tests/scripts/ciRustWorkflow.node.mjs tests/scripts/ciBuildMetrics.node.mjs tests/scripts/sessionRenameSafety.node.mjs tests/scripts/opencodeObserver.node.mjs tests/scripts/terminalDropPaths.node.mjs
```

## 已发生失败，保留而不覆盖

1. `cargo test --manifest-path tests/updater-core/Cargo.toml --locked --offline` 一次为 30/31，`ProxyProbe_BodyTimeout_003` fixture 十秒未接受连接。完整说明及工具输出转录为 [失败说明](updater-proxyprobe-failure.md) 和 [转录](updater-proxyprobe-64262-transcript.txt)。后续串行通过不覆盖此失败。原因未定，存在既有 fixture 负载敏感线索；没有相同负载基线对照，不能排除新增测试工作量的影响。
2. 一次 `npm run test:ci` 与本任务 Windows 编译、Node 聚合和 build 并行，结果 196/202 文件通过，3176 项通过、6 项 process 超时、2 跳过，原始输出 `v027-vitest-final.log` 保留。六项名称见开发记录。这六个测试文件相对 RC3 没有 diff。不能仅凭并行背景断言根因；无本任务重编译时原命令复核为 3184 通过、2 原跳过，另存 `v027-vitest-reviewed.log`。
3. 新验签阶段回归曾因 `--exact` 缺少 `tests::` 前缀运行零项。该命令明确不计通过。正确命令 `cargo test --manifest-path tests/updater-core/Cargo.toml --locked --offline tests::signed_bytes_and_official_digests_are_both_required_before_install -- --exact` 实际运行一项并因 download/verify 阶段不符而 RED，输出 `v027-signature-stage-red-exact.log`。修复后上述 core 31 项全部 GREEN。
4. 最终独立审查两个竞态先以 `npm test -- tests/components/updateChannels.test.ts` 复现，8 项中两个明确断言失败，输出 `v027-final-review-red.log`；修复后的 72 项及最终全量通过分别记录，未删原断言或调整超时。

## 独立审查

只读审查实际调度配置 `gpt-6.1-sol / high`，没有额外模型切换。审查覆盖通道、发布门禁、签名、有界下载、取消竞争、过期回执、脱敏和恢复文案。第一轮确认窗遮挡和重挂载卡住问题已修。最终两项 P2 为未知设置被后台检查绕过、旧取消回执影响新下载，均已 RED/GREEN 并由原独立审查者复核关闭，未发现新的具体问题。

最终关键源码 SHA-256 与复核快照相同：

```text
src/components/settings/sections/UpdateSection.vue 42274654643CC3D5259AF6FBE802E938B206FC9078E4A090271E151FB071956D
src/stores/update.ts 736C1DDB57A2AD34B6CA674EFF3542031653AC0D748810373054C63C32756F69
tests/components/updateChannels.test.ts C63176D6170DDC2D9D602749358EABCA9F641C79850538D3DDA6FBE5D7575635
```

## 未运行与未完成

完整 Rust library 的其余 1725 项、四 harness/16 片 hosted 聚合、all-target Clippy、完整无更新截图基线套件、真实三平台签名构建与发布后校验未运行。只运行新增场景与历史交互的浏览器证据，未修改原截图基线。

没有签名 0.27 候选的隔离 stable→RC→stable 往返，没有安装版 WebView、Mac/Linux 实机或 D20 真实 CLI 证据；没有触碰用户安装、设置或会话。用户双行现场升级、身份关联和 AI 命名仍未验收。

安全清理只完成真实原生接口核查，完整后代预览及一次性执行未实现。CLI 版本安全采集未实现，诊断明确 not-collected。P1 忽略单个候选和持久更新记录未实现。不能将本批次报告为整个 0.27 已完成。
