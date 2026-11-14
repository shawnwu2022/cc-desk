# Browser product / rendezvous 离线组合验收

## 范围与边界

2026-11-14 在 WSK-MSI 的独立项目工作树完成：正式 `dist/browser-profile.js` → 真实 Edge 页面/Worker → 生产 terminal WASM → 实际 rendezvous Relay executable → 既有 `BrowserData_OriginalHostFixture_001` memory Host/data path。只修改 TEST_ONLY 接线；生产 TLS、AppBind、grant、journal、dispatcher、身份与协议均未改。

这不是原生 GUI、手机、真实认证器、公网 WebPKI、真实登记到 Native 重启的整套仪式或部署验收。登记记录与 WebAuthn 使用原公开测试向量，Host approval 仍是原 fixture。产品加载的 provider 不被替换；handoff 套件仍使用其原 stored reader fixture。

## 测试接线

- `CC_DESK_TEST_ONLY_ENTRY_RELAY_MODE` 只接受 `legacy`（默认）或 `rendezvous-v1`，明确写入独立批准的公开 relay configuration。
- rendezvous 必须另外提供 `CC_DESK_TEST_ONLY_RENDEZVOUS_RELAY`。fixture 在项目 `.audit/product-rendezvous/policies/` 内生成独立公开 policy：loopback 监听、`foobar.com` WSS、原 Host/Phone public vectors、原 tunnel 09、允许 origin `https://desk.invalid`、60 秒有效期。policy 文件及 SHA-256 进入每例证据。
- 实际 Relay 用原 launcher `--policy / --tls-chain / --tls-key / --tls-ca` 启动。Host-side oracle 的 external 分支不启动内嵌 Relay，只以 fixture CA、`foobar.com` 和 TLS 1.3 连接实际进程。
- Host 先完成原 challenge/proof/Accepted，再报告 `hostAdmitted` 让浏览器启动；随后发送 Join 并验证精确 Ready tunnel 后才转发原 outer 子帧的字节到不变 memory Host。不能等待 Host Ready 才启动 Browser。
- Browser WSS 路由检查请求的 subprotocol，并原样传给 Node `ws`。另外独立检查真实 socket 的 `protocol`、TLS `authorized`、`TLSv1.3`、`servername=foobar.com`；Playwright `route.protocols()` 和代理 Browser `protocol` 仅表示请求，不算真实协商证据。证书验证保持开启，只信任公开 fixture CA。
- 所有 WSS 回调异常统一 fail-closed、记录并在 teardown 断言 `errors=[]`；记录页面异常。该更严格检查发现原 authenticator 在新页面 `about:blank` 没有 SubtleCrypto 时抛错，现只在 fixture 安全文档注入，没有隐藏错误或削弱错误断言。
- legacy 保留内嵌 Relay 分支及原数据语义，`originalRelayRetired` 仅在原 `connected()==0 / queued_bytes()==0` 断言通过后输出。external 分支只输出 `hostBridgeRetired`。
- 实际 Relay 通过把本次 policy 重命名为 `.retired`（保留证据）触发原 launcher 正常撤销，5 秒内必须输出 `WSS_RETIRED` 且 exit=0/signal=null。该原 launcher 标记在 listener 关闭、全部连接任务 cancel-and-join 成功后才输出。记录 `serviceStopped / connectionTasksJoined`，不把未观测的队列计数声称为零；强杀只用于失败清理，不能使此验收通过。

## 复现

在项目根目录把 `TEMP`、`TMP`、`TMPDIR` 设为项目内 `.audit/product-rendezvous/runtime`，Playwright results 同样指定项目内路径。使用已有锁定 Node 依赖、Edge 与 stable Rust 1.95，不安装工具链。

1. 设置 `CC_DESK_TEST_ONLY_RELAY_ROOT` 为已核验 Relay `6382ffe1fbde46ff9bb5586fb25d7c92e5617f2d` checkout，执行 `node tests/remote-browser/build-entry-relay.mjs`。脚本使用 `cargo build --offline --locked`、jobs=2，oracle target/runtime/log 全在项目内。
2. 设置 `CC_DESK_TEST_ONLY_ORIGINAL_RELAY_ORACLE` 为上述构建的 executable；设置 `CC_DESK_TEST_ONLY_DATA_HOST`、`CC_DESK_TEST_ONLY_DATA_WASM`、`CC_DESK_TEST_ONLY_DATA_GLUE` 为下列已核验产物。使用已核验同源 `dist`，不是重新混编旧源。
3. rendezvous 模式设 `CC_DESK_TEST_ONLY_ENTRY_RELAY_MODE=rendezvous-v1` 和 `CC_DESK_TEST_ONLY_RENDEZVOUS_RELAY`；运行 `node node_modules/@playwright/test/cli.js test -c playwright.remote-browser.config.ts browser-product-entry.spec.ts browser-run-handoff-relay.spec.ts --reporter=line`。
4. 将 mode 改为 `legacy`，再运行完全相同的两个文件。每模式 9 项 product + 9 项 handoff。
5. 执行 `npm run typecheck` 和 `git diff --check`。没有运行全量前端/Rust、真实 CLI/设备 gate，不以本次聚焦矩阵替代它们。

## 实测结果

- RED：旧 fixture 未设批准 relay mode，实际产品安装记录断言预期 `rendezvous-v1`、实得 undefined；原始日志保留。
- 首次 GREEN 尝试：更严格页面异常观察发现上述 TEST_ONLY `about:blank` 问题；保留失败日志，限定注入 origin 后最小产品数据链通过。
- rendezvous：product 9/9 + handoff 9/9。
- legacy：product 9/9 + handoff 9/9。
- 拒绝/取消负例保持原 memory Host fixture 的拒绝语义，其 harness 可因原 `unwrap()` 记录 `Io` panic；这些用例断言没有 installed/dispatched，并不把该 Host 子进程宣称为成功退出。只有正常数据例验证精确 effect。
- 原跨重开 IDB 字节/owner、fresh presence、合成 click 不批准、取消/超时/Forget、错误 pin/principal/epoch/role、旧 scope 拒绝、无安装记录 Locked、精确一次输入写入及 cached reopen 不重放断言均保留。
- oracle offline locked build、全仓 typecheck 及 diff whitespace 检查通过。所有编译/浏览器 profile/临时文件/证据位于项目内；未 push、部署、改安全设置或配置真实凭据。

本轮只证明上述冻结源码和产物组合；集成主分支后的其他修复或新产物应重新执行矩阵，不继承本记录为最终主树认证。

## 产物来源与 SHA-256

源码基线 `2f2d99cc2678efc7ad459cff85755372277945e5`。独立工作树为 `cc-box/.audit/task-worktrees/product-rendezvous-20261114`；原产物来源为相邻 `browser-profile-migrated-20261113` 工作树。最终运行将 Host/WASM/glue/Relay/manifest 复制并冻结于本工作树 `.audit/product-rendezvous/inputs/`，防止并行主树构建改变输入。原 product completion 记录的 `builtAtHead=4910fe32...`、收口 `head=2f2d99cc...`；本次逐文件核对该记录的全部 dist hashes，生产源码 git blob 不变，未把旧编译记录改称为新编译。

| 产物 | SHA-256 |
| --- | --- |
| `dist/browser-profile.js` | `1a0aacb4a7b5dcb9c0a6d367b4d161367fcc73174b6398161d20919572c61a23` |
| 生产 terminal `fixture_bg.wasm` | `11343e12fbbae579599f3b29932df9450df468be5d030fe945c50afa0a44a6c2` |
| 生产 terminal `fixture_bg.js` | `2631ef5e2536935d9dc797ba410edb358e63edbf70c2204da5f7d35827e1859d` |
| 原 memory Host `cc_desk_remote_host-b73379e8a78efdc4.exe` | `14234270d4b31673f4b7c7f7ffb89ee1edd835addf44f21f29e7843005a0e966` |
| 实际 Relay `6382ffe1...` 的 `.audit/registration-rendezvous/target/debug/cc-desk-relay.exe` | `c8bc5f6d50294af6a91b2eb356ecf1b8c6afbe575af3d42854d9130ff9e15e48` |
| 本次 TEST_ONLY Host-side oracle | `6fe42d8fedd08e018a96bbcc72127f4d9c9498181ff22988baf6406db24e7f2e` |

项目内完整记录：`.audit/product-rendezvous/completion.json`，包括所有源文件/产物/日志 SHA、旧 product 与 Relay provenance、两组 18 个 case JSON、每例 policy、真实协商证据和写入 effect。构建 manifest 位于 `.audit/browser-entry-relay-build/artifacts.json`。`red.log`、`green-minimal.log`、`green-minimal-2.log`、`green-rendezvous.log`、`green-legacy.log`（初版）、`frozen-rendezvous.log`、`frozen-legacy.log`（退休证据修正后最终版）和 `typecheck-final.log` 保留全过程；可用同目录 `run.ps1` 与 `finalize-evidence.mjs` 复核本机证据。
