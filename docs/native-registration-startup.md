# 原生首次登记启动

`remote-host` 的 Windows 构建在原 `RemoteHostPanel` 增加首次登记入口。它复用 authenticated document bridge，公开命令只有 request/current/cancel。request 的外层仍为严格的 `runId`、`generation`、`candidateJson`；没有前端批准字段、原始 key 句柄或 credential 导入接口。默认构建返回固定 unavailable。

## 两次确认与独立信任来源

首次原生 Yes 明确批准一次登记连接、两个现有 Microsoft Software KSP/current-user/silent-open-only key 引用，以及精确 Relay/浏览器公开材料。它只允许一次 Relay 签名和一次端点 TLS 签名。候选解析、状态读取、No、取消和过期均不会自动打开密钥或建立连接。

Host `NativeHostProfile` 和浏览器 `BrowserProfile` 是不同类型。启动登记时没有 Browser credential，也不伪造 BrowserProfile。完成原登记验证器的 create/assertion 两阶段后，才使用原证据冻结 run-local Host 候选，并显示既有 BrowserInstallation 第二次原生确认。第二次 Yes 仍只安装 Locked 候选并通过原受认证通道授权浏览器存储。Host 控制必须再次经既有 Host 批准和显式启动。

浏览器必须独立获得并批准 Host pin 和安装计划。此输入不能为浏览器制造独立 pin 来源；原生 UI 也不会把候选自动发送到浏览器。公开 userHandle 是请求关联，不是已认证的账户或人员身份。

## 严格公开请求

`candidateJson` 最大16KiB，拒绝未知字段、重复字段、不规范整数与十六进制。字段如下：

- `version: 1`；`address` 为明确 IP/socket address；`origin` 为精确规范 WSS 地址；`relayMode` 为 `legacyV1` 或 `rendezvousV1`，不会回退。
- `tunnel`、`relayId`、`profileId` 为16字节小写十六进制；`profileRevision` 为正十进制字符串。
- `endpoint` 和 `relay` 各含 `keyName` 与91字节小写十六进制 `spki`。两个已有 key 引用和公钥必须不同；不生成或发现密钥。
- `principal` 和 `activation` 为32字节小写十六进制；`hostKeyEpoch`、`clientKeyEpoch`、`policyEpoch`、`issuerEpoch`、`incarnation` 为正十进制字符串。
- `browser` 含 `origin`、`rpId`、`userHandle`、`tlsSpki`、`grantSpki`、`relaySpki`、`relayId`、`databaseName`、`expectedRevision`、`wasmUrl`、`glueUrl`。origin 必须为规范 HTTPS origin，RP 必须匹配其主机或父域；artifact URL 必须同源。userHandle 最大64字节，SPKI均为91字节小写十六进制。`expectedRevision` 为 null 或明确的已撤销存储修订字符串。

不得包含 credentialId、credentialSpki、私钥、密码、令牌、scope、clock 或 approved。会话 scope 始终由原 run admission 提供。浏览器原验证器继续负责凭据、签名、不同用途公钥、RP 与 WebAuthn 细节验证。

## 生命周期与失败语义

原120秒总截止从解析开始，确认、打开 key、等待 Relay 与浏览器均不续期。Relay/TLS 共用实际8秒 setup 截止；签名 ticket 同时钳到该 setup 截止。只有 Relay Accepted 后才 arm endpoint。

登记有独立 custody 和原 task/key/tunnel 所有权，不消费永久 Host activation/source 单槽。应用关闭使用同一 exit gate，同时等待原登记任务、原 key 线程与原 transport driver 真实退休。取消与最后 native install 共用同步提交门；取消已经返回后，迟到的第二次 Yes 不能再提交。原 channel cancellation token 同时撤销。

旧 document 撤销后，新的已鉴权 request 可以回收已结束的原任务句柄，再重新检查鉴权与发布锁。未知退休不能回收为可用容量。取消 join 观察者不会移走原 JoinHandle。

`InstalledLocked` 是已安装且已确认交付的历史状态，不是 Host 授权。`DeliveryUnconfirmed` 保留真实安装 receipt，不能重放已消费的决定。`RetirementUnknown` 立即报告不确定退休并保留可能已有的 receipt，同时继续保管尚未退休的原 owner，不以超时或错误当作成功退出。

## 验证范围

`NativeRegistration_StartOracle_904` 使用同一生产 startup owner，覆盖实际 Relay/BrowserWasm 登记路径。仅原生 dialog 结果、匿名 TEST_ONLY key supplier 和固定测试 WSS 信任入口被替换。`start-yes` 验证安装和原资源退休，`start-deny` 要求零开 key，`start-cancel` 撤销原 document，等待原 key/task 退休，并验证新 document 能回收旧任务后重新登记。它不是实际 GUI 或真实用户凭据验收。

本机“记住公开配对记录”有独立批准与存储模块，但此面板本轮尚未接 Remember 控件。记录的重启投影保持 Locked，不恢复 run scope、网络、key 使用或 Host 控制。尚未进行真实持久用户凭据配置、真实原生 Yes 点击、部署或完整重启远控验收。
