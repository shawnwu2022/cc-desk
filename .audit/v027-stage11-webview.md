# Stage11 — 真实document IPC与隔离WebView永久删除

Base HEAD `1044345ba6fc22ff943883005e87d9cdfac4f930`，feat/v0.27.0，WSK-MSI。当前跟踪区干净；原todo、补丁和历史fixture均保留。计划见 `docs/superpowers/plans/2026-11-06-native-delete-webview-acceptance.md`。

## Spec / 授权

本轮只补真实document IPC与任务自建Wry的永久删除入口。生产双确认/风险提示、正式document/source鉴权与命令都必须实际运行；不能mock桥、加生产测试后门、调用应用正常启动或用户安装版。全部app/workspace/WebView/project/profile/CLI/sqlite/cache均任务新建。不登录、不调用模型、不读真实历史/凭据，不接真实服务。原B001 unknown不重放。仅本地提交，不push/main/发布；Fast无控制回执，未启用。

## 初始证据与决定

- NativeRuntime::production使用KnownFolder home/.cc-box，因此本轮不用；使用任务WorkspaceRepository和NativeRuntime::new，复用initialize_main及正式commands。
- 锁定Tauri2.11.1/Wry0.55.1：data_directory传入CreateCoreWebView2EnvironmentWithOptions；现有version_history::windows::webview提供reject_manager_overrides/actual_udf，可复用其真实环境身份校验。
- Microsoft官方说明环境与注册表可以覆盖UDF及浏览器参数，故不靠HOME假定隔离。metadata-only查询HKLM/HKCU、32/64位的Software\\Policies\\Microsoft\\Edge\\WebView2，四者均不存在；未读注册表values。正式harness仍须重新检查，未知即拒绝。
- 资产用测试context的精确本地URL，NativeRuntime真实expected_main_url/build_main发放document授权；不是给外部页面绕过鉴权。生产组件/API/store不替换；测试设施仅建立合成选择、驱动、负例和结果记录。
- Ruling: 用户已明确继续执行并限定现有独立分支，沿用该工作区，不新增worktree、不停下来等待计划确认。
- Ruling: 用户明确不重跑全16片，按实际变更运行受影响组，技能中的默认全套不覆盖当前用户限制。

## 当前进度

- [x] 核对HEAD/工作区/指令；有界读取NativeRuntime、D12 Wry、阶段10Fixture、真实UDF校验和官方覆盖语义。
- [ ] NoNative隔离WebView/生产组件/正式IPC验收。
- [ ] 源冻结与独审后真实CLI新合成原件验收及截图。
- [ ] 对应验证、证据索引与本地提交。

## 实际检查点

- 生产组件成功路径确有缺口：invalidate整份history后catalog只遍历缓存，未重读，同来源controls行也消失。`stage11-history-refresh-red.log` 3 fail/8 pass（002及010读取成功/失败分支）；最小改为准确origin的force load且retainOnFailure，再catalog刷新。`stage11-history-refresh-green.log` 4文件60 pass，测试用生产reader/adapter，未以harness补刷新掩盖。
- 专属Vite构建001通过（44生产模块）；真实端口类型检查001发现ProjectsState没有getArchivedSessions。按生产装配组合useSessionStore纯内存getter与projects的archive/restore，不调用默认load，typecheck002通过；构建002通过（47生产模块）。这些只是资产/类型证据，未算Wry通过。
- 资产边界001已取得真实行为RED（stage11-assets-red.log，exit101），Rust唯一实现worker补有界读取、摘要、白名单、server与隔离宿主；根拥有前端/组件/审计，避免同文件并发。
- Ruling: 长宿主接线采用AGENTS允许的单个Rust实现子代理，根负责前端/跨模块边界，明确文件所有权；当前原生命令串行，不重复验证。此安排取代技能的默认逐任务纯inline方式。
- 研发机短暂断连后已核HEAD仍1044345b，既有三命令exit明确，原单测PID29020已退出，fixture无installation_id；阶段11尚未创建Wry/CLI Job，未有未决删除或重放。
- 阶段10/原todo/旧补丁均保护；全部本阶段真实Wry/CLI入口尚待冻结/独审/显式Go，不能把上述结果扩称UI通过。
- 2文件生产修复已独审并local commit `f8d8dd16a0a168dfdaf377c7c1674bce527c6014`；harness/新资产仍独立未提交。
- 资产加载001 GREEN见stage11-assets-green-001.log；server002真实RED后已在stage11-boundaries-red-002.log通过，003 checkpoint顺序/多发计数保护尚缺实现的RED同日志保留。后续纯测试收口另记，均未运行Wry。
- 构建003绑定47个生产模块及3个驱动源，manifest SHA256 `6096abc8eebb58f282e5c65e7895130bf99568e6094f3c8156c0d003181e0122`；buildHead104，生产模块内容与f8d8dd16一致。执行前须按最终源重建，不移植为UI证据。
- 正常生产前端另执行 `node node_modules/vite/bin/vite.js build --outDir .audit/stage11-product-dist`（沿用已就绪SDK，未重跑prepare步骤），exit0/6.50s，1021模块；原dist保留。仅大chunk提示，未放宽阈值，不等同安装包/WebView运行或完整npm build。
- 本机锁定webview2-com0.38.2 options.rs69 OS-primary-account SSO默认false；wry0.55.1使用default选项，browser extensions默认false。仍以清空env/策略拒绝/明确UDF/实际环境身份及父Job浏览器归属为本轮执行前置，不用HOME或loopback单独证明。
- 本地计划受用户global ignore的superpowers/规则忽略，保留原位置，不改全局ignore；本审计保留关键spec、接口、决定与结果。

官方参考：https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/webview2-idl#createcorewebview2environmentwithoptions 。没有把静态计划或阶段10结果算作本轮UI通过。

## 执行前增量审查

- 独审发现测试页面模块顶层已创建store、写3次localStorage并mount，早于实际UDF与browser父Job证明。新增 `PermanentDeleteWebView_ModuleDoesNotWriteOrMountBeforeNativeOwnershipProof_001`：`stage11-bootstrap-ownership-red.log` 1 fail，明确观察3次写入；随后将全部装配移至原生完成证明后调用的start中。该测试只import模块，不造document bridge、不启动窗口或CLI。
- `stage11-bootstrap-ownership-green.log`：5文件61项通过；`stage11-typecheck-003.log` exit0。`stage11-frontend-build-005.log` exit0，47个生产模块、3个驱动源；当前manifest SHA256 `b6110a6f25583bcfda36d361ca677fcd0f46d96eaa383edba8542f6cc845acc3`，此前004摘要作废，不用于本轮Go。
- Rust纯边界 `stage11-host-compile-002.log` 4 pass/3 ignored。第一次编译的两处类型错误记录保留；Clippy001的zombie_processes失败保留，实际补wait/reap后 `stage11-host-clippy-002.log` exit0，fmt001 exit0。尚不是Wry/CLI运行证据。
- 独审另要求父driver记录browser创建前未决阶段，避免PID报告前失败时空browser集合提前释放租约；Rust worker正在补最小生命周期标记，当前NoNative仍HOLD。固定资产摘要将作为独立Go门禁复核，不能只证明manifest自洽。
- 当前执行时钟为2026-11-07 UTC+08:00（UTC为11月6日）。用户当日Fast期限已过；始终未取得当前线程控制入口或setting回执，没有启用或修改任何CLI/全局设置。

## 命令与证据范围

所有Cargo命令使用 `RUSTUP_TOOLCHAIN=1.98.1-x86_64-pc-windows-msvc`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`、`CARGO_NET_OFFLINE=true`。没有重跑四harness/16片；旧全套证据仍只属于其原提交。

```text
npm test -- tests/native-cli/nativeDeleteWebviewBootstrap.test.ts tests/components/nativeSessionDelete.test.ts tests/stores/nativeSessionDeletes.test.ts tests/native-cli/nativeSessionDeleteApi.test.ts tests/stores/nativeHistory.test.ts
npm run typecheck
node scripts/build-native-delete-webview.mjs
node node_modules/vite/bin/vite.js build --outDir .audit/stage11-product-dist
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib tests::native_cli_delete_live::webview:: -- --nocapture
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
```

本节列验证入口，不把尚未运行的ignored测试写成通过。专属测试壳使用真实Vue组件、Pinia store、API client、NativeCliAdapter及生产document/source鉴权和五个正式命令；任务选择、点击驱动、负例检查、截图及生命周期观察是cfg(test)/测试资产。真实document IPC、真实CLI与可见截图仍须分别用本轮实际运行日志证明，单元测试的I/O替身不算这些证据。

仍不涵盖用户安装版启动、真实用户profile/会话、模型调用、Claude/OpenCode mutation、Unix、多后代/部分删除矩阵或签名候选安装往返。原B001未知请求保留且不重放。

`stage11-creation-guards-001.log` 是本轮新增保留失败：4 pass / 1 fail / 3 ignored，`NativeDeleteWebView_ServerServesOnlyPinnedMemory_002` 的404断言失败；新pre-PID marker项通过。不能用后续GREEN覆盖该失败，也不能扩宽路由、超时或NoNative准入。独审同时指出父层应先确认root退出再读取creation marker，避免退出前最后一次写入与空集合检查竞态。

响应故障已取得独立反例：`stage11-server-split-red.log`，006拆分请求头测试1 fail，server得到 `WouldBlock / OS 10035`，client得到 `ConnectionAborted / OS 10053`。这是本轮测试宿主新建HTTP server的缺陷，未改变生产HTTP代码；原因与[Windows accept继承监听套接字属性](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-accept)一致。修复应仅将已接受stream恢复blocking，保留原有读写时限和路径白名单；后续GREEN另记。

## NoNative执行门禁

- 最终 `stage11-creation-guards-002.log`：6 pass / 3 ignored，未运行ignored GUI；`stage11-host-clippy-003.log` strict all-target exit0；`stage11-host-fmt-002.log` exit0。
- 独审增量通过：main/peer创建前marker、先try_wait再读marker、root/Job/browser/未决创建联合退出条件闭环；接受外部固定manifest摘要后加载器再校验源与内存快照。
- 根在 `2026-11-06T16:40:41.9006417Z` 独立核对HEAD `f8d8dd16a0a168dfdaf377c7c1674bce527c6014`、子harness SHA256 `307fa3038030865f46901f3602bfafc4ab5b58fedec4f9ea2a6b1dce0cde9ade`、父harness SHA256 `0157e267e3e98b38e7bed5deb89c039e5a14f8f6210fffbb64fa8628a29ca331`、manifest SHA256 `b6110a6f25583bcfda36d361ca677fcd0f46d96eaa383edba8542f6cc845acc3` 全部匹配后，发出一次仅NoNative090的Go。
- 正式命令为上述四固定环境加 `CC_DESK_NATIVE_DELETE_WEBVIEW_GO=stage11-no-native`，`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib tests::native_cli_delete_live::webview::NativeDeleteWebView_NoNative_090 -- --exact --ignored --nocapture --test-threads=1`。运行日志 `stage11-webview-no-native-001.log`；这里只记录初次历史许可，结果见下节，不构成再次执行授权。Native091仍HOLD。

## 本次真实NoNative：失败与未决

**没有获得本轮document IPC、生产双确认或可见界面通过证据。Native091没有Go、没有执行。**

唯一一次NoNative由session `71914` 执行，父任务目录 `C:\Users\wusha\AppData\Local\Temp\v027-codex-delete-live-Q8q5x9`，子任务目录为其 `temporary\v027-codex-delete-live-9EZwNW`。worker输出1 failed（2.04秒），故障 `ACTUAL_UDF_OR_BROWSER_UNKNOWN`；`actualUdfCount=0`、`confirmIpcCount=0`、`inspectIpcCount=0`、checkpoint与capture为空。页面start门禁没有放行，没有真实CLI删除。`app.run_return`记录0不代表测试成功；worker末尾success断言失败。父driver仍持续PENDING，没有最终exit码，不能把worker的日志完成当成父层root reap/Job empty或完整浏览器退出证据。

- 子报告已复制 `v027-stage11-evidence/no-native-001-result.json`，SHA256 `4ca6a33e6df12291ff60d7f21934ad06e9044b5c04066ebdb3ef4eae752f76e3`。
- 已完成worker日志复制 `v027-stage11-evidence/no-native-001-worker.log`，SHA256 `9c7cdaf9e6459dde1a140a6a09ad56f856ed0b407644be16d110ce65c47e0a13`。
- 父日志只保存 `v027-stage11-evidence/no-native-001-parent-prefix.log` 在 `2026-11-06T16:51:43Z` 附近的快照，SHA256 `a7053b2d30715b6b110f779e065d9ad0d38c8ac355e3d30460e9b284015284ee`；活动原日志继续增长，不冒充最终记录。
- 该时刻任务自己的CLI目录无installation_id，独立sqlite目录为空。NoNative配置的CLI程序不存在。原B001没有重放。
- 报告的 `engineVersion=147.0.3912.98` 来自 `tauri::webview_version()`，是可用运行时查询；因本次未取得实例PID/UDF证明，不能将其写成已绑定实例的完整provenance。

## 已证实配置缺口与未证明事项

初始预检遗漏了最后一层转换。锁定 `tauri-runtime 2.11.3` 的[固定官方源码](https://github.com/tauri-apps/tauri/blob/6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a/crates/tauri-runtime/src/webview.rs#L447-L514)：`From<&WindowConfig>`没有读取data_directory，new将其置None。`tauri 2.11.1` 的from_config最终调用该转换；其[manager默认分支](https://github.com/tauri-apps/tauri/blob/e5ae5b93cdd310045191cc0526f253140ad64b87/crates/tauri/src/manager/webview.rs#L534-L550)随后解析默认LocalData/identifier。因此仅填写WindowConfig绝对路径不能证明本次隔离。该代码缺口已由独审复核；不是自动审批拒绝。

本次generic错误丢失了actual_udf、identity比较、expected.recheck、CoreWebView2/PID各阶段的区别。不能由此声称实际identity mismatch、实际UDF路径或零浏览器副作用。没有打开、扫描或清理可能的默认UDF，也没有启动用户安装版应用、真实CLI、登录或模型。测试context为既有D11 fixture（version0.0.0、identifier io.github.shawnwu2022.ccdesk.d11test），仓库应用仍0.26.0；不是0.27候选安装版。

父宿主持有的未知browser创建标记、Job与目录租约继续保留。**不得直接终止宿主释放租约、伪造BrowserReady、重放本次请求或再次启动相同/新的GUI试验来掩盖未决状态。** 本轮按用户的“隔离不可证明则停该执行部分”约束停止后续WebView和Native091；没有把开发完成或单元GREEN写成真实UI通过。

## 下一次执行的最小条件（未实现/未执行）

1. 先对session71914原own Job/浏览器生命周期作受限只读核实，取得真实root reap、browser退出及Job empty证明后再按合同收口；证据仍不全就保留pending。不能通过改日志/marker制造证明。
2. 使用已核源码会实际写入attributes的显式builder.data_directory路径，并保持生产document bootstrap、SourceRef鉴权及正式命令。下述后续纯代码切片已实现并通过编译/独审，真实API回归尚未运行；下一次Go仍需端到端证明，不能将setter或MockRuntime当作真实UDF证据。
3. 在任何可失败UDF操作前保留有界的安全阶段/错误码和可验证browser生命周期见证；禁止输出proof、凭据或默认目录内容。此为本次宿主观测缺口，不冒充已修复。
4. 上述完成并通过TDD/独审后，才可对全新任务fixture重新取得NoNative Go；通过且收尾后，再单独申请内部Native091执行许可。仍只有用户既有产品开发/合成测试授权，未授权真实用户删除或发布。

独审收口确认：配置传播缺口确证、真实UDF与生命周期仍未知、NoNative失败且Native091未运行；生产history刷新修复 `f8d8dd16` 的独立证据保持有效。候选发布门槛仍包含本轮未通过的真实document/UI路径、签名stable→0.27候选→stable隔离往返，以及完整安装版/平台/现场回归；不得发布。

## 父约束更新后的纯代码修复与存量观察

`document_tauri.rs` 的最小修复SHA256 `c605f3f3417736b7bdaa0e17f044571306de2fe9ad1fa5ebbcdc96f67bc372d0`：先拒绝Some(relative)，Some(absolute)显式对from_config返回的原builder调用data_directory，None原样；authority/bootstrap/navigation/page-load/attach没有改动，无新issuer/生产测试后门、无环境覆盖。固定Tauri setter直接写入attributes，后续runtime-wry/Wry将其传至CreateCoreWebView2EnvironmentWithOptions；这是代码链证据，未冒称真实UDF绿色回归。锁定MockRuntime直接丢弃该属性，没有构造虚假的模拟绿色测试。

四固定环境下 `cargo check --locked --manifest-path src-tauri/Cargo.toml --lib`（35.63秒）、`cargo clippy --locked --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`（11.46秒）、fmt--check均exit0，日志 `stage11-directory-check-001.log`、`stage11-directory-clippy-lib-001.log`、`stage11-directory-fmt-001.log`。此次没有运行cargo test或重链占用中的test.exe。独审已核实际diff，仅这几行；原NoNative RED仍保留，GREEN要等存量生命周期有证据收口后另行Go。

自动审批拒绝了原包含all-target Clippy的整条验证命令，理由是可能编译/链接正在占用的测试exe；该整条命令未执行。按规则读取历史后未重发，只运行审核明确允许的lib-only安全替代。不能把旧host-clippy-003的全target通过归到新document_tauri源码。

### 精确进程事实与拒绝记录

- WMI动作：`Get-CimInstance -ClassName Win32_Process -Filter "Name='cc_desk-e220344963676309.exe'"`，随后拟按精确任务ExecutablePath及NoNative090命令行过滤。原始返回 `Get-CimInstance: 拒绝访问`，exit1，未取到记录。此为权限拒绝，不能说API不支持；不是自动审批拒绝。未重试该动作、未提权、未使用另一接口取得被拒的命令行数据。
- 受支持的基本Get-Process观察已获PID `52908`，唯一精确执行路径为本任务 `src-tauri/target/debug/deps/cc_desk-e220344963676309.exe`，创建时间 `2026-11-06T16:41:05.3893493Z`，当时HasExited=false（观察时刻 `2026-11-06T16:56:13.5461785Z`）。这证明父宿主归属，未证明其held child/Job/browser已经退出。
- 一次性 `.audit/stage11-observe-held-lifetime.ps1` 只针对该PID/精确创建时刻，在当前token请求QUERY_INFORMATION|DUP_HANDLE|SYNCHRONIZE，PSS flags0x3c只取held handle metadata，无VM_READ/VAclone/ReadProcessMemory、无命令行、无kill/CLOSE_SOURCE/ACL/提权。SDK布局与权限边界经独审；它不接入产品或自动测试设施。
- 该唯一观察尝试session11956最终exit1；外层Tee写 `.audit/stage11-held-lifetime-observation-001.log` 遭权限拒绝，catch写相同日志也被拒，未收到JSON或原生阶段回执。**只证实日志落盘被拒，PSS/OpenProcess是否执行/成功仍不确定**，不得将其报成process权限拒绝、Job空或成功收口。按规则读取历史后停止，没有重发、换法或扩rights。
- parent session71914及其租约继续保留。没有终止PID52908、未知browser或用户进程；没有打开、清理或迁移可能的默认UDF，没有启动第二个窗口/CLI。当前最小解阻仍是取得本次原held child/Job与未知browser生命周期的可核验收口证据，而非空列表或重跑。

父最新指示已要求本回合停止新增观察实验/窗口，按blocked收口；上述observer没有重发。已通过lib-only及实际diff独审的setter修复本地提交 `31cbc156b2075a35901032f6d8066f0a53080887`。原NoNative使用的是f8d8dd16基线加当时未提交但已固定摘要的harness；不得把该失败归为setter修复后的运行结果，也不得把旧全target或旧16片GREEN归到新源码。最终来源索引为 `v027-stage11-source.json`，原B001和阶段10证据保持原样。

首次交付暂存后的git whitespace检查对4份原始输出的末尾空行报错，已保存delivery-check记录，没有执行该次commit。原始字节不裁剪、不改换行；最终将冻结源码快照、全部原始日志及报告按字节校验归档 `v027-stage11-raw-evidence.zip`，本地原文件保留。Git仅提交压缩证据、索引/说明和源代码，不放宽源码whitespace检查。活动parent原日志不归为冻结文件，归档仅含先前已保存的prefix快照。
