# Codex 真实 CLI 隔离验收记录

基线 `25c37ac1b7ea2b6f63884a063da324e5e4f1562f`，`feat/v0.27.0`，WSK-MSI。只允许现有二进制和本任务从零创建的合成配置、历史、库、进程；不读取真实会话/secret，不登录、调用模型、继承外部MCP/hooks/认证，不升级或安装CLI，不改真实权限/配置、发布资源或其他树。阶段进度见 `v027-stage10.todo.md`；未运行项目不能由设计或源码审查代替。

## 固定二进制和源码

安装目标：`D:\nodejs\node_global\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe`，SHA256 `edfdd48f8b458af0906e507b4cb05217b326c3d6e515e9f024e05e3b8d650f70`、369062704字节、普通非reparse文件。没有通过PATH选择其他程序。**本轮隔离真实版本已观察为0.167.0**；过去阶段3的0.166输出是旧记录，本轮没有升级或安装CLI。

初始隔离分析按旧记录研究固定 `rust-v0.166.0` / [`6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`](https://github.com/openai/codex/commit/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856)；以下链接保留初始研究来源。实际版本探测后已另取匹配的 **`rust-v0.167.0` / [`086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a`](https://github.com/openai/codex/commit/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a)** 做限定差异核查；不能把初始0.166分析本身当当前二进制证明。

- [配置加载](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/config/src/loader/mod.rs#L806)：Windows系统配置使用KnownFolder ProgramData以及HKLM64 policy RequirementsPath，不受伪造ProgramData环境变量重定向。requirements能覆盖sqlite_home，故仅CODEX_HOME不足。
- [用户根](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/utils/home-dir/src/lib.rs#L14)：显式CODEX_HOME必须存在并被规范化。独立DB由config.sqlite_home/CODEX_SQLITE_HOME选择，二者都需指任务根。
- [项目根发现](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/config/src/loader/mod.rs#L1694)：空.git目录会被跳过，必须有任务合成HEAD；[配置遍历](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/config/src/loader/mod.rs#L1844) 截断于已发现根。fixture不使用真实gitdir/commondir。
- [file认证存储](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/login/src/auth/storage.rs#L246)：只在合成CODEX_HOME/auth.json查找，缺失为None，没有keyring回落。MCP OAuth也显式file，无账号或认证环境。
- [插件启动门禁](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/core-plugins/src/manager.rs#L2885)：空plugins表不足，必须features.plugins=false；同时关remote_plugin/plugin_discovery_prefetch/skill_mcp_dependency_install和hooks。
- [技能惰性入口](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/ext/skills/src/host_roots.rs#L56)：Windows dirs6/dirs-sys0.5使用KnownFolder而非HOME。不能泛称所有home目录已被env隔离；本实验只开放不触发该扫描的有限协议，禁止skills、thread/start/resume/turn/query等请求。SkillsWatcher构造不扫描，register_thread_config才读roots。
- [initialize认证分支](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/request_processors/initialize_processor.rs#L150)：普通客户端省略explicitGatewayOauth会恢复automatic login；必须true。不使用Codex Desktop名称，避免该文件193行Windows安装归属注册；experimentalApi关闭，未请求设备验证。
- [startup-config](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/startup_config.rs#L51) 也经过auth/cloud config bootstrap，必须先有隔离证明；它只输出appServerVersion/configRequirements，不证明物理sqlite_home。
- [SessionMeta](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/protocol/src/protocol.rs#L3359) 和 [原生删除](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/app-server/src/request_processors/thread_delete.rs#L31) 是合成历史和执行范围依据。禁止模型调用来生成子会话；没有实际图证据不宣称级联已验收。

## 已完成的隔离前置检查

根仅查询KnownFolder系统namespace的文件元数据、HKLM64指定policy key是否存在；没有读取registry values或配置内容。`C:\ProgramData\OpenAI\Codex`由Get-Item返回明确ItemNotFound，HKLM policy key为null；没有把吞掉access-denied的File.Exists(false)作为最终证明。实际运行器每次启动前后重新核查，出现、无权或未知即停止。

fixture始终自身mkdtemp/TempDir创建并保留，不接受外部数据路径。CODEX_HOME、独立sqlite_home、log、HOME/USERPROFILE、APPDATA/LOCALAPPDATA、XDG、TMP/TEMP、项目和缓存全部指新任务目录；env_clear后只加入白名单SystemRoot/WINDIR/System32 PATH与合成路径。Git全局配置指任务空文件，NOSYSTEM=1，工作目录具有自有.git/HEAD。CLI/MCP认证均file、无auth.json、无凭据环境。dummy provider仅loopback9、无env_key/bearer/catalog/auth command；analytics/feedback关闭且OTLP三exporter=none，不能仅靠“不发turn”推断零网络副作用。

所有probe包括version/help也suspended创建并加入own Job再resume。固定exe以句柄和哈希租约保持；owner线程持有Job、fixture及Root/container leases直至direct child已reap且ActiveProcesses=0。前端等待超时只报pending，不释放后台owner。spawn后的查询/输出/终止错误走统一协调，不以TerminateJob请求当完成；只终止本次Job。

## 已发现并修复的产品问题

`native_delete::exchange` 原initialize缺少explicitGatewayOauth。新增003安全断言后真实行为RED：发送值Null而期望Bool(true)，`0 pass/1 fail/1799 filtered`，exit101；日志 `.audit/stage10-auto-login-red.log`。最小修复仅增加该capability和意图注释，不启实验API、不冒充豁免客户端，不改变删除/来源/确认准入。GREEN及真实协议结果待下文按实际补入。

## 测试运行器与失败保留

新增 `native_cli_delete_live.rs` 为显式本机验收入口，默认套件不运行真实已安装CLI。新synthetic worker与真实probe分别有ignore说明，不修改任何旧忽略/宿主排除。

- 初次Cargo普通sandbox无法打开本任务target/debug/.cargo-build-lock（os5），未执行测试；必要require_escalated获准后执行。不是auto-review拒绝，不修改ACL/token。
- 合成001“已存在system namespace应拒绝”和002“插件必须显式关闭”均有先RED；不把这些错误说成真实CLI失败。
- 生命周期003/004首次观察GREEN，未冒称先RED：晚退出own Rust descendant、pending仍持owner/目录、注入观察fault后own Job回收。`.audit/stage10-live-synthetic-001.log` **4 pass、0 fail、2 ignore、1797 filtered**；ignore是真实A入口及只由本测试启动的合成child。
- 初次fmt --check只报告新live.rs和mod排序格式差异；普通sandbox格式写入os5未修改文件，后续在任务许可内处理。原失败保留，最终结果另列。

当前记录只覆盖已经观察的前置检查/合成运行器和产品RED，不构成真实CLI或WebView通过。精确最终命令、env名单/文件摘要、实际原生成功/拒绝/未知、未跑项和最终提交在完成相应步骤后追加。

## 实际版本差异与阶段A原始结果

首次A `stage10-real-stage-a-001.log` 在spawn前返回INSTALLED_EXECUTABLE_UNSAFE：自行设置的256MiB测试上限不适配实际352MiB二进制，0pass/1fail，真实CLI未执行。只读元数据证明它是普通文件且原SHA未变后，测试改为固定精确369062704字节而非提高任意程序上限。

`stage10-real-stage-a-002.log` 中实际 `--version` **exit0**，stdout18字节、stderr0、rootReaped=true、JobEmpty=true；白名单读取为 `codex-cli 0.167.0`，stdout SHA256 `ff63aca76f41814af0725bba286b7311709b1cfd7fbb17df9e17d18eaa3c5e62`。测试原0.166预期不匹配使整条入口exit101，随后3项未执行；不是CLI版本命令失败，也不放宽产品CLI版本范围。fixture `C:\Users\wusha\AppData\Local\Temp\v027-codex-delete-live-UqJ4ip` 只存在version三份报告，原始结果保留。

取得真实0.167版本后，根比较17份相关官方文件，9份逐字相同；对其余变化逐项读差异，独审另核auth/MCP/initialize/models/cache：未发现改变本限定A隔离合同的新增入口。GitHub compare响应文件表截断300，不宣称全上游树已审；缺失文件另直接比较原文件并校验差异重建相同。路径、当前Git blob和范围在 `v027-stage10-upstream-comparison.json`。auth变化加入保存锁/原子提交、异步读与缓存；file缺失仍None，无keyring新增来源。initialize新增仅experimentalApi条件的人类approval分支，实验不开该API。帮助/strict startup-config可在新正确来源下补跑；已成功version不重放，B尚未执行。

`stage10-real-stage-a-003.log` 只执行未跑的3项，**1 test pass、0 fail、1802 filtered，45.06s**，没有重放version：

| 实际参数 | exit / stderr | stdout字节 / SHA256 |
| --- | --- | --- |
| `--help` | 0 / 0字节 | 5940 / `d38f0c75ef44deb9f6f3f40c3bed05aefc44e36905432361c4254a1d799a50bc` |
| `app-server --strict-config --help` | 0 / 0字节 | 3703 / `d2854a3ad9c9733f8be8034feb198ba017ddce07d539d8e777167fc96dffbd10` |
| `app-server --strict-config --print-startup-config` | 0 / 0字节 | 111 / `a9e7ac97ecf4f4ba6ef4caab372395c17f3c293c46d5dde55bfdcad4614819bc` |

各项rootReaped/JobEmpty=true，前后metadata gate通过。startup两字段存在，appServerVersion实际0.167.0。fixture `C:\Users\wusha\AppData\Local\Temp\v027-codex-delete-live-yXIYzu` 有界库存34项，无reparse、auth.json或DB；sqlite/sessions/archived_sessions均空，仅另创建codex-home/tmp/arg0-windows空目录。没有initialize、模型或delete。

执行cwd为仓库根，固定环境为RUSTUP_TOOLCHAIN=1.98.1-x86_64-pc-windows-msvc、CARGO_PROFILE_DEV_DEBUG=0、CARGO_PROFILE_TEST_DEBUG=0、CARGO_NET_OFFLINE=true；真实入口另显式CC_DESK_NATIVE_DELETE_LIVE_GO=stage-a-isolated-probes。精确命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib tests::native_cli_delete_live::NativeDeleteLive_InstalledCodexStageA_099 -- --exact --ignored --nocapture --test-threads=1`，最终exit0。原始A001/A002仍各自失败，不借A003覆盖。

产品OAuth最小修复已通过 `stage10-auto-login-green.log` **7/7**（1796 filtered，4.48s）；新运行器最终合成 `stage10-live-synthetic-final3.log` **4 pass/2 ignored**，strict all-target Clippy/fmt final4退出0。一次fmt-final3表达式换行diff退出1保留，仅对新测试格式化后验证；无逻辑放宽。最初空输出Tee未生成fmt-final.log，未把它称已保存，后续fmt摘要显式记录EXIT_CODE。

B在下一节已有一次真实产品后端执行；不能把A读配置或直接Service caller当作实际IPC、可见Vue双确认或安装版验收。

## 首个真实B：原未知结果保留

仅新建的 `C:\Users\wusha\AppData\Local\Temp\v027-codex-delete-live-AbzPpB`，同fixture strictprint成功后，真实产品Service调用已执行；不是用户数据。`stage10-real-stage-b-001.log` **exit101，0 pass/1 fail、1804 filtered、29.43s**。六组本地保护通过，但正向确认返回 **unknown / DELETE_OUTCOME_UNKNOWN / rootPresent=true**，因此未通过真实删除验收，停止且未重发。

原件删除前单链接、FileID `[2083130666,2359296,1299268]`、字节相同、SHA256 `c78b319646043ec17cb438eb7de943a240e6319e19b6a7a209e1225c4337d0c6`；确认前已关闭该reader，不预设POSIX unlink-open。删后只接受symlink_metadata的明确NotFound；本次为false，原件及未归档/无关/同UUID另一根三个控制记录保留。owner已实际metadata idle，后置检查和断言才结束宿主。未到duplicate/inspect成功断言，不把原件存在说成“未发送delete/零关联维护”。

实际初始化已在任务根生成installation_id、内置skills/.system、sandbox日志，独立sqlite目录出现state_5/queue_1/goals_1/memories_1及WAL/SHM，log目录出现logs_2及WAL/SHM；无auth.json和reparse。根只读打开这个任务logs_2.sqlite，readOnly+query_only，schema合法但logs表为空，不能猜测RPC失败阶段。

下一步仅新fixture的initialize→initialized→thread/read→EOF只读诊断，不发delete或模型调用；原未知目标不重放。取得实际帧形状/错误后再判断是产品或fixture问题，不能靠把期望改为unknown来制造正向通过。

只读运行器006的ACK状态转换先在 `stage10-readonly-protocol-red.log` 失败（exit101，收到initialize ACK后仍未产生initialized/read），实现后 `stage10-readonly-synthetic-001.log` 为6 pass / 4 ignored，exit0；共享进程owner新增stdin路径，因此连同003/004生命周期回归。严格Clippy和fmt分别见 `stage10-readonly-clippy-001.log` / `stage10-readonly-fmt-001.log`，均exit0。真实101入口冻结后另做NoMutation复核，只允许任务新根的初始化和读取。它的协议证据只能解释这个新请求，不能反向证明B001没有发送delete；B001阶段无记录，继续unknown且不重放。

独立NoMutation复核匹配live.rs SHA256 `5d712614925f215d1c7ec1eb0deaa1f05d184fa1492c7de89d6e19eebaca459c` 后放行一次101。init同源ACK前不会写initialized/read；输入/解析失败统一EOF/终止后等root reap与整个Job empty，不提前释放目录和二进制租约。trace只保存有界帧形状、数字errorCode、line SHA；原始结果不外发。与生产包的一项形态差异是显式 `experimentalApi:false`，生产省略该字段；不是B001逐字回放。

### 新只读101结果和有界根因

`stage10-real-readonly-001.log` exit0，1 pass / 1806 filtered，16.20s。新fixture `C:\Users\wusha\AppData\Local\Temp\v027-codex-delete-live-Sn6sCD`；实际只发initialize、initialized、thread/read，读到同源且UUID匹配的合成原件，原字节未变，EOF正常退出、rootReaped/JobEmpty=true、forced=false。stdout1503字节、stderr0。安全trace SHA256 `ff32b29e29ee7fc5e8011767d9d7ed5e2c469cb696434b99d96deb63e0ce377c`；stdout SHA256 `733b358776c99a3bba74ff5034c9156be56716c683b6ace5d7b8f31b19976c74`；测试日志SHA256 `b4d12e8ce0db0475e13917bd5586b58266b67a635eba2253c63767794eee3f8d`。固定4个Rust环境变量同A，另CC_DESK_NATIVE_DELETE_LIVE_GO=stage-b-readonly-protocol，精确命令 `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib tests::native_cli_delete_live::NativeDeleteLive_ReadOnlyNativeProtocol_101 -- --exact --ignored --nocapture --test-threads=1`。

实际帧为id1 ACK→无id的remoteControl/status/changed→id2 ACK。固定086ccc的 [协议](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/app-server-protocol/src/protocol/common.rs#L2081)、[payload](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/app-server-protocol/src/protocol/v2/remote_control.rs#L26-L35)、[初始化发送位置](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/app-server/src/lib.rs#L1264-L1277) 已独立复核：仅转发连接状态与身份元数据，本帧不启动session；不推断整个remote-control传输没有执行能力。产品receive此前拒绝该精确通知，会在delete发送前结束协议。对应真实通知的合成004已复现RED：无delete marker，trace止于thread/read，state unknown；同批008/009分别复现准备阶段丢失和收口后被误分unknown。`stage10-protocol-stage-red.log`：6 pass / 3 fail，exit101。前端008同样将安全阶段回执误丢成unknown，`stage10-api-stage-red.log`：7 pass / 1 fail，exit1。

上述是新请求可重现的具体缺陷，不反向修改B001。修复只增加精确无id、params对象的状态通知；新增本助手尚未发送delete的固定阶段说明，写入开始后仍保守unknown。独立重验前不声称真实删除已通过。

### 最小修复和新B002正向验收

最小产品修复已独立本地提交 **`515b75bb0488d69e04ba90f6f4051de4668863e8`**（6文件）；验收运行器与文档/索引另收口，不混同发布或完整套件。

根实现后独审核对三个生产文件：native_delete `8c79c92fd7fbc0eb6b9b30ab7991dea2ef13c3eb6656a413464761e5f9a2d175`、launch_service `ffdfdac25efd77dbd036f23100a8fbdd7c80384222c0f5a1ac1efd8571f7ce1c`、projection/service `fc64d1caab3f7e8792d2ebbfc159e670fe7fd3cbe5c4ec3dbaf45bcb22f9a563`。仅精准忽略无id且params为object的remoteControl状态通知，未放开其它方法或server request。实际send_packet在current成功后、首次delete序列化前置backend-held标记；此前初始化/读取失败用固定DELETE_INITIALIZE_FAILED/DELETE_READ_FAILED，worker未开始删除写入的其它准备失败用DELETE_PREPARE_FAILED，均待实际Job收口后发布blocked。已开始写入后的写/flush/RPC error/ACK丢失仍unknown；pending优先unknown，迟到的收口结果才可只读转为准确阶段，不自动重放。这里只说明本helper没有发delete，不声称其它进程没改来源，也不声称初始化没有原生维护写。

`stage10-send-boundary-red.log` 是010行为RED（writer报错时开始标记仍false），exit101。修复后 `stage10-protocol-stage-green.log` **10/10 pass**、1800 filtered、4.62s；前端 `stage10-frontend-stage-green.log` **3文件24/24 pass**，包含safe阶段回执、账本、双确认、不重放；`stage10-protocol-clippy.log` 严格all-target Clippy exit0，`stage10-protocol-typecheck.log` 类型检查exit0。最初前端RED命令因沙箱out-file AccessDenied未形成测试结果，$LASTEXITCODE=0不是PASS；在已授权工作区权限下重试一次，取得上述真实RED。没有自动审批拒绝或待批准动作。

生产修复和独审后仅对新fixture `C:\Users\wusha\AppData\Local\Temp\v027-codex-delete-live-v6Nsyk` 运行B100一次；**不是B001重放**。`stage10-real-stage-b-002.log` exit0，1 pass / 1809 filtered，35.80s。精确命令同B001：`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib tests::native_cli_delete_live::NativeDeleteLive_InstalledCodexBackendStageB_100 -- --exact --ignored --nocapture --test-threads=1`，固定4个Rust环境变量另CC_DESK_NATIVE_DELETE_LIVE_GO=stage-b-product-delete。

真实productService→受控CLI initialize/read/delete收到ACK、rootPresent=false，原件symlink_metadata明确NotFound。原件之前nlink=1、FileID `[2083130666,2490368,1429628]`，字节SHA256 `a4358d7fc9f68fe68fbaf2fc0d8770510d1a0cac2cd2d7b82feea038edc6edbb`；执行前已关闭reader，不是删私有副本。未归档根、同源无关根、另一root同UUID三个控制原件保留。重复确认和inspect复用原回执；metadata owner/整个Job均已收口后才结束断言。结果报告SHA256 `84d329abcfccebd2f2a8c09a5538c67d8cf94519d1d14e4ec7ecd669dc57576a`，日志SHA256 `1c95d334135aa4083e89ba997d4e256691b8192cd5c8a9477e72c3dee3924535`。

报告SHA256已在收口时从实际文件重算核对。

边界仍是直接Service/caller，不是authenticated document IPC、真实可见Vue双确认、安装版WebView或逐后代清单。未调用model、resume/start/turn，未升级CLI或改用户安装，未触用户数据、main、发布资源。

### 启动前错误分类的补齐

随后按相同要求补查worker之前的确定no-send路径：明确选中的程序不存在时，freeze/resolve_process准备错误还会漏成unknown。新增008只使用UnavailableFixture，不运行CLI，`stage10-prespawn-red.log` exit101、0 pass/1 fail/1812 filtered，实际unknown而预期blocked；原件未变、ownerIdle=true，结果留在任务新fixture svSo63。修复只把原准备段包入Result tuple闭包，共用pre_dispatch_failure；这段没有spawn，thread::Builder.spawn明确失败也归prepare，保留busy/args/root/source固定域码。不改已开始写入后的保守unknown。

最终受影响域命令 `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib NativeDelete -- --nocapture`，固定1.98.1/CI debug0/offline，`stage10-final-native-delete.log` **18 pass / 5 ignored / 1790 filtered / 0 fail，5.39s，exit0**。5个ignored分别为手动A/B/101/102真实入口及仅供owner调用的合成后代worker，未把它们计为普通GREEN。最终strict `cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` 与 `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check` 均exit0，日志为stage10-final-clippy.log / stage10-final-format.log。前端24项/typecheck源未再改，保留原结果，不重复整套。

最终准备修复源码摘要：launch_service `8266c1db7f212064920aae0574d221b897818b176236edec33808bd3b643df42`，native_delete `6798b7519ac947e5248293fa0f9698adc49946fec903beb6bcdca7f4d802bcc0`。B002正向真实证据绑定此前515b75bb实现；最终准备错误补齐另有上述合成回归，不能把B002或旧16片伪称新源完整重验。

### 持合成原生writer锁的真实负例102

按固定086ccc的 [writer_lock.rs](https://github.com/openai/codex/blob/086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a/codex-rs/rollout/src/writer_lock.rs#L44) 建立任务自己持有的thread-writer-locks/UUID.lock，使用相同std::File.try_lock，锁文件分享READ/WRITE/DELETE以排除分享模式本身拒绝访问。held目录和锁文件FileID前后校验、第二独立句柄实际WouldBlock。007先RED→GREEN，保留 `stage10-writer-lock-red.log` / `stage10-writer-lock-green.log`；不是启动真实模型writer。

独审要求收紧负例，不能将初始化/来源blocked算成锁结果：冻结live SHA256 `280f49c443d46f329d7cf729f1871a292a62a066877cf58ddd25677abd8da52b`，必须initial unknown/DELETE_OUTCOME_UNKNOWN、inspect partial/DELETE_NOT_ACKNOWLEDGED，且root=true、原件/控制字节不变、锁一直争用。仅对此新fixture放行一次真实102，未重跑B100或原B001。

`stage10-real-writer-lock-001.log` **exit0，1 pass / 1812 filtered，35.19s**。fixture `C:\Users\wusha\AppData\Local\Temp\v027-codex-delete-live-F7C8ky`；固定4个Rust环境变量另CC_DESK_NATIVE_DELETE_LIVE_GO=stage-b-writer-lock，精确命令 `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib tests::native_cli_delete_live::NativeDeleteLive_InstalledCodexWriterLock_102 -- --exact --ignored --nocapture --test-threads=1`。

实际initial和inspect符合上述严格值；原件SHA256 `c7f6d9585e88323e3636153d426e7aae38ad0ba099fb94a97044c8cc8ed773a9`、三个control SHA前后一致，锁前后继续WouldBlock，整Job/metadata owner已settled。inspect前后重复确认均复用原回执；最后才释放合成锁，未解锁重发。报告明确 `syntheticWriterLockOnly=true / runningModelWriter=false / nativeRpcErrorProven=false / associatedCleanupUnverified=true`。这证明持锁条件下Desk本次未取得成功回执且所观察原件保留；不证明服务从未发送ACK、具体原生RPC错误或所有关联维护无改动。

## 最终边界和后续

- 已完成：受控现有CLI真实版本/help/strict配置；新101读取；新B002单根原件永久删除；新102持合成原生锁的无ACK保留观察；产品双ack/来源/操作票据与只读不重放协调；18项最终Rust、24项未变前端、类型/严格Clippy/fmt与独审。
- 未运行：本阶段的真实authenticated document IPC、可见Vue双确认、安装版WebView、Unix、完整16片/完整Vitest/生产构建/CSP和hosted CI；旧阶段对应结果仅是各自旧源证据，不移植成当前PASS。本轮直接Service caller和DOM假桥已经显式标注。
- 未运行：真实spawn/attached/history-reference、多后代部分删除与真实断网/超时故障矩阵。当前只创建metadata记录；没有调用会触发尚未隔离的全局skill配置扫描的start/fork/resume/turn来补图，不用模型/真实数据代替安全fixture合同。单根成功不替代这些验收。
- Claude/OpenCode生产删除仍关闭；具体源/维护准入、正常生命周期、崩溃/部分失败、需新增的协议或用户操作、验证入口已写入后续维护设计，已独审，但不是实现。没有ACL权限变更、停外部会话、新服务、真实数据或凭据操作。
- 签名0.27候选隔离升降级往返与目标平台/现场验收仍缺授权产物/隔离安装输入，不申请secret或测试账号、不发布来制造证据。当前可继续候选审阅准备，不能宣布整版P0或发布完成。

完整文件、日志和安全结果摘要见本阶段source.json。51个本地文档链接有界核对无缺失，git diff --check通过；未重跑已通过旧整套。所有原失败、unknown、跳过和旧材料保持原样。

## Fast 请求核查

用户要求仅当地今日（UTC+08:00）开启Fast。执行时钟2026-11-06 14:38:43 UTC，即当地22:38:43，今日截止16:00:00 UTC。当前工具catalog未暴露本执行线程serviceTier/fast读写或到期控制；create/followup的model/thinking参数不能充当service tier控制。因此**没有启用Fast、没有设置回执或持久到期任务**，也没有修改模型、推理等级、全局配置或被测CLI以假装启用。此为控制入口不可用，非审批拒绝；不阻断当前验收。

Claude/OpenCode维护设计见 [后续设计](../docs/provider-delete-maintenance-design-0.27.md)，仅设计、执行仍关闭；不为本轮新启两家服务或改生产权限。
