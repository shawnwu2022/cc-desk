# Native Host 未授权配置候选

本片演进原无 JS 配置入口的合同：前端只提交未授权公开候选，后端严格验证并冻结；原生批准和原 document/run admission 才赋予一次启动权限。候选安装不打开 key、不联网、不读文件、不持久化。

候选文本限16KiB，外层请求限24KiB；两层均拒绝未知/重复字段。现有 key 名仅指固定 Microsoft Software KSP/current-user 的已有对象，不是路径，不允许私钥、secret、句柄或批准布尔值。scope 来自实际 run，clock 使用 native 单调计时。配置有效期60秒，查看并控制当前会话；setup8秒、IO2秒，无自动重连。界面和原生批准显示同一被冻结的引用、公开指纹、目标和期限。

用户显式完成冻结、原生批准和启动三个步骤。UI 捕获精确 NativeAttempt，切换/关闭丢弃晚回执，不自动重发，不把 Registered 当已连接。真实用户凭据和真实平台 GUI 验收另行记录。

## Candidate schema and cancellation
The raw nested JSON is parsed directly, not through a JavaScript object that loses duplicate fields. All objects reject unknown and duplicate fields. The outer request contains only `runId`, original u32 `generation`, and `candidateJson`. The generation is nonzero and ownership comes from the authenticated document and owned run.

Candidate v1 fields: `version`, `address` (SocketAddr), canonical `origin` (wss), `relayMode` (`legacyV1` or `rendezvousV1`), lowercase-hex `tunnel` and `relayId` (16 bytes), `profileId` (16 bytes), canonical nonzero decimal-string `profileRevision`, `endpoint` and `relay` objects (`keyName`, 91-byte public DER `spki`), `phoneSpki`, `issuerSpki`, 32-byte `principal` and `activation`, decimal-string `hostKeyEpoch`, `clientKeyEpoch`, `policyEpoch`, `issuerEpoch`, `incarnation`, and `browser` (`origin`, `rpId`, bounded public `credentialId`, `spki`). Browser backup flags are fixed false. No private key or arbitrary provider/path is accepted.

Successful configuration returns a native monotonic candidate receipt. This is correlation, never authorization. A strict discard command reauthenticates the original document and run, checks owner plus exact receipt under the same controller lock, takes only an unrequested candidate, and drops it outside the lock. An old receipt cannot remove a later candidate. Duplicate discard is harmless. Once an approval request/slot exists, discard cannot reset it or replenish the one-shot ticket. A user can discard an unrequested candidate and submit a new one, which starts a new candidate cap without renewing any approval.

Closing before configuration returns waits only for that existing response, then discards its specific receipt once. It does not issue a broad cancellation or retry an unknown result. Once requesting has begun, close uses the original approval cancellation path. Unknown cleanup failures are shown without echoing input.

Validation scope: synthetic public inputs and isolated run fixtures only. No actual named key was opened or created, no credential was persisted, no deployment occurred, and an actual native approval window has not been clicked in this slice.
## Public scope projection

The independent explicit read described in [native-host-pairing-scope.md](native-host-pairing-scope.md) supplies only the original native session/run/generation for Browser configuration. It does not install this candidate, enroll a peer, approve a connection or renew the candidate cap.
