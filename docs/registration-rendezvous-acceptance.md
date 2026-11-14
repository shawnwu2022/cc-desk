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
