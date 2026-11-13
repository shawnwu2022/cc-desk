# Native Host 单次 start 入口

本片基于 `1de904b25326e918dbb8f04fc181b81ef1d57fb5`。增加受控应用入口，不安装真实 Host 配置，不启用无人值守访问。旧生命周期文档中的“尚无 start IPC”描述的是各历史分片，当下入口以本文为准。

## 调用与状态

`remoteHostStart` 只经 authenticated document bridge 调用 `remote_host_start`。请求仍使用原严格 runId/generation DTO；未知字段、重复字段和错误类型在原鉴权路径拒绝。feature-off 或非 Windows 的命令入口不接收/解析请求，直接返回 `REMOTE_HOST_UNAVAILABLE`。生产命令不接收配置路径、key 名、批准布尔值或原始句柄。

Windows remote-host 依次经过原 document/run admission、controller 原配置/批准 owner 检查、单次 claim、原受保管准备线程、实际 join、原登记 driver。登记成功后，该 driver 只调用一次原已鉴权网络 begin，生产连接仍使用 WebPki。没有新增后台协调任务，没有把 Webview 移入工作线程，也没有让查询推进准备、登记、clock 或联网。

`start` 成功返回 `Consumed`，仅表示本次请求已受理，不是连接成功。`Registered` 仍只是历史登记事实，不能展示为 Ready。同步网络拒绝或 panic 撤销同一批准 token，不补票、不重试、不返还材料；既有网络 Unknown 与原句柄保管继续有效。异步连接失败走原网络停止/实际 join 链路。批准截止、grant、单次签名、原 setup deadline、退出拒绝与 Unknown 均未放宽。

原内部 `prepare_owned` 保留登记-only 行为；新 `start_owned` 使用显式联网模式。原测试 019 等不会因新入口而悄然自动联网。配置仍由原生可信调用者安装不可变 `FrozenHostApproval`，生产默认为空，未配置时保持 Locked/拒绝 start。没有新增 JS、环境变量或文件配置加载器。

## 测试边界

新的 `HostStart_` 组合使用公开匿名 CNG Existing 适配、隔离 relay 和实际 Node 子进程。原 start 驱动自主联网，测试不会调用网络 begin 补成功，只观察其已发布的原 attempt。覆盖 Phone 先 poll、Host 先发起时的会合/TLS、真实终端 snapshot 与 ACK、登记后取消/关闭、同步 clock 拒绝与 panic、异步无对端失败及重复请求。批准前请求与错误 owner 不得污染原槽。

两种调用/poll 顺序不证明实际 relay 登记先后；精确协议顺序保证与既有 relay/transport 独立验收分开。测试没有为此新增生产 transport 能力、暂停网络 deadline 或将 Ready 解释为端到端授权。

同步 clock panic 用例期待网络 Unknown 保留，即 `network_joined=false`；key/driver 和物理 fixture 收尾是独立事实，不能将测试进程退出或 fixture 清理称为整个服务已知退休。原任意 panic payload 阻塞及原句柄保管由既有 Driver 回归独立覆盖。

本片没有真实 WebView IPC 执行、真实原生批准对话框、实际用户命名 key 打开、真实配置安装、真实手机或公网端到端验收。生产配置仍缺失，不能据宿主测试声称完整远程产品已可用。无凭据配置、权限改变、部署、推送或发布。

证据位于项目内 `.audit/host-start-entry-20261114/`。`bridge-red` 是新入口缺失的行为 RED；`build1` 因环境中断留下截断 JSON，不计构建成功；`clippy1` 为新测试多余 import 失败，修复没有添加 lint allow。后续同源冻结清单、精确 executable 哈希和各项结果分别记录，不把历史快照合并成一次验收。
## 本片冻结验收

冻结11个源码/测试文件，remote-host 精确产物 SHA256 为 `CF40A2EA7E685B54EE6AE7145D0D0F7E49C26FE713FCF65B9D2B1EF4E448038A`。新 start 7 条、原 NativeRemote 19 条、受影响 driver/approval/activation/network/DTO/会合 68 条均逐原始进程通过。另一个 feature-off 精确产物的四个禁用入口测试通过。Windows remote-host 严格 Clippy（lib+tests、-D warnings）、四个修改模块格式检查、前端 bridge/权限测试16条及 typecheck 通过。client-only check 通过，保留原有3条测试 helper dead-code警告，不称该模式严格Clippy通过。未运行全量Rust或非Windows原生编译。
