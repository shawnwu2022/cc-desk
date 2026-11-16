# 原生首次登记启动

`remote-host` 的 Windows 构建在原 `RemoteHostPanel` 增加首次登记入口。它复用 authenticated document bridge，登记命令为 request/current/cancel，公开配对记录另有 list/select/forget/forget_cancel。request 的外层仍为严格的 `runId`、`generation`、`candidateJson`；没有前端批准字段、原始 key 句柄或 credential 导入接口。默认构建返回固定 unavailable。

## 两次确认与独立信任来源

首次原生 Yes 明确批准一次登记连接、两个现有 Microsoft Software KSP/current-user/silent-open-only key 引用，以及精确 Relay/浏览器公开材料。它只允许一次 Relay 签名和一次端点 TLS 签名。候选解析、状态读取、No、取消和过期均不会自动打开密钥或建立连接。

Host `NativeHostProfile` 和浏览器 `BrowserProfile` 是不同类型。启动登记时没有 Browser credential，也不伪造 BrowserProfile。完成原登记验证器的 create/assertion 两阶段后，才使用原证据冻结 run-local Host 候选，并显示既有 BrowserInstallation 第二次原生确认。第二次 Yes 仍只安装 Locked 候选并通过原受认证通道授权浏览器存储。Host 控制必须再次经既有 Host 批准和显式启动。

浏览器必须独立获得并批准 Host pin 和安装计划。此输入不能为浏览器制造独立 pin 来源；原生 UI 也不会把候选自动发送到浏览器。公开 userHandle 是请求关联，不是已认证的账户或人员身份。

## 严格公开请求

`candidateJson` 最大16KiB，拒绝未知字段、重复字段、不规范整数与十六进制。字段如下：

- 可选 `remember: boolean`，默认 false。它只请求在验证后显示独立原生持久化确认，不是批准字段。
- `version: 1`；`address` 为明确 IP/socket address；`origin` 为精确规范 WSS 地址；`relayMode` 为 `legacyV1` 或 `rendezvousV1`，不会回退。
- `tunnel`、`relayId`、`profileId` 为16字节小写十六进制；`profileRevision` 为正十进制字符串。
- `endpoint` 和 `relay` 各含 `keyName` 与91字节小写十六进制 `spki`。两个已有 key 引用和公钥必须不同；不生成或发现密钥。
- `principal` 和 `activation` 为32字节小写十六进制；`hostKeyEpoch`、`clientKeyEpoch`、`policyEpoch`、`issuerEpoch`、`incarnation` 为正十进制字符串。
- `browser` 含 `origin`、`rpId`、`userHandle`、`tlsSpki`、`grantSpki`、`relaySpki`、`relayId`、`databaseName`、`expectedRevision`、`wasmUrl`、`glueUrl`。origin 必须为规范 HTTPS origin，RP 必须匹配其主机或父域；artifact URL 必须同源。userHandle 最大64字节，SPKI均为91字节小写十六进制。`expectedRevision` 为 null 或明确的已撤销存储修订字符串。

产品 Browser Prepare 页面同时展示同一组三个公开 SPKI 的 Base64URL 与小写 hex。填写上述 `browser.tlsSpki/grantSpki/relaySpki` 时使用明确标注的 Native SPKI hex；这三项是完整91字节 SPKI，不是 Relay policy ``public_key_sec1`` 所需的65字节 SEC1 点。Browser 安装 JSON 仍使用 Base64URL。转换只减少人工编码错误，不生成完整候选、不发现 Host key、不证明身份或提供批准。取消或过期后须刷新页面重新准备身份，旧身份的配置不能用于新一轮。

不得包含 credentialId、credentialSpki、私钥、密码、令牌、scope、clock 或 approved。会话 scope 始终由原 run admission 提供。浏览器原验证器继续负责凭据、签名、不同用途公钥、RP 与 WebAuthn 细节验证。

## 生命周期与失败语义

原120秒总截止从解析开始，确认、打开 key、等待 Relay 与浏览器均不续期。Relay/TLS 共用实际8秒 setup 截止；签名 ticket 同时钳到该 setup 截止。只有 Relay Accepted 后才 arm endpoint。

登记有独立 custody 和原 task/key/tunnel 所有权，不消费永久 Host activation/source 单槽。应用关闭使用同一 exit gate，同时等待原登记任务、原 key 线程与原 transport driver 真实退休。取消与最后 native install 共用同步提交门；取消已经返回后，迟到的第二次 Yes 不能再提交。原 channel cancellation token 同时撤销。

旧 document 撤销后，新的已鉴权 request 可以回收已结束的原任务句柄，再重新检查鉴权与发布锁。未知退休不能回收为可用容量。取消 join 观察者不会移走原 JoinHandle。

`InstalledLocked` 是已安装且原登记尾发送已排空的历史状态，不是 Host 授权，也不承诺 Browser 已落库。原 TLS ACK/close_notify 完成后，仍须在原 checked work 内等待原 transport driver 退休，核对 producer 在全部原 WS send receipt 完成后到达 EOF，再核对原 document/run、stopped、signing token 与绝对截止时间；之后才发布该状态。取消或发送失败不得用正常 join 冒充交付成功。`DeliveryUnconfirmed` 保留真实安装 receipt，不能重放已消费的决定。`RetirementUnknown` 立即报告不确定退休并保留可能已有的 receipt，同时继续保管尚未退休的原 owner，不以超时或错误当作成功退出。

## 验证范围

`NativeRegistration_StartOracle_904` 使用同一生产 startup owner，覆盖实际 Relay/BrowserWasm 登记路径。仅原生 dialog 结果、匿名 TEST_ONLY key supplier 和固定测试 WSS 信任入口被替换。`start-yes` 验证安装和原资源退休，`start-deny` 要求零开 key，`start-cancel` 撤销原 document，等待原 key/task 退休，并验证新 document 能回收旧任务后重新登记。它不是实际 GUI 或真实用户凭据验收。

本机“记住公开配对记录”已接入原 RegistrationCustody：原验证证据完成后进入 PendingNativePersistence，独立原生 Yes 才向精确目标提交 schema-2 公开记录，然后用同一原证据进入 BrowserInstallation。No/过期返回 NativeStoreDeclined 并结束登记，不写记录、不安装浏览器；错误/不确定分别为 NativeStoreFailed/NativeStoreUnknown。InstalledLocked/DeliveryUnconfirmed/RetirementUnknown 的 remembered 标志记录真实已完成的写入；后续安装失败/取消为 NativeStoredRegistrationIncomplete，不抹掉已经保存的事实。

启动后的列表只读并保持 Locked，不创建/修复目录或 lease，不打开 key 或网络。显式选择按当前 store CAS 冻结新 run 候选，保留原公钥、backup flags 和 epochs；scope 只取新 admission，activation/incarnation/clock 为新值。Forget 使用独立原生确认和 requestId 精确取消，可在请求入场前建立取消 fence；不撤销已独立运行的连接。详见 native-public-pairing-store.md。

NativeRegistration_StartOracle_904 新增 start-remember/start-remember-deny/start-remember-cancel，仍使用实际 startup owner、匿名 key supplier 和 synthetic native store。成功分支重新打开原文件验证 Locked；No/取消要求无已提交文件。该 oracle 不是实际用户 DPAPI 或原生 GUI 验收。尚未进行真实持久用户凭据配置、真实原生 Yes 点击、部署或完整重启远控验收。
