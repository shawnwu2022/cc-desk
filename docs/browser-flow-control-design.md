# Browser 有界流控：独立传输阶段设计与未证义务

状态：设计评估；生产功能关闭。本文不是协议定稿或安全证明。
基线：d6f21464。不得仅增队列、缩小终端网格、增加 fixture 延时或放宽截止来消除失败。
不修改 Relay 云服、真实凭据或系统安全设置。

## 已确认的 RED

真实原 TerminalClient 自动续租 + 原 WASM/TLS + 本地真实 Relay + Native Host：
LeaseState、两次自动 LeaseRenew 成功，5.2 秒无业务保活后首次完整 Snapshot 失败。
首个失败是 browserRelay incoming backpressure：queuedFrames=4、queuedBytes=43480。
随后原 WASM 返回 DataError::Io，Host read-grant authority_failed 是连锁关闭，不是首因。
Host 在约 9 毫秒发出前 8 个数据片；不是 1 秒预算超时证据。
证据：.audit/browser-live-auto-smoke-outer-error.log，以及此前 diagnostic/requests/
host-trace/frame-trace/delivery-trace/wasm-error 日志。诊断插桩已移除。
61 秒组合验收尚未启动；没有公网、实体手机或后台节流通过的结论。

## 现有调用链与不能复用的语义

- host/local_writer_attach_connection.rs：Snapshot 持有单次 dispatcher，取得原 frame。
- host/local_writer_view_delivery.rs：SnapshotEncoder 的各片依次 send_projection；
  本地 flush 不是远端消费确认。
- client/src/lib.rs：收集完整 Snapshot（最多 4 MiB）后才返回。
- ui-terminal/client.ts：完整安装后 ProjectionAck；Window 是输入授权，不是字节信用。
- transport/src/lib.rs：duplex 每次 read 到 n<=16384 字节就封装一个 outer 帧。
  任意合法短读使一个逻辑片/TLS record 产生任意多个小 outer 帧。
- browserRelay.ts：现有 4 帧/65536 字节队列是溢出关闭保护，不是对端背压。
- browserTransport.ts 与 browserTlsWorker.ts：逐次 MessagePort pull，不能暂停经典 WebSocket。
- DATA_QUEUE_SLOTS=4 是进程内容量，不是对端信用；空 outer 心跳也不是信用。

因此单片 stop-and-wait 无法证明 outer 帧数<=4。约 15 片在 1 秒预算内的
RTT 粗上界约 66 毫秒，不能以 localhost 通过推断公网可用。
两逻辑 record 窗口同样不能直接证明 64 KiB：协议最大 payload、inner header、
TLS 开销和 post-handshake 字节必须全部计入，不能使用典型 10 KiB 片作上界。

## 候选方向：原 transport 的累计密文字节账

这是待验证候选，不是已授权启用的生产协议。

1. 新能力必须在原认证协商中明确选择，旧端不启用；新模式不能静默降级。
2. 不增加 outer 类型，不让 Relay 解释信用。信用通过同一已认证 TLS/Data owner
   的内部控制消息，绑定原 channel、方向、epoch；不经过业务 TerminalClient Busy。
3. 原发送 owner 分开记录尚未发出的预留量 Q 与原写入完成凭据确认的累计发送高水位 E；
   原接收 owner 记录真实释放累计 R。ACK 只能确认 E 中实际可确认的前缀，不能确认 Q。
   写入失败/部分写入/flush 未确认时，Q 不能凭 API 调用已返回转换为 E；精确提交点仍待证明。
   对端只承认单调累计 ACK，且 ACK<=E；重复 ACK 不增加额度。
   使用固定 W，未回收占用 Q+E-ACK，禁止对端任报余额或直接改变 W。
4. 所有计数使用检查过的整数运算，溢出、跨 epoch、乱序、越界或身份丢失关闭原 owner。
5. 候选固定字节 ring 仍最多 65536 字节，消除小帧对象数量的偶然限制；
   这只解决表示方式，不能单独修复超 byte-cap 突发。超限仍 fail-closed。
6. 信用只回收原传输资源，不授予输入、续租、目标选择或延长任何绝对截止。

## 初始化与兼容性：尚未闭环

状态候选：Dormant -> Negotiating -> Active -> Closing -> Joined/Unknown。
只能在原 Installed 认证和新能力协商完成后激活一次。不得跨连接恢复计数。
原 channel 身份/epoch 必须为内部所有权凭证，不能由 JS 参数伪造认证结果。

必须在实现前择定并证明一条计数基线：
- 从第一条 TLS 密文字节持续计数，并在认证握手中对齐双方累计位置；或
- 用原通道屏障精确封闭认证前残余字节，再建立新 epoch。

不能把两边本地计数简单清零：bootstrap、TLS 内部缓冲、duplex、WS 和 MessagePort
均可能仍有在途字节。未对账残余必须算入窗口，不能凭 descriptor 发布声称排空。
初始化、信用控制记录和 TLS post-handshake 数据本身的额度/存储也须明确。
旧端兼容路径与新端拒绝降级必须分别测试，不能修改 Relay 部署来掩盖缺口。

## 未证义务 A：真实消费与总内存账

postMessage 成功不等于远端释放；Window 将 bytes 交给 Worker 后不得立即 ACK。
Worker imports.read 清空 JS rx 也仅将字节复制入 WASM，不证明 rustls 已释放。
必须建立原 read loan 的唯一收据，处理部分消费、重复/延迟收据、终止和跨 epoch。
没有原收据不能开始下一份无界 loan，不能靠新的 Promise/消息队列转移积压。

总账至少包括：浏览器 WS 事件暂存、ring、MessagePort 复制/转移、单个 pending read、
Worker rx、WASM 临时读取数组、rustls 密文/明文缓冲、Data pending/inbox/credits、
Snapshot 累积及其复制、双方发送缓冲和信用控制容量。
目前代码仅证明若干单层界（如 16 KiB read、4 个 Data slots、4 MiB Snapshot），
尚无全部层的可加总固定界。尤其浏览器实现内部 WS 缓冲不能冒充应用已证明的内存。

实现必须列出每层最大字节数、最大对象数、所有者、转移/复制次数、释放点、
取消后原任务 join 的观察。若采用 TLS 消费而非应用释放点，必须证明下游固定容量
如何反压回该消费点，不能循环 ACK 将无界积压移到 rustls 或应用层。

## 未证义务 B：双向信用与 TLS 半 record 死锁

反例候选：双方数据窗口耗尽，各停在一条 TLS record 中途；信用消息已生成，
但排在同一 TLS 字节流未完成的 bulk 之后。独立控制槽不能穿越半条 TLS record。
截止后关闭只是有界失败，不是正常进展证明。

需要评估完整 TLS record 的预留/不可中断发送与有界控制优先级，或其他可证明机制。
不能只在应用层分配一个 ACK 槽。控制消息、TLS 封装和可能的额外记录也要计费。
必须禁止 ACK-of-ACK 自激循环、控制消息洪泛、饥饿和无限 coalescing 历史。
仅控制流也必须测试：每条 ACK 再触发 ACK 会永续自激；一概不确认控制字节则固定
信用会被有限次控制流耗尽。二者均不是可接受方案，更不能直接把控制字节免计费。
不得假设一条应用 write 对应一条 TLS record 或一个 outer 帧。

## 威胁模型与不变量

假定 Relay/网络可丢弃、延迟、重复连接或发送非法 outer 数据；经典 WS 不提供
消费者背压。未认证信用不能扩额度。认证但恶意的 peer 也不能越过本地硬上限、
获取新权限或让本地保留无界账本。可信客户端本身的资源收据仍须来源于原 owner。

必须保持：
- 默认未激活无数据额度；原认证/协商是唯一激活入口。
- 同 epoch 的有效 ACK 只释放已发送的前缀，重复不加额，回退/越界关闭。
- 队列、loan、账条目、控制请求均固定有界；不能因运行 30 天线性增长。
- 本机撤销、设备删除、信任失效、断连、grant 截止与单 RPC 截止覆盖等待信用阶段。
- 关闭不伪造完成、不重发原 Snapshot/输入；等待原 transport/Worker/Host 退休。
- 外层心跳仅保活原传输，不能突破信用或任何固定结束时间。

## 分阶段验证门禁

F0：保存上述真实 RED；失败路径先报告 Node 原错并关闭/join，不再等待不足 6 字节的
Native 探针掩盖原错。强制 kill 必须与正常 retirement 区分。
F1：纯状态机/反例测试，不接生产。覆盖计数基线残余、跨代/重复/越界信用、
loan 仅转移未释放、部分释放、隐藏队列、两侧半 record 耗尽、控制饥饿和截止。
模型测试通过不等于 Rust/WASM/WS 实现通过，模型假设须逐项映射真实调用点。
F2：初始化计数基线/激活屏障/兼容性独立审查门禁，以及 A、B 两项未证义务
全部通过后才接最小原链路；补突发、任意碎片、多片、恶意超限、
关闭/撤销以及原任务 join 测试；同时检验旧端兼容和新端拒绝降级。
F3：同源码真实 5.2 秒无业务保活 + 完整 Snapshot/ACK/Window/输入/settle/stop；
再跑真实 61 秒，只允许生产自动续租与 outer 心跳，中间无 fixture 私调 renew。
F4：公网 RTT/吞吐、1 秒 RPC 预算、最大 Snapshot 和手机后台行为单列实测门禁。
不承诺 1 秒处理 4 MiB 公网快照，不把本机模型或活动长测当上线验收。

当前禁止跨越 F1 进入生产接线。上述未知项需记录为阻塞而非以假设抹除。

## F1 探索模型记录（不是 F1 安全完成）

browserFlowCredit.model.test.ts 仅包含 test-local 模型，不导出也不接入生产。
定向执行 11/11 通过，日志 .audit/browser-flow-credit-model-review2.log（166ms）。
覆盖残余基线、预留与已发送分账、重复/错误方向/跨代/越界 ACK、转移与复制非释放、
可达的双向半 record 阻塞、纯控制枯竭和反馈自激等反例。
这些通过表示反例被重现/理想账本约束成立，不表示真实内存与无死锁已经证明。

额外并发义务：对端真实 ACK 可能早于本地原 write/flush future 的完成观察。
不能因此把 ACK 上限从已发送高水位放宽为预留量；需确定真实提交点，或仅有界
保留超前 ACK 等待原发送事实，期间不释放额度。模型没有覆盖此真实 IO 时序。
两项失败中的验收测试不属于此独立文档/模型阶段提交；5.2 秒链路仍 RED。
