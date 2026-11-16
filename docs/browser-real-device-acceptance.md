# Browser 产品真实设备验收准备

本清单说明隔离开发证据与真实启用的边界，不是部署、创建凭据或授予长期访问的授权。

## 产品路径

1. 可信登记来源独立选定完整 plan、现有三个不同的不可导出签名 handles，以及登记 runtime 地址。公开候选、Native Ready、receipt、URL 参数和成功页面都不能替代这份批准。
2. 原登记 Worker/TLS 验证身份，原生 key-use、可选 Remember 与 Browser installation 继续分别走各自批准。安装必须等待原 outcome、ACK、完整 retirement 和三对公私钥绑定验证。
3. plan 从一开始就选定产品固定库 `CC_DESK_BROWSER_ENROLLMENT_V1`。来源返回后不能重写 databaseName、origin、expectedRevision 或其他批准字段。原通用安装器继续支持受控的其他库，产品启动模块不会发现或自动迁移它们。
4. 新页面加载正式 `/browser-profile.js` 不读库、不签名、不连线。显式 Connect 才读取原已安装记录。缺失、已撤销、未知 owner 或不支持的记录都保持 Locked。
5. 新 run 的 scope 来自同一认证 Host 通道的原 challenge；历史配对记录不授权新 run。原完整 presenter、WebAuthn 和 Native Host Yes 仍必需。新 scope 不写回记录。
6. Disconnect 只关闭当前激活与原 transport，等待原完整 retirement；它不等于 Forget。强制关闭、浏览器崩溃或挂起不能证明异步清理完成，残留 owner 必须继续 fail closed，不能自动清除。

## 实际启用前需明确授权的对象

- 精确 HTTPS 站点、RP、Relay endpoint/mode，以及允许访问的设备、用户关联、Host/browser 公钥、principal、role 与 scope。
- 使用哪些已有 TLS/grant/relay handles 和 native named keys。缺失时不得由脚本悄悄创建替代密钥或新信任根。
- 是否在该 RP 创建真实 WebAuthn credential、指定设备上的持久存储位置、允许的 backup policy 和后续逐次签名用途。
- 是否发布经过审查的站点与构建产物，目的服务器及其访问范围。构建本地文件不等于已授权发布。
- 原生提示中具体的 key-use、Remember 与 installation 内容。程序按钮不能代替用户自己的原生确认。
- Forget 若会不可恢复地删除唯一 key handles，需要该具体删除的适用确认；关闭连接不是同一动作。

本轮开发不执行上述真实操作，也不更改证书信任、浏览器安全开关、防火墙或其他安全设置。

## 构建与来源核对

登记与终端 runtime 是不同的生产链接入口，不能拿 TEST_ONLY oracle 或包含固定测试私钥的模块部署。

- 终端：`public/remote-runtime/fixture_bg.wasm`、`fixture_bg.js`、`manifest.json`。
- 登记：`public/remote-registration-runtime/fixture_bg.wasm`、`fixture_bg.js`、`manifest.json`。
- 产品 provider：Vite 构建的 `dist/browser-profile.js`，唯一导出 `readApprovedBrowserInputs`。配套页面及任何合法 shared chunks 必须来自同次构建。
- 首次登记的 `/browser-registration.js` 仍是独立可信供应入口；产品不会自动生成批准记录或替该来源挑选真实身份。
- [供应适配器与独立构建](browser-registration-source.md)已提供严格公开配置解析、完整独立批准比较、原 handles 保留及唯一导出 bundle。实际可信 provider 仍须单独实现与授权，默认产品构建不会替用户生成此来源；reader 只验 key policy，公私钥匹配仍由原 entry/installer 证明。

验收记录精确 source commit、工具链、每个 runtime/glue 的 SHA256 和部署字节。核验 manifest 的生产源文件、原始库与要求的导出，拒绝 TEST_ONLY 导出。不要把一轮旧 WASM、另一轮前端与最新 Rust 的结果合并成单一最终验收。

## 需要实际运行的设备矩阵

以下各项尚不能由合成 loopback 测试代替：

- 手机/桌面浏览器真实 WebPKI、受信 HTTPS、CSP、COOP/COEP、Worker、Web Locks 和 IndexedDB 能力。证书错误必须按原安全流程处理，不能用忽略证书开关验收。
- 真实 authenticator 的 UP/UV、正确 RP/origin、用户取消、backup flags 与原始挑战绑定；错误 origin/Host pin 必须在形成可用记录前拒绝。
- 原生真实窗口的三类独立确认：拒绝、取消、期限、换 document/run 及迟到回调。测试 callback 成功不证明真正用户批准。
- 原生重启后只读配对记录，选择新的 run 后仍 Locked；重新 Host Yes、Browser presenter 与 WebAuthn 才能连接。
- 显式 Disconnect 等待完成后重新打开页面，不重新导入 private keys、不修改历史配置、不重放输入。另测 tab 硬关闭、挂起/恢复、掉网和进程退出，并记录 Unknown/uncertain，不把超时当作完成。
- 实际公网 Relay rendezvous 的独立认证与保活 deadline；当前 legacy 与 rendezvous-v1 的本地合成组合证据仍不能替代该项，范围见 [注册会合验收](registration-rendezvous-acceptance.md)。
- 若验收终端控制，必须使用明确授权的测试会话，核对一个原始输入对应一次 Host 写入，并保留 Partial/Unknown 的原事实；不能用重试凑成成功。

## 合成证据可以说明什么

真实 Edge、原 Relay、原 Host/WASM、实际 IndexedDB/WebCrypto 能验证模块组合、原数据链、一次写入和生命周期边界；使用的是公开固定测试资料和项目内隔离 browser profile。它不证明真实用户身份、硬件凭据、原生 GUI、用户 DPAPI 状态、真实设备网络、部署安全或完整 native GUI 重启整链。

同源受控代码与 origin-local storage 是当前可信存储边界；不能宣称抵御恶意同源脚本、特权 OS/browser 访问、磁盘回滚、存储驱逐或复制的浏览器配置。丢失记录应保持 Locked，不应自动重新创建凭据。
