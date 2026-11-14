# Codex 归档修复验证记录（2026-11-14）

归档针对性验收通过；完整 Rust library 未通过。没有发布、安装或归档任何用户真实会话，也没有修改系统安全设置。

## 源码与执行边界

- 基线：b2540b7b80e7129f09e71b0d9da5206bc7e14d2d（0.27.0-rc.8）。
- 修复：51911dd27c461b04e8da4525ce9c9b063ad381b0。完整库执行的 Rust 内容与该提交一致，执行后才建立本地提交。
- 在独立工作树使用锁定 Cargo.lock、offline 编译和已校验 ConPTY 1.24.2607.10001；全库运行编译产物，参数 --test-threads=4，没有测试过滤或新增忽略。
- 完整库存：1932；1836 通过、48 失败、48 原有显式忽略；2675.36 秒，退出码 101。
- Windows 本地化错误文本经过终端转码；以下按未受影响的 HRESULT、固定阶段文本及完整测试名核对，不按损坏的中文消息推断。

## 已完成的归档验证

- 精确旧门禁先复现 RED；修复后归档组 34 通过、3 原有显式忽略。
- 当前已安装 Codex 0.169.0 的固定映像实测 8 案例通过，412.47 秒。目标/后代正在写入时 Busy 或 Unknown，无关 writer 不阻断，writer 持续存活并持锁，合成 root/child 原字节核对。完整边界见 [并发说明](codex-archive-concurrency.md)。
- 前端归档、adapter、workspace 三文件 113 通过，含恢复后不打开、直接点原历史行再次归档；不创建/切换终端，不增加普通归档二次确认。
- cargo fmt --check、cargo clippy --offline --locked --all-targets -- -D warnings、TypeScript/Vue 类型检查通过。
- 不把组件和合成 ConPTY 恢复链宣称为真实 CLI 恢复循环或安装版 UI 验收；SXY 历史原因仍未确认。

## 完整失败分组

### 注册表夹具：23 项

全部报告 HRESULT 0x80070005（拒绝访问）。只读源码核对确认，例如 registry-state 在创建专用测试键的 Fixture::new 就失败，未进入被测恢复行为。这是本轮执行的访问拒绝证据，不是归档门禁失效。没有重试这些写入、改 ACL 或安全设置。

- `tests::version_history_classes_alias::HistoryClassesAlias_HeldDrift_003`
- `tests::version_history_registration_windows::HistoryRegistration_SourceReadmission_001`
- `tests::version_history_registry_state_windows::HistoryRegistryState_AbsenceAndMissingTarget_006`
- `tests::version_history_registry_state_windows::HistoryRegistryState_CapacityBeforeMutation_011`
- `tests::version_history_registry_state_windows::HistoryRegistryState_CompleteCapture_001`
- `tests::version_history_registry_state_windows::HistoryRegistryState_CompletedReadback_014`
- `tests::version_history_registry_state_windows::HistoryRegistryState_ConflictRetentionFailure_008`
- `tests::version_history_registry_state_windows::HistoryRegistryState_ForeignJournalRoot_007`
- `tests::version_history_registry_state_windows::HistoryRegistryState_ForeignRunConflict_009`
- `tests::version_history_registry_state_windows::HistoryRegistryState_NamespaceChange_002`
- `tests::version_history_registry_state_windows::HistoryRegistryState_RefuseUnsupported_003`
- `tests::version_history_registry_state_windows::HistoryRegistryState_RetainRestore_004`
- `tests::version_history_registry_state_windows::HistoryRegistryState_RunGrammar_010`
- `tests::version_history_registry_state_windows::HistoryRegistryState_SharedParentDrift_012`
- `tests::version_history_registry_state_windows::HistoryRegistryState_UnknownNoReplay_005`
- `tests::version_history_registry_state_windows::HistoryRegistryState_VerifyOriginal_013`
- `tests::version_history_scope::HistoryScope_FixedUninstallSlot_018`
- `tests::version_history_scope::HistoryScope_RegistrationAliasRefusal_022`
- `tests::version_history_scope::HistoryScope_RegistrationRefusal_016`
- `tests::version_history_scope::HistoryScope_RegistryNamespaceRebind_019`
- `tests::version_history_scope::HistoryScope_RegistryVersion_023`
- `tests::version_history_scope::HistoryScope_RegistryViews_009`
- `tests::version_history_scope::HistoryScope_RelocatedImage_010`

### Manager / typed cleanup：18 项

全部明确报告 source host is job-contained。manager_process.rs 的 require_job_free_source 使用 IsProcessInJob 检查当前宿主，并在 manager 准备前拒绝。未解除外层 Job、改 token、关闭保护或将测试改为忽略。

- `version_history::windows::manager_process::tests::HistoryManagerProcess_ReadyChain_002`
- `version_history::windows::manager_process::tests::HistoryManagerProcess_SuspendedIdentity_001`
- `version_history::windows::manager_process::tests::HistoryManager_AdmittedJob_010`
- `version_history::windows::manager_process::tests::HistoryManager_ConcurrentHostJob_013`
- `version_history::windows::manager_process::tests::HistoryManager_ContainedSources_005`
- `version_history::windows::manager_process::tests::HistoryManager_DisarmUnknown_006`
- `version_history::windows::manager_process::tests::HistoryManager_JobCollision_007`
- `version_history::windows::manager_process::tests::HistoryManager_LifetimeCollision_012`
- `version_history::windows::manager_process::tests::HistoryManager_RejectReceiptMix_009`
- `version_history::windows::manager_process::tests::HistoryManager_SourceCrash_004`
- `version_history::windows::manager_process::tests::HistoryManager_TypedChildren_011`
- `version_history::windows::manager_process::tests::HistoryManager_UniqueJobs_008`
- `version_history::windows::manager_process::tests::typed_probe::HistoryManager_MarkerTimeout_016`
- `version_history::windows::manager_process::tests::typed_probe::HistoryManager_PrepareTimeout_015`
- `version_history::windows::manager_process::tests::typed_probe::HistoryManager_RejectExit124_020`
- `version_history::windows::manager_process::tests::typed_probe::HistoryManager_RejectFallback_019`
- `version_history::windows::manager_process::tests::typed_probe::HistoryManager_RunningFailure_018`
- `version_history::windows::manager_process::tests::typed_probe::HistoryManager_SuspendedFailure_017`

### WebView2 边界：5 项

Channel 无完成回执；document 停在 main-build-start；launch 停在 app_setup_started；projection 停在 event-loop-start,main-build-start；history WebView 未收到退出回执。超时不能证明这些产品行为通过。仅 D12 做了下述独立基线复现，其余四项仍未证明属于既有失败。

- `tests::native_cli_channel_live::D11_Channel_Native_011`
- `tests::native_cli_document_live::D11_Webview_Live_001`
- `tests::native_cli_launch_live::worker::D11_Launch_Native_001`
- `tests::native_cli_projection_live::D12_Webview_FormalCommands_001`
- `tests::version_history_webview::HistoryWebView_Exit_001`

### Restart lifetime 诊断：2 项

两项报告 confined Medium diagnostic worker must complete，底层 HRESULT 0x80070057（参数错误）。现有日志未定位到 run_scoped_worker 的具体 Win32 调用，不能据此归因到归档或擅自修改权限。只读核对 token.rs 与对应测试均未被本修复修改，未扩大诊断。

- `version_history::windows::process::restart_lifetime_tests::RestartLifetime_LiveRootObservation_001`
- `version_history::windows::process::restart_lifetime_tests::RestartLifetime_TerminalBeforeReopen_002`

## 只读核对与单例基线

上述失败测试文件、version_history/windows 实现以及 version_history_payload/token.rs 在 b2540b7b 与 51911dd2 之间 git diff --exit-code 为空（退出码 0）。这证明未修改这些路径，不单独构成它们运行结果不变的证明。

另建精确 b2540b7b 的 detached 工作树，正常校验资源并编译原始 1927 项库清单，在与修复版相同的受限执行上下文只运行：

```text
cc_desk-<hash>.exe --exact tests::native_cli_projection_live::D12_Webview_FormalCommands_001 --nocapture --test-threads=1
```

结果：0 通过、1 失败、1926 过滤，90.12 秒，退出码 101。基线与修复版都在 native_cli_projection_live.rs:202 报 D12 worker timeout: event-loop-start,main-build-start。因此这一项失败已在未修改基线复现。未重跑全部库，未重试注册表访问拒绝或解除 Job 门禁；也未把这一个基线结果泛化到其余 47 项。

## 保留的原始证据

原始日志保存在本机 %TEMP%/cc-desk-archive-diagnosis-20261114/；完整48项 panic 输出另有 full-lib-failures.json。下列 SHA-256 固定本次记录：

- full-lib-regression.log: `c84d19f2e4aa9a42df09c0ecae6ef3b6a9363cd0cf7a85cd7be2631393f364e6`
- full-lib-failures.json: `a079cfdfb519cc59c1a1d1df434e6850c2c8d09daf6e5eb8b6cdc105f46fa71b`
- green-native-archive.log: `2bc38878dab5320f1bebf78a399c67db95e3959605d58c1ee84a6b200e34f103`
- green-native-0169-eight-case.log: `a852e6c18a6b0b18f8529e3b376226f5374631be9ef1815642883d5553eec8a3`
- frontend-restore-cycle.log: `6b2d3b3f707e6e4eff65c2e8a3ef78f146bb16ed88049c3c11d42f306e6b0c46`
- baseline-b254-D12.log: `9fdf949bae6f90bdbe9a6d4494b9156bdf301d8ab574afb0339a78ac94e0453b`

## 剩余验证

归档修复可按已完成的独立验收审查；完整库不得标为通过。其余 WebView 与两个 restart-lifetime 的进一步定位需要逐例有界诊断或同环境基线比较；注册表和 Job-free 专项应在已有授权且满足前提的 Windows 测试宿主上补测，不通过提升权限、修改安全设置或放宽断言来消除失败。本次到此收口，不扩大为平台测试环境修复。

## PR CI preparation compatibility follow-up

PR #89 at head 36cbdd56 reached ordinary CI run [43590268694](https://github.com/shawnwu2022/cc-desk/actions/runs/43590268694), attempt 1. Frontend checks passed. Rust producer/static and both roundtrip jobs failed before compiling the archive change: Windows PowerShell `Expand-Archive` could not find the three-argument `ExtractToDirectory` overload. Historical-recovery run 43590268704 and ConPTY-integration run 43590268617 independently failed at the same preparation step. These failures are not archive test failures or successful Rust validation.

The preparation script is unchanged between b2540b7b and 36cbdd56. Baseline run [43374558457](https://github.com/shawnwu2022/cc-desk/actions/runs/43374558457), job 131717893821, logged successful fresh pinned ConPTY preparation on Windows image 20261101.353.1. The failing jobs used image 20261112.366.7. This establishes compatibility drift in the preparation environment; it does not identify which image component introduced it.

The narrow repair uses the explicit .NET Framework two-argument ZIP API with the same Windows PowerShell process and environment-data path arguments. Package and file hashes, download limits, x64 checks, staging checks and failure cleanup are unchanged. No machine module, permission, registry, SSH, credential or release-gate setting changed.

Local regression executes the unmodified generator logic from a disposable checkout against a local HTTP fixture and genuine ZIP, without mocking the extraction subprocess. Before the repair, the real installed PowerShell reproduced `MethodCountCouldNotFindBest,Expand-Archive` (9 passed, 2 failed). After the repair, 31 ConPTY/CI/release-policy tests passed, including fresh staging, corrupt ZIP, mismatching package/file hashes, Unicode/literal staging paths, and cleanup on success/failure. The earlier full Rust library results above remain separate; they were not reclassified or rerun for this build-script-only change.

- RED log SHA-256: `e9e044ee6b8b9d2ff64c4af385997b12800fe760916409cd20e5bd5f40adcc06`.
- GREEN log SHA-256: `09ab39f1d83f60dbd9dde16b546b90cfe8f7382bc3b26472f08a426125439081`.
- Logs: `conpty-extraction-red.log` and `conpty-extraction-green.log` in the same local evidence directory described above.
- Independent read-only review found no blocking issue. New hosted CI must still validate the repaired exact source; baseline success and local fixture results do not replace it.

A separate empty temporary checkout successfully downloaded and staged the official pinned ConPTY 1.24.2607.10001 package, then passed `--verify` against the unchanged manifest. The first network attempt failed with `fetch failed`; a later read-only package fetch returned HTTP 200 and the fresh retry completed. This is fresh preparation evidence, not a cached-runtime shortcut.
