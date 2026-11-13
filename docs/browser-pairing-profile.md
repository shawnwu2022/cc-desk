# Browser 公开配置与可信供应

## 接口与信任前提

公开 JSON 只是待核对候选，不建立身份、批准或 enrollment。受控同源 `/browser-profile.js` 是既有代码和 key-use 信任入口，它须独立持有已批准的完整公开配置，并取得原先已登记的非导出 TLS/grant handles、relay signer 与 presenter。

- `parseBrowserPairingConfiguration(bytes)`：复用 `wire.parseJson`，限制 16 KiB，拒绝重复/转义重复字段、未知字段、非法 UTF-8 和深嵌套；返回冻结的 `PublicBrowserPairingConfiguration`，不返回 Approved 类型。
- `createApprovedBrowserProfileReader(candidateBytes, provider?)`：同步复制解析候选；唯一一次调用 `provider.readApproved(signal)`，只传原 signal，不传候选、任意 key 名或选择器。
- provider 独立返回 `{ configuration: Uint8Array, handles: { tlsKey, grantKey, relaySign, present } }`，或者无批准记录时返回 `undefined`。configuration 必须来自原先独立批准的完整记录，不能来自候选、scope lookup 或仅通过语法解析的配置。
- provider 返回后立即捕获原 handles、复制解析并冻结配置。两份配置全字段语义匹配后，只从 **provider 的配置** 显式映射与原 handles 合成 `approvedBrowserInputs`，继续执行既有 non-exportable P-256、独立 handles、origin 和 grant 约束。

不得把候选复制为 provider.configuration，不得用候选选择任意 key、替换 pins、自动登记或续签权限。provider 必须保证 pins、credential 与原 handles 已经过独立可信绑定；helper 不通过试签名证明绑定，不接受序列化私钥、key handles、批准布尔值或回调。

## 公开 v1 格式

完整可构建示例见 [browser-profile.ts](../remote/ui-terminal/examples/browser-profile.ts)。所有对象拒绝额外字段，除 relay.mode 外各字段均必填：

- `version: 1`。
- `profile`: `origin`、`rpId`、`credentialId`。origin 为规范 HTTPS origin；RP 为相同 hostname 或其后缀；credentialId 解码后 1–1024 bytes。
- `localSpki`、`hostSpki`：各 91-byte 公共 SPKI。
- `trust`: `hostKeyEpoch`、`clientKeyEpoch`、`origin`。epochs 为非零规范 u64 十进制字符串；origin 必须等于 relay.endpoint。
- `grant`: `issuerSpki`（91 bytes）、`principal`（32-byte 小写 hex）、`scope`（session 为 16-byte 小写 hex，run 为 1–128 字符，generation 为非零规范 u64 字符串）、`role`（Controller/Observer）、`budgetMs`（1–30000 整数）、`peerInstall: { budgetMs }`（1–10000 整数）。
- `grant.profile`: `origin`、`rpId`、`credentialId` 与外层 profile 完全相等，另含 `credentialSpki`（91 bytes）、`backupEligible`、`allowBackedUp` 两个布尔值。
- `relay`: 可选 `mode`（legacy/rendezvous-v1）、`endpoint`（规范 WSS URL）、`tunnel`（16 bytes，非全零）、`keyId`（16 bytes）。省略 mode 不等于显式 legacy，不能隐式降级。
- `artifacts`: `wasm`、`glue` 为同 profile origin 的规范 URL，各最多 2048 字符，无用户名、密码、query 或 fragment。relay URL 最多 512 字符。

除 session/principal 指定 hex 外，字节字段均为无 padding 的规范 base64url。检查长度与格式不证明 SPKI 的密码学真实性或对应私钥存在；真正认证继续由原 TLS/AppBind/grant 路径完成。JSON 字段顺序、格式空白不影响匹配，任何字段值或字段有无的变化都必须匹配独立批准记录。

## 唯一读取与取消

reader 保留同一读取的成功、缺失、失败或取消结果。不重启 pending，不自动重读，不生成或延长 deadline。并发调用共享原读取，但任何调用自己的 signal 已取消均不能取得成功。首读取消后等待原 provider settle，再拒绝所有读取，不能换新 signal 发布迟到成功。错误固定且不回显配置或 provider 异常。

provider 须合作响应原 signal。helper 不通过 race 假装外部工作已经退休，也不证明 provider 的原生或外部工作可被抢占。原 `browserProfileSource.ts` 的每页一次、可见文档、原 setup deadline 和同源受控模块约束继续生效。换 run/generation 后须新的显式页面/连接流程。

## 外部受控模块构建

仓库的 [browser-profile.ts](../remote/ui-terminal/examples/browser-profile.ts) 是永远锁定的构建示例：使用标明 PUBLIC_TEST_ONLY 的合成公开值，provider 总是返回 undefined。其唯一导出为 `readApprovedBrowserInputs`，可直接使用项目已安装 esbuild 构建，**不需要也不更改 Vite 生产入口**：

```powershell
# 从仓库根执行，输出仅保留在项目内审计目录，不部署。
New-Item -ItemType Directory -Force .audit/browser-profile-example | Out-Null
$env:TEMP = (Resolve-Path .audit/browser-profile-example).Path
$env:TMP = $env:TEMP
$env:TMPDIR = $env:TEMP
node_modules/.bin/esbuild.cmd remote/ui-terminal/examples/browser-profile.ts --bundle --platform=browser --format=esm --target=es2022 --outfile=.audit/browser-profile-example/browser-profile.mjs
node --input-type=module -e "const m=await import('./.audit/browser-profile-example/browser-profile.mjs'); console.log(Object.keys(m),await m.readApprovedBrowserInputs(new AbortController().signal))"
```

预期只有一个导出，读取结果为 undefined。示例里的 `.invalid` URLs、合成 SPKI 和 scope 不能用于真实登记；该构建产物不是已部署的 `/browser-profile.js`。实际站点需从真实受控源码导入本 helper，并把示例的锁定 provider 替换为它自己已有的独立可信供应实现，然后在另行授权的部署流程中将审核后的单一 ESM 模块供应在固定同源路径。候选来源和 artifact URLs 也必须由站点按真实受控配置提供，不能直接照搬示例。

## 验证边界

本层不签名、不调用 presenter，不联网、不存储、不导入/导出/生成密钥，不改变连接、WebAuthn 批准、Host cap 或 relay 协议。自动化覆盖完整合法/非法配置、全字段匹配、非导出句柄规则、冻结隔离、取消/唯一读取与锁定示例 bundle。测试只使用现有公开 TEST_ONLY P-256 向量和隔离合成数据。

真实 Edge 隔离回归位于 `tests/remote-browser/browser-pairing-profile.spec.ts`。它在内存中 bundle 真实 helper，以 Playwright route 提供带 COOP/COEP 的 `https://desk.invalid` 页面，仅导入现有公开 TEST_ONLY scalar1/2 为非导出 WebCrypto handles。四条行为测试检查原句柄及冻结快照、完整配置不匹配保持锁定、首读取消保留原 pending、后续调用取消与缺失批准；未批准输入不能因缓存或换 signal 成功。所有其他页面请求被拒绝，未连接 Relay、Host 或账户。

```powershell
# 已安装 Edge；沿用上文项目内 TEMP/TMP/TMPDIR，默认结果仍在项目 test-results 中。
node_modules/.bin/playwright.cmd test --config playwright.remote-browser.config.ts tests/remote-browser/browser-pairing-profile.spec.ts --reporter=line
```

这补充真实 Edge 引擎、WebCrypto handles 与事件循环的行为证据；HTTPS 由测试路由截获，不证明真实 TLS、站点部署或同源模块供应。真实 enrollment、credential/handle 管理与撤销恢复、可信 provider、WebAuthn、Host、网络和实际设备全链验收仍未完成。

## 合成 supplier → Host/WASM 数据回路

`remote/ui-terminal/src/browserData.integration.ts` 现在增加 5 条 supplier 验收。可信 TEST_ONLY 快照由隔离 Host 输出的原生 scope 与固定公开向量独立构造，候选只是另一份待核对输入。只有 `createApprovedBrowserProfileReader` 成功返回，才将其原 handles、pins、trust、grant 与 presenter 交给既有 TLS/grant broker 和 WASM Worker；候选不选择 key，也不会回填供应端记录。

- 匹配路径通过真实编译的 Rust Host、WASM TLS/AppBind/grant、原生 peer 安装和原数据 dispatcher，完成 Snapshot/ACK/Window、InputReceipt、Query、Settle、Cursor，并核对内存 writer 的字节与恰好一次写入。
- session、run、generation、Host SPKI 任一不匹配，都在 broker/Worker 建立前拒绝；核对零 WebCrypto 签名、零 presenter/WebAuthn 调用、零连接，并确认换 signal 不能让已拒绝的读取成功。
- 在项目内隔离副本中删除全字段匹配检查，4 条负例均以缺少预期拒绝失败；生产 helper 未被修改。

复用 [Browser data 验收构建与显式 artifact 输入](../remote/BROWSER-PROFILE.md) 的 Host/WASM 命令，设置 `CC_DESK_TEST_ONLY_DATA_HOST`、`CC_DESK_TEST_ONLY_DATA_WASM`、`CC_DESK_TEST_ONLY_DATA_GLUE` 后，从仓库根运行：

```powershell
# TEMP/TMP/TMPDIR 与构建产物必须指向项目内的隔离审计目录。
node --experimental-strip-types --test --test-name-pattern=PairingSupplier remote/ui-terminal/src/browserData.integration.ts
# 完整 data 回归，包含现有取消、重连、不重放与 fencing 场景。
node --experimental-strip-types --test remote/ui-terminal/src/browserData.integration.ts
```

该验收使用 Node 的 browser-shaped 平台、公开 TEST_ONLY non-exportable handles、合成 WebAuthn、loopback 直连和内存 writer。它不经过 Relay，也不启动真实 PTY/CLI，不代表真实浏览器网络、站点供应、真实 enrollment、真实设备或 GUI 审批全链验收。生产可信供应和真实设备门禁仍保持未完成。
