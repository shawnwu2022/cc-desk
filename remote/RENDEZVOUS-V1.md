# 显式会合客户端（实验性）

本分支对接 `cc-desk-relay` 的 `6382ffe` 会合协议。生产部署、真实凭据配置和手机/WebView 资格验证不在本次范围。

## 选择模式

- Rust `connect_admitted` 与旧 loopback fixture API 保持 v1 语义。新调用显式传 `RelayMode::RendezvousV1` 与非零 tunnel 到 `connect_admitted_with_mode`。`NativeProfile.relay_mode` 必须由批准来源提供。
- Host `HostApprovalDescription.relay_mode` 与 route/tunnel 一起冻结，批准文本显示模式。注册后的 `HostSourceDescription` 保存它；补充签名材料不能修改模式。
- 浏览器批准输入可添加 `relay.mode: 'rendezvous-v1'`。省略或显式 `'legacy'` 保留 v1。模式随批准副本冻结，未知值拒绝。浏览器来源仅在新模式构造带子协议的 WebSocket。
- 新模式必须收到精确 `cc-desk-relay-rendezvous-v1` 子协议回显。缺失或不同回显失败，不签 relay proof，不自动切回 v1、不自动重连。两端必须独立明确选择相同模式。

## 时序与边界

1. 外层 TLS/WebSocket 和原 Challenge/Proof/Accepted 不变；Accepted 只证明本端登记。
2. 新模式发送 `[2, 0] + tunnel[16]` 的 Join。
3. 只接受同一 socket 上精确 `[2, 1] + tunnel[16]` 的 Ready，之后才允许 Endpoint arm、内层 TLS 和 mux。等待中不缓存应用数据，提前外层数据或其他控制帧失败。
4. Join/Ready 消耗原 setup 绝对截止时间，不重置批准 cap 或签名窗口。取消与断开丢弃原 socket；失败不能授予新的会话所有权。
5. Ready 只表示 relay 已匹配配置中的双方，不认证端到端身份。原 endpoint pin、AppBind、BrowserProfile、grant、peer install、ACK 与输入授权仍全部执行。

与旧服务端或单方启用混用时有界失败。旧 v1 两端仍可直接在 Accepted 后启动原数据通道，不改变旧 Accepted 的含义。

## 验收范围

- transport 单测使用公开 TLS/签名向量，核对协议回显、原签名 transcript、Join/Ready 字节、错误 tunnel/帧、断开与原 deadline。
- 浏览器单测覆盖 Accepted 不放行、无降级、错误帧、取消和输入冻结。`npm run test:browser-rendezvous` 是生产浏览器 relay 适配器加 Node WebSocket 与实际 relay 的双顺序外层数据测试，不是完整浏览器 TLS/WebView 验收。
- `RelayRendezvous_*` 原生组合测试使用实际 relay、公开测试身份、原 Host source 注册、内层 RPK/Browser grant、真实 owned Node 输出及 ACK，并检查无对端超时/取消时未创建 mux。
- 所有运行都必须显式设置项目内 TEMP/TMP/TMPDIR、`CARGO_INCREMENTAL=0`，以及经过验证的 relay fixture 根目录和候选 executable。不要覆盖并行任务的 target。

本次本地验收：remote workspace 258 通过（1 项既有 ignored）、浏览器单测 93 通过、实际 relay 浏览器适配器双顺序 2 通过、原生会合 4 通过、旧 v1 原生隔离矩阵 48 项与 Host driver 8 项通过；生产浏览器 TypeScript 和客户端/原生严格 Clippy 通过。未部署，未完成真实手机/WebView 资格验证。
