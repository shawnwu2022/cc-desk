# 注册会合链路的实际 Relay 验收

## 范围与来源

本验收直接运行 `cc-desk-relay` 的实际服务端二进制，不用 mock 服务端或测试 oracle 替换 admission、会合或转发。服务端候选位于同级仓库的 `.audit/task-worktrees/rendezvous-20261113`，提交 `6382ffe1fbde46ff9bb5586fb25d7c92e5617f2d`，构建前后均无源码改动。这是本地实验候选，不是已部署服务。

- 客户端基线：`8258e21c4d02d8a446e2488cbe1b7f13deec550f`。
- 生产浏览器入口：`createBrowserRegistrationRelay`，本次未修改生产代码。
- 新验收：`remote/ui-terminal/src/browserRegistrationRendezvous.integration.ts`。
- 服务端以 Cargo `--offline --locked` 构建，产物、策略、日志和 TEMP/TMP/TMPDIR 都在本工作树 `.audit/registration-rendezvous/`。
- 二进制：`.audit/registration-rendezvous/target/debug/cc-desk-relay.exe`。
- 二进制 SHA-256：`c8bc5f6d50294af6a91b2eb356ecf1b8c6afbe575af3d42854d9130ff9e15e48`。
- 只使用仓库公开 TEST_ONLY 证书与合成 P-256 身份；校验 CA 和 `foobar.com` 服务名，实际连接仅指向 loopback。没有关闭 TLS 校验、写入真实密钥、部署或 push。

主 sibling checkout `7026049026d0ee42bb6d5e8b626c741506184d1b` 只实现旧模式，不支持本会合子协议。不能把它的拒绝解释为客户端回归，也不能为了通过而降级。这里使用与 [RENDEZVOUS-V1](../remote/RENDEZVOUS-V1.md) 相同的明确候选，不改动旧验收的 executable。

## 实际结果（2026-11-14）

新增 5 项全部通过，44.695 秒，没有 skipped/cancelled：

1. `RegistrationRendezvous_Keepalive_001`：Phone 先完成 Accepted/Join；在 Host 缺席时未放行数据、未发空帧。双方精确协商 `cc-desk-relay-rendezvous-v1`，同一 tunnel 的 Ready 后才放行。空闲 35 秒后仍能双向传输；Host 收到 3 个来自生产注册适配器的空帧。原 40 秒总终点在 40003.9ms 关闭，没有因为 Ready、心跳或有效数据重置；整个过程只有原 2 条 socket。
2. `RegistrationRendezvous_SetupExpiry_002`：无 Host 时，原 1 秒 setup 终点拒绝 ready 并物理关闭；只发 proof 和 Join，不发应用数据或心跳，不重连。
3. `RegistrationRendezvous_TotalExpiry_003`：原注册终点 700ms 早于 setup 时，按原注册终点关闭；Accepted/Join 不延长它。
4. `RegistrationRendezvous_NoDowngrade_004`：实际 Relay 的旧握手给出 Challenge，但无子协议回显；新模式适配器在签名前拒绝，零签名、零发送、零重连。
5. `RegistrationRendezvous_BadProtocol_005`：实际 Relay 拒绝不支持的子协议；零签名、零 Join、零第二连接。

独立严格 TypeScript 检查通过。新文件用受约束的 `createRequire` 定义实际 ws 调用边界，没有安装依赖、引入 any、抑制类型错误或伪造服务端。既有注册单测 9 项通过；既有实际 Relay Phone-first/Host-first 测试另跑 2 项通过。

首跑有 2 项失败，原因是新测试观察器把 ws 的 ArrayBuffer 当作 Buffer 复制，读出空观测。仅修正测试观察器后重跑全部 5 项通过；首跑日志保留为 `acceptance-first.*.log`，不作为生产故障或已修生产 bug 的证据。

## 重跑

从该 cc-box 工作树根目录执行 PowerShell。`CC_DESK_TEST_RELAY_ROOT` 必须明确指向上述会合候选；不要复用另一任务的 target。

```powershell
$audit = Join-Path (Get-Location).Path '.audit/registration-rendezvous'
New-Item -ItemType Directory -Force -Path $audit, "$audit/temp", "$audit/target" | Out-Null
$env:TEMP = "$audit/temp"
$env:TMP = $env:TEMP
$env:TMPDIR = $env:TEMP
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_BUILD_JOBS = '2'
$env:CARGO_TARGET_DIR = "$audit/target"
$env:CC_DESK_TEST_RELAY_ROOT = 'E:/source/github/cc-desk-relay/.audit/task-worktrees/rendezvous-20261113'
cargo build --offline --locked --manifest-path "$env:CC_DESK_TEST_RELAY_ROOT/Cargo.toml" --bin cc-desk-relay --message-format=json
$env:CC_DESK_TEST_RELAY_EXE = "$audit/target/debug/cc-desk-relay.exe"
$env:CC_DESK_TEST_ARTIFACT_ROOT = $audit
node --experimental-strip-types --test remote/ui-terminal/src/browserRegistrationRendezvous.integration.ts
node --experimental-strip-types --test --test-name-pattern=BrowserRendezvous_ActualRelay remote/ui-terminal/src/browserRendezvous.integration.ts
node --experimental-strip-types --test remote/ui-terminal/src/browserRegistrationRelay.test.ts
node node_modules/typescript/bin/tsc --noEmit --strict --target ES2022 --module esnext --moduleResolution bundler --allowImportingTsExtensions --lib ES2022,DOM --types node --skipLibCheck remote/ui-terminal/src/browserRegistrationRendezvous.integration.ts
```

完整本次记录见 `.audit/registration-rendezvous/completion.json`、`build.stdout.jsonl`、`build.stderr.log`、`acceptance.stdout.log`、`typescript.log`、`registration-unit.stdout.log` 和 `existing-rendezvous.stdout.log`。各策略实例退出后保留 Relay stdout/stderr 和正向用例结果，策略文件移除触发服务端有界退出。

## 证据边界

这是生产浏览器注册适配器、Node WebSocket 和实际 Relay 的真实 WSS 会合/空帧/截止时间组合验收。测试 Host 是公开签名材料的直接协议端点，不是原生 Host 启动 UI。它不替代 Edge/WASM 内层 TLS、原生 `start_admitted_registration`、完整 Host supplier 或真实手机/WebView 资格验收。

生产 120 秒总上限未修改；本次实际时钟用较短的 40 秒终点验证不续期，不能宣称已实际等待完整 120 秒。Ready 也不是端到端身份认证；原 endpoint pin、AppBind、BrowserProfile、grant、输入授权与 ACK 的责任没有变更。没有改既有集成测试、旧 Relay binary 或任何生产协议。

## Edge / production registration / native startup 组合验收（2026-11-14）

以下是独立于前文 Node 5 项的新证据，不能把两组范围合并为全部场景均已覆盖会合模式。客户端基线为 `2f2d99cc2678efc7ad459cff85755372277945e5`；最终代码与产物哈希、构建 JSON 和每个场景证据保存在 `.audit/registration-rendezvous-edge/completion.json`。

### 实际覆盖

- `RegistrationEntry_ActualRelayNative_rendezvous-v1_*_003` **8/8 通过，38.8 秒**。运行 Edge 147.0.3912.98、生产 registration source/entry、真实专用 Worker、正式 registration WASM、实际会合 Relay。install / wrong-pin 走原生 903；start-yes / start-deny / start-cancel / start-remember / start-remember-deny / start-remember-cancel 六项走原生生产 startup owner 904。
- 同一最终原生 executable 上，原 **legacy 29/29 通过，3.3 分钟**。包括既有 8 项生产入口、9 项通道生命周期和12项持久安装/重载/撤销场景；这21项较底层场景没有宣称通过会合模式。
- NativeRegistrationStart **13/13**、HostApproval **11/11**、HostCandidate **5/5**、NativePairing **15/15**，合计44项。另行运行 transport registration **8/8** 与 owned driver **1/1**。没有跳过这几组的用例；`BrowserInstallation` 名称过滤未发现用例，不计入执行数。
- strict native library/tests Clippy、项目 typecheck、单独的 Playwright fixture strict TypeScript、原生改动文件 rustfmt 和 diff 检查通过。不是全部2008项原生测试或全部前端测试的执行声明。

新模式核对两端独立显式 mode、每个浏览器仅原1条 socket、精确请求与实际协商的 `cc-desk-relay-rendezvous-v1`，没有降级或第二连接。审批后配置重载与原生 Remember 记录保留原模式。wrong-pin 在 WebAuthn create/assert 之前拒绝；No/取消不产生可用浏览器记录；Remember No/取消不写原生记录。持久成功后，页面重载只使用原 non-exportable handles，不再次导入私钥。原 Host 槽与控制授权仍 Locked。

903 在任何新 credential 前冻结独立 plan，仅会合模式在 Host connect 前发布它以避免双方等待；legacy 仍在 Host admission / driver 就绪后发布，保留 Host-first 约束。904 的原启动审批时序未被测试改写。

### 本次发现并修复的真实尾发送竞态

首个 legacy 回归矩阵为28通过/1失败：start-yes 的原生端已收到 ACK 并报告 InstalledLocked，浏览器却在原 TLS 关闭阶段失败，未落库。使用基线原始 fixture（原 forwarder）和同一组 native/WASM/Relay 字节做5次对照，4通过/1失败；当前 fixture 对照5/5通过。全部历史结果保留，不能以最后一次重跑代替根因证据。

根因是 startup 的 `deliver()` 仅确认客户端 ACK/close_notify，并将服务端 close_notify 写入原 duplex；成功路径随即取消原 channel、关闭原 tunnel，可能在最后一次实际 WS send 完成之前抢先退出。原 transport 的 send receipt 能阻止 producer EOF 抢先退休，但不能抵消显式取消。

确定性 RED 使用 oneshot 持有原 WebSocket 底层 AsyncWrite，确认尾发送已进入但未释放；等价原 completion 路径的008/010两项均提前 Ready 而失败。修复后：

1. 成功路径在原 checked work 内借用并等待原 tunnel，保留20ms document/run检查及原取消/截止时间。
2. 只有原 producer 在全部原 WS send receipt 完成后读到 EOF，才发布 registration-specific tail-drained 事实。`OwnedTunnel.wait_closed()` 仍只表示任务已 join，不更改全局含义。
3. 原 tail-drained 事实和原 identity/stopped/end/signing-token 检查均成立后，才发布 InstalledLocked。BrokenPipe、取消、期限或失效继续保留 DeliveryUnconfirmed 及原 receipt；失败回收仍立即取消并 join 原资源。

008–013六项固定覆盖持有尾发送、失败取消、原取消/绝对终点、drain后原owner检查、等待中断后仍能join原owner，以及底层BrokenPipe不能冒充交付成功。没有新增浏览器存储 ACK、重连或协议变更。InstalledLocked 不代表浏览器已落库。

### 精确产物与限制

- 最终 native executable：`target-browser-profile/debug/deps/cc_desk-0bd025bf142cef03.exe`，SHA-256 `d0bfca61b2607f34cf1b6b2b324149758c1a14fdca9dd35235bfcc1dedf3c94a`，由本次 Cargo `--offline --locked --features remote-host --lib --no-run --message-format=json` 输出选择。旧 `4c942b…` 和中间 `dac711…` 均不充当最终修复证据。
- 会合 Relay：前述 `6382ffe…`、二进制 `c8bc5f6d…`；legacy 回归另用 sibling 主树 `7026049…` 对应 `c5ee65fe7606ae1d4c28c2c8d9e3bcdfe785f3b06d49c3c0b4ef08f2cbf543c5`，没有混作会合服务。
- 正式 registration WASM：`public/remote-registration-runtime/fixture_bg.wasm`，SHA-256 `723e187c9abcdfbf8dd797e92e7fe107956bd6c040a4f4cf813521dccb44eda5`；glue `2631ef5e2536935d9dc797ba410edb358e63edbf70c2204da5f7d35827e1859d`。registration 的48个源码、2个输入以及 terminal runtime 的55个源码、3个输入均按清单逐个核对，产物和 exports/imports 无变化且无 TEST_ONLY。transport 不在两份 WASM 的目标依赖中，本次修复不要求重建这些字节。
- 外层 WSS 使用 Playwright routeWebSocket 与 Node ws。Playwright 会将请求的首个子协议暴露给浏览器，不能据此证明 Edge 实际握手协商；forwarder 原样转交请求协议，并在转发任何二进制消息之前，单独验证 Node ws 实际 `.protocol`、fixture CA、`foobar.com` 和 TLS1.3。最终37个场景均无 forwarderErrors。forwarder 不解析、改写或签名 admission/TLS 帧。
- 内层原 RPK endpoint pin/TLS、Worker 和生产 WASM 实际执行；凭据与原生批准仍是 TEST_ONLY 合成资料。没有浏览器 TLS 绕过、真实凭据配置、安全设置变更、部署或 push。这不替代 Edge WebPKI/子协议握手资格、真实手机/WebView、真实 authenticator、原生 GUI、完整 Host terminal/controller 或公网验证。

### 本地重跑

先在项目内设置 TEMP/TMP/TMPDIR 和 Cargo target，执行 `node scripts/prepare-conpty.mjs`，用 stable 1.95 的上述原生构建命令从 JSON 选择实际 executable。固定 `CC_DESK_TEST_ONLY_NATIVE_REGISTRATION_ORACLE` 为该 executable，`CC_DESK_TEST_ONLY_REGISTRATION_WASM/GLUE` 为上述正式产物。

- 新8项：`CC_DESK_TEST_ONLY_REGISTRATION_RELAY_MODE=rendezvous-v1`，Relay root/exe 固定为前述会合候选；执行 `node node_modules/@playwright/test/cli.js test -c playwright.remote-browser.config.ts registration-relay.spec.ts --grep RegistrationEntry_`。
- 旧29项：mode 明确改为 `legacy`，Relay root/exe 改为前述旧候选；同一命令去掉 `--grep`。
- `CC_DESK_TEST_ONLY_BROWSER_RESULTS` 必须指向项目内的新结果目录。所有日志、测试profile和生成数据仍在项目内；可复用 `.audit/registration-rendezvous-edge/run.ps1` 的完整本地参数与产物选择。
