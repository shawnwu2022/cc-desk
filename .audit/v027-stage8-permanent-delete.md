# 0.27 第八阶段：实际永久删除窄场景

## 来源与变更

目标 WSK-MSI；独立 clone `E:\Codex\2026-11-06\task-2\cc-desk`，`feat/v0.27.0`。实现提交 **`6de019a2ee22528d1c391e76c449c968c9846140`**，tree **`aeae3a67d369efdab26755aa4c15c8462bcaa265`**。之后只记录文档/实验脚本，不改变已验证产品源码。完整元数据见 `v027-stage8-source.json`。

整合前 aaad3d2b、固定正式 main 0aaaf2a1、无冲突 merge 89e30ff3、版本转换测试提交68436f31保留；本轮开工HEAD c58d5901。应用身份仍0.26.0，没有bump、推送、main写入、PR、tag或Release。当前 main 已是开发分支祖先，不能把整合前的RC3 merge-base说成当前 merge-base。

用户将方向从隐藏/可恢复归档纠正为永久删除，并明确要求二次确认和风险提示。旧归档片段分别保存两个可重放patch，核对hash后仅撤出本任务改动；原todo和先前失败/构建材料未修改。SHA256见source JSON。原生归档补丁的227pass只适用于那份被撤出的补丁，不算当前删除验收。

本轮实际支持Windows、同精确授权configured-profile、已原生归档的Codex根及原生动态子树；一次只选一根。三阶段API、独立双确认、未知结果只读协调、物理来源/容器句柄、整个own Job生命周期已实现。OpenCode/Claude及Unix执行分别禁用。完整语义、官方固定源码和未保证事项见 `docs/permanent-delete-0.27.md`。

## 最终验证命令与结果

Rust固定实际编译器 `1.98.1 (48a229cea 2026-09-01)`、`x86_64-pc-windows-msvc`、LLVM22.1.8：

```powershell
$env:RUSTUP_TOOLCHAIN='1.98.1-x86_64-pc-windows-msvc'
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_NET_OFFLINE='true'
node scripts/prepare-conpty.mjs
rustc -Vv
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib NativeDelete_ -- --test-threads=1
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

**7pass/0fail/0ignore、1780filtered**，4.53s；all-target Clippy退出0，13.11s；fmt退出0。最终日志 `v027-stage8-native-delete-worker-{compiler,tests-final2,clippy-final,fmt}.log`。根重新计算13个Rust文件的排序path+NUL+rawBytes+NUL摘要，确与worker的 `b3eaa0d6fb4ca9764737cfbc6e71c3c50fedd6cb10cc1349cff12845075c5295` 相同。较早212项projection和旧归档227项通过属于较早工作源，不声称当前源全部Rust通过；没有重复旧完整四harness/16片。

```text
npm test -- tests/native-cli/nativeSessionDeleteApi.test.ts tests/native-cli/nativeProjectionApi.test.ts tests/stores/nativeSessionDeletes.test.ts tests/stores/nativeCliAdapter.test.ts tests/stores/unifiedSessions.test.ts tests/stores/unifiedWorkspaceRuntime.test.ts tests/components/nativeSessionDelete.test.ts tests/components/nativeCleanupAvailability.test.ts tests/utils/sessionPresentation.test.ts tests/config/testProjects.test.ts
10 files / 408 pass / 0 fail

node --test tests/scripts/sessionRenameSafety.node.mjs
9 pass / 0 fail

npm run typecheck
npm run build
npm run test:production
exit 0 / exit 0 / en+zh CSP 2 passed

node .audit/v027-stage8-sdk-delete-fixture.mjs
3 isolated real-SDK cases passed, no CLI/server/query
```

最终前端 `v027-stage8-frontend-qualified.log`；最后类型 `v027-stage8-typecheck-qualified.log`；独立build `v027-stage8-build.log`；最终生产命令按项目配置重建正常生产入口，`v027-stage8-production-qualified.log`，2/2、9.6s。Node安全 `v027-stage8-node-safety.log`。SDK首次 `v027-stage8-sdk-delete-fixture.log`、加物理root检查后的最终 `v027-stage8-sdk-delete-fixture-final.log`；两份合成目录 AMYBUm/rJo41k 保留供检查，没有删除真实用户数据。

SDK三个真实隔离观察：删除合成主JSONL及子目录；删除私有hardlink不删任务自建original/child；普通Node开放写句柄没有阻止SDK删除且句柄仍可写。这里只证明固定SDK/Windows Node行为，不冒称真实Claude CLI活动验收。

## 已保留的RED、失败与修复

- 能力纠正 RED `delete-assessment-red.log`：3fail/1pass；最初缺新API模块的 `delete-api-red.log` 是脚手架失败，不算行为RED；补空实现后 `delete-api-behavior-red.log` 5fail/1pass为行为RED。
- store初次5fail、UI空实现初次5fail；UI初轮实现4fail/4unhandled errors是 `crypto.randomUUID` 在jsdom缺失，改复用安全getRandomValues生成RFC UUID，没有Math.random fallback。
- `delete-edges-red.log` 2fail/10pass/1unhandled：范围变化缺重新预览方法、factory缺桥异常在try外；修正后22pass。
- `delete-typecheck.log`：Object.hasOwn不在项目ES目标，以及TDD测试先于新方法的类型失败；改既有hasOwnProperty.call，没有抬编译目标，后续类型0。
- 独立审查 `delete-review-races-red.log` 3fail/19pass：API dispatch前微任务失权、store dispatch前失权、A迟到预览关闭B；相应guard及captured key/operationId修复后26pass。
- `delete-refresh-red.log` 1fail：重复click只发一次native但刷新两次；只在confirmation阶段接受首次界面执行，后续8pass。`delete-rereview-red.log` 1fail：A结果借B重新预览；sameTarget守卫后13pass，最终全10文件408pass。
- Rust `worker-tests.log` 完整保留Windows QueryInformationJobObject绑定E0308、exit101；`worker-tests-green.log`名字不表示成功，实际6pass/1fail，旧fixture将所有未知都断言为unknown，现按已定契约区分“owner settled且根仍在→partial”与“没有ACK且根缺失→unknown”。独审确认不是放宽成功断言。
- `worker-clippy.log` single_match失败、exit101，等价iflet修复；final2测试和clippy-final/fmt通过，旧日志未覆盖。
- 早期只有工具原始stdout、没有独立文件的证据：Node空合成环境缺SystemRoot导致CSPRNG启动失败；fixture启动管道/IPC迭代失败；运行中新增字段的fmt diff；实质Job RED断言 `launcher descendant escaped a released container lease`、`escaped != held`、exit101。修复创建suspended→加入Job→恢复，真实合成detached后代在全部结束前无法重命名容器。不能将这些stdout冒称磁盘日志。最终7项均通过。
- 提交前元数据脚本 `git rev-parse :src` 不能寻址index目录，未取到预期子树；`git write-tree`已成功且等于commit tree。之后使用正确 `HEAD:src` / `HEAD:src-tauri/src` / `HEAD:tests` 取得source JSON中真实子树；这是元数据采集失败，非测试通过证据。

前端短名日志均以 `.audit/v027-stage8-` 为前缀；Rust `worker-*` 短名对应 `.audit/v027-stage8-native-delete-worker-*`。旧BodyTimeout_003十秒未连接、六项5000ms进程超时、1.95错误工具链聚合失败等继续按旧记录保留，未借此阶段覆盖或放宽门槛。BodyTimeout原测试读路径与RC3相同，本轮未改该源码；不宣称其已被此次修好或已证明只是环境偶发。

## 独立审查与未跑项

实际使用superpowers TDD、系统调试、请求/接收独立审查和完成前验证；pstack figure-it-out用于有界方案与审计轨迹。没有安装技能或复制外部实现。根负责TS/UI/集成，唯一实现worker负责Rust，唯一只读审查者交叉核查官方源码、Job P1、两个归属P2及真实ACK保留。最终独审读取7/7、Clippy/fmt及rereview13/13日志后放行限定实现，没有替根重复跑套件。

未跑：真实Codex CLI删除/外部busy演练、安装版WebView、Unix/macOS、真实用户会话/身份现场、当前源完整Vitest/四harness16片/hosted CI、真实0.27签名候选stable→RC→stable。Unix旧E_ACCESS_DENIED未绕过。没有push/main集成/发布、用户安装或真实服务操作。生产CSP是正常构建+合成IPC，不是安装版WebView或原生CLI验收。

可以进入限定实现的候选审阅和隔离验收准备，不能发布或声称整版P0完成。最小剩余：获准的合成隔离真实Codex/Windows WebView输入；OpenCode受控来源与活动执行契约；Claude真实源SDK/活动保护；受保护发布流程产生的真实签名候选及隔离恢复环境。具体需求已明确，不是等待用户再次确认永久删除意图。

文档/来源一致性检查 `v027-stage8-doc-check.log`：9份文档、50个本地链接、三产品子树、三个受保护材料摘要和7份最终日志均核对通过；`git diff --check`退出0。之后仅保存文档/源元数据及已实际运行的SDK fixture脚本，不改变已验证产品源码。
