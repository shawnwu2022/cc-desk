# CC Desk 项目错题集

目的：把已经付过的排查成本变成下次动手前的检查，而不是再造一份架构规范。
每次任务先读本页清单，再查相关条目；权威行为仍以当前用户要求、AGENTS 和专题契约为准。

## 开始前的最小检查

1. 核对实际机器、worktree、分支、HEAD、dirty 和仍在运行的任务。断线恢复先收日志/退出状态，不盲目重放写操作或测试。
2. 明确本次版本与范围。主干发布不得夹带独立开发分支；提交、推送、合并、CI、签名和发布分别核实。
3. 找到相关条目，读证据中的实际代码/测试。记录适用版本与证据层级，旧结论不自动适用于新版本。
4. 修复先取得命中行为的失败证据，再验证同源码修复。编译错误、fixture 初始化失败、未执行到断言不算行为 RED。
5. 优先最小回归；必要时补完整门禁。零测试、部分日志、超时、进程仍运行都不算通过。
6. 区分模型/fixture、真实生产核心、真实 CLI、WebView、手机公网。每一层的通过只说明那一层。

## 阅读索引

| 条目 | 涉及任务 |
| --- | --- |
| L01 原生归档不等于界面隐藏 | 会话归档、恢复、列表刷新 |
| L02 退出码不等于所有权已收口 | PTY、子进程、超时、取消 |
| L03 不能为探测缺口静默换输入协议 | Codex 粘贴、CLI 能力 |
| L04 失败归因必须有可比基线 | Windows 测试、CI、发布 |
| L05 帧数不是流的内存上界 | 远程传输、TLS、浏览器 |
| L06 登记状态不是安装或授权凭证 | 配对、无人值守、撤销 |
| L07 真实 CLI 要分层验证 | 新 CLI 接入、版本与恢复 |
| L08 文档入口不等于实际加载 | Codex、Claude、项目指引 |
| L09 版本晋级不只改 manifests | 发布、固定版本/日期断言 |
| L10 发布门通过不等于版本目标已交付 | 标准打包、用户目标、真实设备验收 |


## L01 原生归档不等于界面隐藏

- **已证事实**：原修复改用官方 `thread/archive`，不再以 Desk `archivedSessions` 冒充原生归档。归档前重复读取整份历史拖慢反馈；后台未知写屏障若未只读核清，会妨碍恢复后再归档。
- **防再犯**：先确认 source/profile/原生 ID 与 held root；保留 pending/unknown 屏障，核查物理归档结果。刷新失败不能重新发送 mutation。根 ACK 不证明所有后代成功。
- **最小回归**：原生归档、丢 ACK 后只读核查、恢复后再归档、非相关 owner 与未知 owner 的不同准入。
- **证据**：[原生归档契约](native-cli-v3.md#codex-原生归档连接)，提交 `bc05143b`、`49b61169` 的代码与 `native_cli_archive` 测试。
- **边界**：此条不固定永久删除/弹窗产品交互，必须读取当前版本最新要求。

## L02 退出码不等于所有权已收口

- **已证事实**：Windows 缓存退出码曾不足以完成 command owner，导致恢复返回成功后仍挡住立即再归档。修复要求精确句柄已 signalled 并 reap admission。异步 timeout 丢弃 JoinHandle 也不会自动终止、等待该任务。
- **防再犯**：以持有的精确句柄处理本次进程/任务；超时路径保留 handle，停止后 await/join，并区分“请求停止”和“确认退出”。不要终止同名的无关进程。
- **最小回归**：退出/EOF/超时/撤销、迟到结束、立即下一次准入；检查 reader/worker 收口。
- **证据**：`038f4aca`（`platform/admitted_child.rs` 与原生归档测试）；`e01a8845`（远程 TLS 探针 proxy 回收）。后者是分支测试代码，不证明所有生产 owner 已修复。

## L03 不能为探测缺口静默换输入协议

- **已证事实**：`ab92f3c2` 曾因缺被动能力接口把 `nativeTextPaste` 固定为 false，撤销 Codex 原生粘贴路由，专题契约已明确禁止重复此回退。
- **防再犯**：按冻结 attempt/binding 走原生 Ctrl+V；失败/unknown 不改发正文、不重放；不把可选 observer 当进程或输入权限。
- **最小回归**：两个入口只发一个快捷键字节、无正文读取、过期 attempt、超限样本和失败不回退；真实草稿/图片及 WebView 单独验收。
- **证据**：[粘贴契约](paste-framing.md)。旧版 CLI 兼容范围读该文，不能靠猜测降级。

## L04 失败归因必须有可比基线

- **已证事实**：Antigravity 分支全 Rust 库 18 个 `source host is job-contained` 失败，在精确起点 `038f4aca` 的同环境专项中复现，失败名称集合完全一致。只能据此称为已复现基线/环境限制，不能称全库绿色。
- **防再犯**：记录源码、工具链、执行环境、完整失败集合；先做小范围基线对照，不反复全库。禁止改 Job/ACL/token、删断言或扩大 ignored 来伪造通过。
- **最小回归**：复跑受影响行为；发布仍检查精确 SHA 的完整所需证据，timeout 或部分输出不算成功。
- **证据**：`38b3611d:docs/antigravity-cli.md` 的本地验证记录；[CI 性能记录](ci-performance.md) 与 [发布流程](release-process.md)。

## L05 帧数不是流的内存上界

- **已证事实**：合法 Snapshot 突发曾被 4-frame 队列保护拒绝；底层短读使一逻辑批次也可能产生很多外帧。修复用了固定字节 ring 与显式拉取；单独改 ring 不是全链证明。
- **防再犯**：分别计 message、ring、read 副本、WASM、TLS record、累积 Snapshot；写成功、读入和真实释放不同。不能靠增队列/延截止/缩终端使测试绿，也不能假定同一 TLS 流的控制消息能越过受阻 bulk。
- **最小回归**：碎片、环回、恰满/+1、错误路由原子拒绝、取消/固定总截止、真实流及资源退出。
- **证据**：`c8914c0a` 固定 ring；`35d4f80a` TLS 探针；`d88567a8` 61 秒组合验收。通过范围是本机真实中继加模拟浏览器，不代表手机公网或已上线。

## L06 登记状态不是安装或授权凭证

- **已证事实**：原流程先保存 Active，后请求浏览器安装；拒绝安装可留下 Active。修复新增独立、版本化、身份绑定的历史安装事实。确认后成功 UI 在锁外发布还可能覆盖取消，已用竞态反例修正。
- **防再犯**：区分 Active、安装完成事实、当前候选、本机批准与持续授权；不要用路径、ID、DTO 或历史 Confirmed 直接授网络权。阻塞 I/O 后复核取消/截止/原 run；成功发布与取消要有明确线性化点。
- **最小回归**：保存成功/安装拒绝、旧数据未验证、Forget/修改/CAS 失效、取消与迟到成功、重开后的事实与权限区别。
- **证据**：`ed1904c0` 真实注册反例、`c4388235` 安装事实、`78e6fe3e` 候选、`08333902` 本机批准准备。它们不单独证明长期 owner 或真实设备授权完成。

## L07 真实 CLI 要分层验证

- **已证事实**：`agy` 的官方帮助、真实 TUI 和 CC Desk 生产 OwnedPty 启动分别验证；1.5.9 未认证启动时 Ctrl+C 未在测试期限内退出，后续成功的是 owned stop，不是键盘优雅退出。
- **防再犯**：核官方参数、实际路径/签名/版本/哈希；不把 IDE 启动器当 CLI，不把 `--continue` 当 picker，不猜历史格式。未知能力关闭而非借用另一 CLI。版本变化未直接观察原因时只记变化。
- **最小回归**：默认关闭、显式启用、新建/指定 ID argv、实际启动/输出/停止；登录后真实恢复及可视布局另验。
- **证据**：`5fd6ad5f`、`38b3611d` 与该提交的 `docs/antigravity-cli.md`。真实恢复仍未验证，不因入口存在就宣布可用。

### L07 补充：临时 HOME 不隔离 OS 凭据

- **已证事实**：AGY 官方认证文档声明本机启动会尝试 OS keyring，存在令牌可静默登录。原 smoke 仅改部分目录环境，未隔离 OS 用户。
- **防再犯**：运行真实 CLI 前检查认证发现来源；不得因没有点击登录就推断没有使用凭据。无凭据授权时只运行纯参数/能力契约，真实认证与恢复另用已授权隔离身份。
- **证据与边界**：[官方认证说明](https://antigravity.google/docs/install/#local-silent-keyring-sign-in)、[AGY 边界记录](antigravity-cli.md)。这是历史隔离保证不足，不是已证明凭据泄漏。

## L08 文档入口不等于实际加载

- **已证事实**：仓库 2026-10-11 的 Claude 深层启动检查中，祖先导入因外部信任未获批准而未加载；根会话再读目标文件能加载目录规则。普通 Markdown 链接不是通用自动导入。
- **防再犯**：`AGENTS.md` 为唯一基线，`CLAUDE.md` 用同目录原生 `@./AGENTS.md` 薄壳；先从根开始，主动读目标目录规则和本错题集。不要复制规则或修改客户端信任来绕过加载边界。
- **最小检查**：追踪所有入口、相对路径、导入正文/预算；客户端加载需新会话核来源，静态路径通过不等于模型遵循。
- **证据**：[指南加载与维护](agent-guides.md)。

## L09 版本晋级不只改 manifests

- **已证事实**：0.27 正式版准备后仍需单独修正 `tests/config/packageVersion.test.ts` 的固定版本与 changelog 日期期望；`1d6d435f` 将 RC12 / 2026-11-16 同步为 0.27.0 / 2026-11-17，最终进入 `a51b80f7`。只检查 manifest/lock 一致不足以覆盖这类断言。
- **不再犯**：版本晋级同时检查 package、npm lock、Cargo manifest/lock、Tauri 配置、changelog 和固定版本/日期测试；保留历史不可发布测试版本断言，不做全局字符串替换。提交 PR 前先运行下面的定向检查，再按发布流程跑完整门禁。
- **最小回归**：`npm test -- tests/config/packageVersion.test.ts`，核对 5 条用例真实执行；定向通过不替代完整 CI、签名与发布后验收。
- **证据**：`1d6d435f`、[固定版本测试](../tests/config/packageVersion.test.ts)、[发布流程](release-process.md)。此次教训依据已提交差异；历史 CI 失败次数应以对应运行日志为准，不从最终修复提交反推。


## L10 发布门通过不等于版本目标已交付

- **已证事实**：0.28.0-rc.1 的标准 Windows 配置仍只执行 SDK 准备与 Vite，未传 remote-host，也未构建浏览器两份 runtime。独立 remote-trial 的通过和文档披露不能证明标准安装包已经交付远程能力。此次把“已知缺口已披露”当成可发布判断，是交付判断失误。
- **防再犯**：先把用户目标映射到实际标准构建、产物、可见入口和真实操作链。区分编入能力、默认启用、授权、可访问的手机站点与端到端可用；默认关闭不应成为不打包实现的理由。发布审批必须单独检查目标满足情况，不只版本、CI 和签名。
- **最小回归**：标准 Windows 配置显式编入 Host、两份同源生产 runtime 进入 dist；无登记/缺省授权零连接；非 Windows 明确拒绝不支持的 Host 能力。真实设备登记、可信 HTTPS/COOP/COEP、Relay 可达性、普通安装包 WebView 和 AGY 专用身份认证/恢复仍须各自验收。
- **证据**：main `9ac38fd` 的 Windows beforeBuild/标准发布命令与独立试用脚本；本轮 `desktopRemotePackage.node.mjs` 在缺少 feature 时的实际 RED。本条不宣称打包修复已经通过真实手机验收，也不将 60 秒试用等同长期无人值守。

## 维护方式

- 新问题先查现有条目，优先修正而非叠加重复教条。只记可复用原因和最小防线，不复制整份日志、凭据或用户数据。
- 每条保留：现象/已证原因、适用模块与版本、禁止的捷径、回归方法、可追溯提交/文档、未证边界。根因未知写“待证”，不得强行归因。
- 修复和有效反例出现后更新证据；后续推翻时保留更正与替代依据，不能把历史失败改写为从未发生。
- 本页引用其他分支的提交是历史证据，不是它们已进入 main、0.27 或已发布的声明。可用 `git show <commit>:<path>` 核查；提交不可得时停止沿用其结论。
- 完成任务时说明用到/新增哪些条目及未覆盖风险。纯文档更新只检查内容、引用和入口，不运行无关全库。

### 新条目模板

```text
Lxx 标题
状态/适用：已证事实或待证；模块、版本/平台
现象与原因：仅记录证据支持部分
防再犯：禁止捷径与正确步骤
最小回归：可观察失败/通过及边界
证据：提交、测试或文档；仍未验证什么
```

## L11 平台合并配置与真实 IPC 覆盖必须先核实

- **已证事实**：排查 `79a9e445` 的公开 scope 读取失败时，仅查 `tauri.conf.json`、Cargo 默认 features 和工作流命令，曾误判标准 Windows 包未启用 Host。`tauri.windows.conf.json` 自动合并的 `build.features=["remote-host"]` 已由 PR #99 接入，原判断已撤回；当前用户实例的具体 IPC 错误仍待取得。
- **防再犯**：先核平台配置、显式覆盖和实际产物来源，再判断最终功能集合。UI 的泛化失败不能推断具体错误码，也不能通过批准、放宽 canonical 或伪造 scope 绕过拒绝。
- **覆盖边界**：原 HostUI worker 未注册 scope 命令，组件单测 mock API，直接调用 `read_pairing_scope` 的资源测试不经过 WebView IPC。新增隔离原资源 IPC 观测只说明测试进程，不能冒充当前用户实例的根因或真实 Codex 验收。
- **最小回归**：组件仅展示固定 SafeError 白名单，不展示任意异常消息，不自动重试；真实 WebView 经原 document bridge、正式命令和 NativeRuntime 读取 scope，保留实际结果及有界退出收据。
## L12 同版 CLI 探针先核当前参数，再区分诊断完成与产品通过

- **已证事实**：Codex 0.169.1 空 home 探针沿用了旧 `--ask-for-approval untrusted`，CLI 早退；测试执行成功不代表 canonical 准入通过。按该精确可执行文件 `--help` 改为 `on-request` 后，真实 Native 链才复现 `NATIVE_CANONICAL_UNAVAILABLE`，控制类别观测定位到未建模的终端模式。
- **防再犯**：运行前核精确版本、哈希与当前参数；保留早退证据，不能把 fixture 参数错误记作产品 RED。无输入、空账户与 read-only sandbox 也不等于无网络。
- **最小回归**：分别报告进程存活、scope 实际值、受拒控制类别和回收见证；禁止采任务正文。新增终端模式必须与成熟模拟器状态、epoch、投影和输入隔离一致，不能仅放行序列或用逐项编译掩盖整体能力缺口。

### L11: Android versionCode 跨可复用工作流按文本传递

- **现象**：RC4 的桌面三端构建和发布预检通过，但 GitHub 未创建 Android 签名任务。运行注释显示调用方在求值可复用工作流输入时拒绝了实际的 28004。此时没有待审记录，也未读取签名材料。
- **防再犯**：手动 Release 和 Android 的两个入口统一用字符串传递；原发行版号检查仍限制为无前导零的正整数及既有上限。不要因父流程已经把值放入环境变量，就假定可复用工作流输入也已通过。
- **证据**：[RC4 Release 运行注释](https://github.com/shawnwu2022/cc-desk/actions/runs/45633115193)；本地最小回归为 tests/scripts/releaseCombinedMobile.node.mjs，真正启动签名任务仍由新运行确认。


### RC4: pass protected environment secrets by name into reusable workflows

The desktop Release caller omitted named forwarding, so the Android signer received empty inputs even though all three secrets existed before dispatch. Keep the values only in the protected signing environment, declare each named input in the reusable workflow, and pass exactly those three names from the caller. Do not inherit unrelated repository secrets. Required inputs check the mapping; the signer still verifies values and certificate identity.

## Android 验签范围要覆盖两种签名

- RC4 第二次运行已成功签名并匹配固定证书，但 apksigner 以 API 31 为验证下限时，有 v3 就不检查 v2。Build Tools 35.0.0 的一次性本机签名夹具复现：API 31 仅 v3；同一 APK 用 API 27 验证，两者均通过。
- 构建方和发布方都以 API 27 为验签下限，并继续要求 v2、v3、单签名者和固定证书；APK 的安装下限和签名配置仍为 API 31。
- 证据：[失败任务](https://github.com/shawnwu2022/cc-desk/actions/runs/45738804191/job/139642625304)；回归：node --test tests/scripts/releaseCombinedMobile.node.mjs。

## 合并发布的产物清单要跟生产路径一致

- RC4 的桌面三端与 Android 已签名，但发布脚本仍只接受三个桌面产物，导致发布前复核拒绝同运行的第四个产物。证据：[发布任务日志](https://github.com/shawnwu2022/cc-desk/actions/runs/45746545884/job/139675056092)。
- 回归用发布命令实际核对完整清单：桌面版严格三个；合并测试版严格三个桌面加一个 Android，其名称、产物 ID、提交、运行和尝试次数必须匹配。不得过滤掉未知产物来凑数量。产物清单校验仍不能替代下载后的真实验签。

## 发布说明缺失应在构建前拦截

- RC4 已签名且通过两次产物复核，却因 `docs/release-notes.json` 缺少当前版本而在最后写说明时停下。`CHANGELOG.md` 有同版本不能替代实际发布入口的说明源。
- 在普通 CI 和发布构建前核对当前精确版本的发布说明；正式发布继续重新读取同提交的说明并保留实际技术披露。证据：[失败的发布任务](https://github.com/shawnwu2022/cc-desk/actions/runs/45753101953/job/139691747812)。

## APK 构建证明必须绑定跨平台一致的锁文件字节

- RC4 的 Windows build proof 哈希匹配 CRLF checkout；Linux 发布检查读取同一提交的 LF blob，拒绝发布。精确运行：<https://github.com/shawnwu2022/cc-desk/actions/runs/45759279625/job/139726348749>。
- 固定 Android 原生 Cargo.lock 的 Git checkout 字节，不在校验时归一化、不接受两套摘要。跨平台回归实际调用 Git，分别核对 autocrlf=true/false 得到的字节与原始 blob。
