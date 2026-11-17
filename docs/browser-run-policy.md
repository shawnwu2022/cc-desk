# Browser authenticated Host run policy

已注册 Browser 显式恢复时，可以从本次已认证 Host 的 canonical GrantRequest 获取新 SessionScope；这不是把旧授权延续至另一个 run，也不是通配授权。

## 两种互斥配置

- 原 exact 模式必须提供 `scope`，不提供 `scopePolicy`。原 `ClientGrantTrust` 和 `grant_client` API、scope 精确比较、Native 的既有 reuse 行为保持不变。
- 新 resume 模式只提供 `scopePolicy: "authenticated-host-run"`，不得同时提供 `scope`。缺两者、混合模式、null、未知策略均拒绝，不推断或默认恢复。
- issuer、principal、role、已注册 BrowserProfile 和全部 budget/enrollment 校验两种模式都必须通过。

## 权限边界

新的 `BrowserResumeGrantTrust { issuer, principal, role }` 和 `grant_browser_resume` 只允许提供 BrowserProfile 的 signer。API 不接收旧 ClientGrantState，而是在内部建立 fresh state；每次都是新的 Request nonce、Host challenge、WebAuthn approval 和 TLS binding，不能复用旧 COSE 或旧 run 的能力。

沿用现有 TLS/RPK/AppBind：固定 Host RPK，校验端点、key epoch、origin 和 exporter MAC。HostOffer 的 activation、incarnation、policy epoch、issuer epoch 是此认证通道中的本次动态 context，不宣称这些值由 Browser 事先固定。GrantRequest 的 context 必须与该次 AppBind 结果完全一致；nonce、binding、principal、role、canonical encoding 与 expiry 约束仍照常校验。

唯一变化是 resume 不要求 Browser 预先持有新的 SessionScope；Host 本次原始 canonical payload 的 scope 被完整保留到 VerifiedCanonicalGrant、presentation、COSE TBS 与 WebAuthn challenge。Window 必须展示完整内容，并经用户明确批准后才能调用 grant 签名及 WebAuthn。此策略不会新增 ALPN、信任根、通配 scope、Native signer fallback 或 Host 端豁免。Host `verify_management` 仍精确校验当前 admission 的 scope、role、duration 和已发出的 challenge。

原有绝对 deadline、取消、单次 management exchange、签名验证、Accepted nonce/binding/grantId/digest 校验和 install_remote 顺序不变。失败不会发布成功回执或可用 data authority；旧 callback、旧 proof 或重复进入不能恢复已关闭 owner。

## 验证范围

Rust 回归使用真实 TLS/RPK/AppBind 与公开 TEST_ONLY 密钥，覆盖新 scope 批准、exact/native 拒绝、fresh approval、错误身份/context/nonce/binding、deny/timeout/replay/reentry，以及 wire 严格互斥。它们不等于真实 WASM/Window 组合验证；真实浏览器组合、presentation、晚到 callback 和持久化恢复的证据须由 Browser 测试提供。测试不注册或使用用户真实密钥，不部署或发布。
