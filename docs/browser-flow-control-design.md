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

## 优先候选修订：显式协商的有界 Snapshot 拉取

路线已选择进入安全设计/RED，尚未选择生产启用。
相较通用 wire-credit，该路线不在 TLS 字节流中途等待新的传输信用，因而不引入
同流半 record 后排队 ACK 的新等待环；不需要把原 transport 改成 TLS 控制调度器。
代价是额外 RTT，原固定 1 秒预算仍保留，公网大快照不作可用性承诺。

候选状态：Idle -> Captured -> ServingBatch -> AwaitNext -> Delivered -> AwaitProjectionAck。
任何阶段均可 Closing -> Joined/Unknown；截止、撤销、设备/信任失效必须独立触发。

- 新协商能力与新请求格式精确匹配。旧端路径明确分离，不以静默降级绕过保护。
- 每连接最多一个冻结的原 CapturedProjection，继续占用原 MAX_VIEWS=2、每view
  4 MiB 的预算。不得每次 Next 重新capture、换stamp或增加保留快照。
- 每个请求只能触发一个严格限制编码总字节的小批响应；Client 完整认证、验证并
  消费该批后才请求 Next。恶意超批/乱序响应直接终止，不缓存待将来拼装。
- transfer 标识不是新权限：必须与同一 channel/target/binding/fence、原stamp、
  精确下一offset和请求序列同时匹配。重复/跨代/越界 Next 不能重放发送。
- 整次传输端到端截止取自首次请求，不因 Next、heartbeat 或续租重置；
  每批只使用原剩余预算。Host 不能因等待下一个请求而无限保留 frame。
- 最后一批仍不授输入；完整安装后保留原 ProjectionAck/Window 流程。
- 固定 64 KiB byte ring 或等价有界拼接仍是必要候选：任意小 outer 碎片不能
  每片新增无界对象。不得把这个表示修补单独称为背压修复。

### 拉取路线仍需证明的上界

已查锁定源码：inner grant DATA_PREFIX=114，outer Native pump 每次读至多16384；
rustls 0.23.46 默认明文 fragment 长16384、接收 MAX_WIRE_SIZE=18432+5、
默认 sendable buffer 64 KiB；原 ServerConfig 禁用 TLS1.3 tickets。
这些是单层事实，不足以直接证明一个拉取响应的总 wire bytes 或总驻留。

响应cap必须包含实际编码后的 inner header、Data envelope、全部 TLS records、
可能的 KeyUpdate/其他自动记录及初始化残余。禁止把 payload 上限直接等同密文上限。
不能假设一次 write 就是一条 TLS record，也不能使用典型片长；必须记录真实原
TLS配置/编码器可达最坏界，并以最大payload/任意碎片/自动控制记录回归验证。
若边界无法证明，拉取候选仍停在设计阶段。

下游内存账仍需包含批响应解码、WASM/JS复制、最终4MiB Snapshot以及原view。
Client 请求下一批不能把仍被多份引用的上一批当作已释放；允许的累计Snapshot
必须单独计入固定总账。一个无限请求历史数组同样不可接受。

### 拉取路线 RED/验证清单

1. 模型：一个原view、一次固定截止、精确Next、重复/方向/跨代/超限拒绝；
   未完整消费批不能发Next；关闭/到期在等待Next时也释放原view且不伪造join。
2. 编码：最大Unicode/style网格保持完整覆盖；响应按编码后字节cap分批，
   不缩小网格；总Snapshot仍有原上限；拒绝超批及未授权追加帧。
3. 原链：任意小碎片、突发多片、慢消费者、撤销/断连/早到ACK、旧端协商与拒绝降级。
4. 同源码5.2秒原RED转绿后才做61秒组合验收，RTT与吞吐仍是独立门禁。

上述只是有限比较后的较小候选，不是内存/兼容/无死锁证明已经完成。

### 原生命周期补充

现有 deliver_bound 消耗 self；SendingGuard 在未完成时 Drop 会 REVOKED 并关闭。
不能在每批调用这个完整发送函数后提前 DELIVERED，也不能临时 drop guard 再重建。
候选必须由原 Host owner 跨批保留唯一 Transfer、原 guard 与 CapturedProjection。
MAX_VIEWS=2 是全 Host 共享预算，绝非每连接各有2份。撤销立即禁止 Next；
仍在途的原发送必须继续观察/join，不把“发出close”视为已经释放全部资源。
到期/撤销/断连的回收必须由原owner独立驱动，不能依赖下一个 Next 到达。
NativeClient 整个 snapshot 持有原 pending，Next不开放并行 exchange/LeaseRenew。

## 测试专用真实 TLS 探针阶段（仍非生产修复）

`tls_batch_probe.rs` 仅经 `tls.rs` 的 cfg(test) 加载，复用原 connect/accept、
测试 RPK 身份、真实 rustls 0.23.46。未新增生产接口或传输协议。
同源码两例 2/2 通过：`.audit/tls-batch-probe-final.log`。
正常 32776 明文字节产生 [16406,16406,30] 共32842 wire；
主动 KeyUpdate + 每次底层最多7字节写产生 [27,16406,16406,30] 共32869 wire。
短写例 write_all 成功时只有888 wire，flush 后才32869；写成功不是消费证明。
KeyUpdate+7字节短写例首次应用读取1字节时底层已收18464 wire；普通例为18437。
rustls Reader此时仍持有16383明文字节。
读完整批后 Reader 返回 WouldBlock，只证明当前明文队列无可读内容，不证明
分配容量、WASM/MessagePort/JS副本已经释放。

更重要：KeyUpdate收到后空flush没有输出（0字节），但下一1字节请求产生
[27,23]，相较无KeyUpdate的[23]多27字节。空flush、wants_write为false及
当前明文队列空都不能合起来推导“后续没有TLS控制残余”。握手输出计数未重置；
本次两例首批前未读残余为0，仅是这两例观测，不是全链激活屏障证明。

源码可推导的条件公式：锁定ring TLS1.3 AES128GCM/ChaCha（均16字节tag），
默认16384明文fragment；每个非空应用record增加5字节头+1字节类型+16字节tag。
若一次完整明文提交P且未受内部缓冲余量分割，则wire=P+22*ceil(P/16384)。
32776对应32842；独立KeyUpdate明文5字节，对应27 wire。
依据：rustls msgs/fragmenter.rs、crypto/ring/tls13.rs、common_state.rs。
这不是通用批上界：原data_owner.rs使用poll_write(bytes[offset..])，待发余量
和每次接受边界可增加record数；必须证明激活/前批flush/额外控制的实际状态。
queued_key_update_message是一个Option，但被perhaps_write_key_update取走后
可再次排入；每次应用数据会重置32次KeyUpdate接收计数，不能把Option解释成
整个传输生命周期最多一个控制record。告警/关闭记录须独立有界并终止owner。
本探针没有强制另一cipher、序号自动换钥阈值或恶意KeyUpdate洪泛，不能代替这些门禁。

当前仍不接生产：需真实批编码/owner截止边界、旧控制残余账、原Worker消费接点
和固定总内存预算。保留原5.2秒smoke失败、61秒未开始的事实。
Pull模型现9/9（含每连接唯一Transfer槽位），仍只是探索约束；Set删除不等于
真实Arc回收，手动tick/join不证明主动回收，Unknown发送结果不得冒充成功join。

## 原 NativeClient 资源语义上的候选拉取实验（仅 cfg(test)）

client/src/snapshot_pull_probe.rs 挂在原 tests 模块，使用原 NativeClient.pending、
record 验证、真实 RunningChannel/TLS/grant，而不是另建模拟资源账。
其 pull 是私有实验函数，尚未替换生产 NativeClient.snapshot；生产无新调用入口。
当前握手无 snapshot-pull capability，不造 bool/fact 冒充协商；旧 command decoder
确实拒绝 Begin。下一生产阶段必须把 capability 纳入原绑定 transcript/channel fact，
从 ManagedChannel 传递到 owner/BoundDataAuthority，再允许显式新模式；不自动退旧模式。

候选消息为 Begin(request) / Batch(request, transfer, index, part) / Next(原request,
transfer, index, offset)。Next不消耗UI顶层request号；认证Data序号仍由原安全层递增。
一个Batch是一个DataRecord；part自带完整原stamp/state，独立BatchDone不另发记录。
实验批payload受原32768-DATA_PREFIX界约束，累计part受原4MiB界约束；这些不是密文
或全链内存上界。transfer当前为固定测试值，Value解析容忍重复JSON键，不能原样推广
生产codec；生产须由严格DTO/规范化编码及原owner唯一transfer绑定替换。

迁移接点：原 NativeClient.snapshot 内保留整个pending与首次绝对end，在单批词法作用域
内验证原record、schema/stamp/offset、累计字节账，复制part后离开作用域，再发送Next。
这使独占DataRecord及其许可、解析临时先drop，累计parts仍驻留。它不证明rustls/WASM/JS
分配容量释放。原真实安全层回归另验证保留4个record阻塞第5个、drop一个后恢复，
并且该阻塞不妨碍反向发送；不新增虚构消费事件。

本阶段候选实验4/4通过，另原真实credit回归单独记录。覆盖两批、错offset关闭、
并发顶层请求Full、完成后Window request2真实往返、已poll future主动取消及checked
Client/Host owner join。它没有真实Host dispatcher顶层counter，因此request2往返
不是Host counter接线已完成的证据，也没有模拟真实Snapshot捕获/guard回收。
固定截止测试在真实握手后暂停Tokio时间，advance30ms才送首批，总预算仍40ms；
虚拟定时粒度落在40–42ms。有效“Next刷新截止”变异在71ms被断言拒绝，已还原，
日志snapshot-pull-deadline-mutation-red2.log；首次未命中替换的变异无效，不计RED。
日志snapshot-pull-real-green.log记录还原后的4/4；无墙钟窄容差验收。

本片段只闭合候选交互与原资源持有/取消语义。仍未完成cap协商、Host原frame/guard
单transfer loop、严格生产codec、全链wire/内存账和浏览器接入；原真实smoke仍RED，
61秒未启动。禁止把该私有函数当成已实现的生产客户端或重用其固定transfer身份。

## P1：显式 Snapshot pull 能力事实（未接拉取生产链路）

新增 snapshot-pull-v1 显式 LiveSnapshotPull 绑定模式，依赖原 live-control
能力组合。完整 offer 仍进入原认证 transcript；混合模式拒绝，不静默回退。
原绑定工厂在成功协商后签发绑定 exporter binding 与本端角色的私有事实，
沿 ManagedChannel、PeerStart、BoundDataAuthority 传递；旧工厂默认无此事实。
修复首轮 client 工厂漏传事实的真实失败，不通过测试布尔值模拟协商。

本阶段只提供协商事实，不授予 grant、输入或 Installed 权限，不延长截止。
尚未接严格 Begin/Next/Batch codec、Host transfer loop、NativeClient pull、
浏览器固定字节缓冲及全链内存上界；原 5.2 秒 smoke 仍 RED，61 秒未开始。
原绑定回归 22/22 与 security 严格 Clippy 已通过；最终格式源码复核日志为
snapshot-pull-bind-final.log / snapshot-pull-clippy-final.log。
审查范围：篡改用例可能在协商即拒绝，不独立证明 MAC 覆盖；原完整 offers 的 hash 覆盖及跨 binding/role 拒绝为源码审查证据，尚无跨 binding 搬运动态用例。

## P2a：未接线的拉取 codec

Begin/Next 使用严格 DTO，拒绝重复/未知字段与非规范十进制；request u64，
batch/through u32。Batch 使用 SPB1 + BE request(8)/transfer(32)/batch(4)/
through(4)/final(1)/part_len(4) 的57字节头。final只0/1，长度完全相等，
单Data payload上限32654，原part上限32597。先检查上限再单次分配；
解码借用原part，不重复JSON转义。公有解码入口同时校验原projection和
through/final；原projection自身的字段兼容性保持，不能泛称所有内层字段严格。
当前request/transfer/stamp/offset的期望状态必须由原owner额外核验。

codec 5/5及严格Clippy通过；移除未知字段限制的有效变异失败后已还原。
仅codec，无Host/Client调用，不等于密文上界或浏览器内存门禁通过。
## P2b：原借用 encoder 的显式 payload 上限

新增 with_payload_limit，不改变原 new 默认行为。仍借用同一 SemanticSnapshot；
每批最多128cells，若编码超预算则按cell数量减半，最多7次减半，单cell仍不适配
则失败并终止迭代。每次保留一个候选Vec，原projection在序列化前验证有界stamp、
run<=128、固定Cell字段和网格；不是任意字符串重试缓冲，也不是全链内存证明。
累计原part仍受4MiB上限，未缩终端网格或重复捕获。Host未来必须传MAX_PART，
2048只用来施加小批压力。原188批兼容回归与80x24两种限额完整覆盖共3/3通过；
禁用limit的有效行为变异失败后已还原。尚未接Host/Client协议。
补充：最终encoding 5/5及严格Clippy通过；新增offset10单cell不适配与4MiB累计失败的fused真实回归。临时serialize沿原128cells固定结构上界，不承诺临时峰值<=payload_limit；仅返回批受该limit限制。

## P2c：原 Rust Host / Client 拉取闭环（浏览器及生产入口仍关闭）

显式 NativeClient.snapshot_pull 仅在原 P1 认证事实存在时发送 Begin，旧 API 与旧绑定
不自动切换。Host 原 Connection.dispatch 持有同次捕获的 frame、SendingGuard 与 busy
状态；其原顺序 receive→dispatch 调用链内只有此 future 等待 Next。批次绑定原 request、
基于实际 binding + 完整原 stamp 派生的 transfer、批号及 through；Next 不占顶层 UI
counter。Client 调用公有 validated codec，再核原 channel/scope/fence、stamp/state、
offset 与 transfer，保存原 part，并 drop 实际 DataRecord 后才发 Next。

两端累计预算均为原 part 的 4MiB，不将 57 字节外壳重复计入累计；Client Vec 只留原
part。Host encoder 的小批限制来自协议 MAX_PART，不事后拆 JSON。等待 Next 保持首次
绝对截止，并监听关闭/撤销；每次后续动作仍检查原 grant。成功后原 ACK / Window
request 2 可继续。旧 delivery 不计算新 transfer，也不额外编码其 stamp。

本阶段同源码回归：bind 22/22、Host snapshot_pull 5/5、Client 全库 24/24。
Host 测试包含原 dispatcher 真实捕获、多批、counter 2；生产 NativeClient 与生产 Host
同 TLS / P1 / grant / Installed 链路；坏 Next、authority close、真正 GrantGate.revoke、
首次截止；实际已 poll future 取消后 frame credit 从 1 回到 0。暂停时间测试在首批后
推进 60ms，再收下一批，仍在原 100ms 截止退出。checked channel join 与 frame credit
回收不等于完整 native writer / 网络栈 retirement 或设备 UI 撤销验收。

P1 增加事实后的 authority 内联尺寸确实引起四个旧 enum 的严格 Clippy 回归；
隔离 9240bbaa、同工具链/features 的基线检查通过。中间 fact Arc / capabilities Arc
方案未保留。最终仅把不可变 binding、role 与三项独立事实放入私有 ChannelIdentity Arc；
grant、active、cancel、stop 未移动，same_channel 仍比较原 active 指针与身份值。
构造阶段 with_* 使用 COW，clone 后不会扩大原实例能力。跨 binding/role 事实拒绝及
COW 后能力隔离、同 channel 身份保留已有动态测试。新增一份固定身份 allocation，
不声明全链内存峰值已证明；未新增 Drop、弱引用或 grant 持有语义。

仍未完成：完整 wire / rustls / WASM / JS / 队列驻留内存账、浏览器拉取接入与真实突发/
碎片压力、固定 1 秒公网 RTT 可用性，以及原 61 秒低流量组合验收。原 smoke 失败证据
保留，不将本地 Rust 通过宣称浏览器问题解决；不接生产 UI，不推送或部署。
最终补验：原 data authority 11/11，Security/Host/Client 三 crate 严格 Clippy 通过。
有效变异在合法 Next 后错误重置截止，真实 Host 用例于 164.3264ms 拒绝（exit 101）；
已还原原代码，Host 5/5 再通过。对应日志 snapshot-pull-identity-{bind-final,
host-final,client-final,authority-final,strict-final}.log 与
snapshot-pull-host-{deadline-mutation,restored-final}.log。早先 bind_tests 过滤器
选中 0 项不计作回归；最终 bind::tests 实际执行 22 项。
此阶段近 4MiB 的双端累计边界尚待专门实测，后续 P2d 已部分补验 Client 精确边界；仍不以 encoder 的单独累计失败测试代替真实 Host 边界。
## P2d：累计边界与分层内存账（仍非 Browser 准入）

真实 NativeClient + 原认证/Installed channel 下，故障注入对端发送 129 个合法
SnapshotPart，各 part 保持正确 stamp/state/offset，仅补合法 JSON 空白使累计精确
达到 4MiB 或 4MiB+1，且各 part <=32597。前者保存的原 part 总和恰为 4MiB，
即使 Batch 明文 payload 总量（raw + 129×57，不含 Data/TLS/WS）超过 4MiB 仍成功；后者返回 Bounds、关闭原 authority，双方
checked close。对端不是生产 Host encoder。错误把 57 字节头计入累计的有效变异
使成功边界报 Bounds（exit101），随后原源码还原，Client 全库 25/25。

原生产 Host dispatcher + NativeClient 的最大合法空白网格 100×240 实测：
188 批、原 part 共 2,013,411 字节、24000 cells 完整，frame credit 回到 0。
Host pull 回归 6/6，Host/Client strict Clippy 通过。该 fixture 一次本机约 0.75 秒
不是稳定性能门禁，更不保证公网 1 秒。首次误用 120×200 的准备失败保留；
RGB 扩大内容尝试被原 canonical 门禁拒绝 Unsupported，未放宽且未计作 pull
失败/通过。不能声称该合法 Host fixture 自然触达 4MiB；Host encoder 独立累计
耗尽测试与 Client 精确边界各自限定证据范围。

内存账按所有者分开，以下是逻辑长度/数量界，未完成进程 allocation 峰值证明：
- Client 留存 raw part 总和 <=4MiB。每批 1..128 cells、state <=100×240，且原
  pending 强制 start 连续/同 state，所以 parts.len <=24000。Vec<Vec<u8>> 自身
  capacity、每个 allocation 的容量/对齐/allocator 元数据不能略去；不将 length
  当 capacity，更不将 raw4MiB 当全部内存。
- 当前解析一批至多128 cells（bounded visitor 对额外第129项先解析再拒绝），
  加独立 stamp/state 字符串及 clone。输入先受 MAX_PART 限制，但 JSON decode
  的临时容量与拒绝前字符串分配仍须计量。Client 保留当前 record 同时复制原 part，
  非末批在发 Next 前 drop parsed part 和真实 record；末批随 future 返回释放。
- 原 security read credits 共4份，覆盖 Reading/pending/Inbox/调用者持有的
  DataRecord，而非各层各4份。原 Data payload 分配 <=32768；剥离114字节前缀
  用 copy_within+truncate，不缩 capacity，故不能按当前 payload.len 少算此容量。
  发送侧4份 credits 下的请求 body、当前 Writing 编码副本另计，不能合并成接收预算。
- Host 同一 view/guard，encoder 每轮最多128原固定结构cells的候选序列化，成功
  原 part <=32597，包装 Batch <=32654；当前转换会短暂同时持原 part、解析结构、
  Batch，候选失败的临时序列化不能按最终payload limit声称同峰值。
- 当前本地锁定 rustls 0.23.46 源码默认 received_plaintext limit16KiB、sendable_tls
  limit64KiB；这些是库队列阈值，不是整个 TLS 分配上界。record处理、deframer、
  handshake临时、容量保留及tokio/websocket/socket缓冲仍需独立合算。
- Browser/WASM/JS 复制、worker消息、WebSocket碎片/4-frame队列尚未接入此协议；
  未证明端到端 wire/驻留内存上界，原突发失败与61秒组合验收仍未闭合。

证据：snapshot-pull-budget-{exact-second,header-mutation,client-final,host-final,
strict}.log、snapshot-pull-host-max-grid-measured.log；失败准备日志单独保留。
## P2e：Relay 固定 ring（独立表示修复）

原 createRelay 的 4-frame / 65536-byte 片数组替换为单个 65536-byte ring，不增
byte cap。仅原 admitted 后分配；完整 WebSocket message 的长度/路由/单子帧语义/
总余量先校验，再一次同步提交 ring。读仍只有一个 outstanding read、最多16384，
按 TLS 字节流顺序跨旧 frame 边界返回独立副本；close 清零 ring 并释放引用。
未知/第二子帧、截断、超限均原子拒绝，不发布合法前缀。未增加 writer 或排队 promise，
未改变原绝对截止、live activation、撤销、heartbeat 与 retirement 条件。

测试是 EventTarget Socket fixture 驱动实际 createRelay，不是实际网络 WebSocket
或浏览器峰值验收。首 RED 证明旧4frame规则拒绝多碎片/环回/缓冲关闭情形；GREEN
覆盖100个单字节片、环回后恰好65536、+1关闭、错误路由/截断/trailing、输入与
输出无别名、合法首子帧+非法第二子帧不发布前缀、缓冲/待读关闭。移除bytecap的
有效变异导致+1测试缺少应有拒绝，已还原。共享注册、DataKeepalive、Rendezvous、
Transport 生命周期回归与严格局部 TypeScript 检查另有日志。

本层逻辑驻留需同时计固定ring64KiB、当前message ArrayBuffer<=64KiB、read副本
<=16KiB及消息/对象开销，不能仅报ring容量；当前事件的buffer由WebSocket提供，
在handler前可能已有浏览器内部事件队列，此处无控制权。未将 JS 默认 GC 的释放
时刻当作已证固定峰值。尚不据此启用 Browser pull。后续必须实测每批 Data plaintext
32768+inner header8 的 TLS ciphertext/记录开销、post-handshake与前批残余、跨
MessagePort/WASM的副本及固定1秒预算，仍可能因bytecap/RTT正确fail-closed。
## P2f：真实 TLS 最大 Batch 计量（非全配置上界）

透明测试 proxy 位于原 TLS 下方，每方向固定16KiB临时数组，连接两对各64KiB
DuplexStream；不解密、不存历史ciphertext，仅累计bytes/records/read次数。
原P1/grant/Installed后，生产NativeClient消费认证测试对端通过原Data owner发送的
两个最大合法Batch（raw part合法空白填至32597，Batch32654）。这不是Host encoder
输出测试。计量位于proxy读取处、write_all之前，不是Client内存释放计数。

当前rustls0.23.46 fixture cipher/config、无额外周期控制/主动key-update插入时，
短读上限16384和257两种均得每批32842 ciphertext bytes、3条TLS records：
32768 Data payload + 8 inner header + 3×22。容量按bytes计，不以碎片/记录数代替。
握手/grant窗口2776或2777字节、Installed窗口375字节另计。第一批窗口结束于
真实Next，第二批结束于NativeClient返回，证明本fixture对应批已认证消费，
不证明未来没有延迟post-handshake或自动控制残余。

双方checked close和proxy正常join通过。审查发现原timeout拥有JoinHandle会在超时
detach：先以已启动任务Drop标记做有效RED，现timeout借用&mut handle，超时abort后
await再报失败；意外panic/取消由Proxy Drop请求abort，但不宣称该Drop同步完成join。
两个forward是同一个proxy任务中的try_join futures，不另spawn；其中出错会丢弃另一
forward。此为测试proxy清理，不冒充生产retirement证明。Client全库27/27、strict
Clippy通过。证据snapshot-pull-real-wire-{first,client-final,strict}.log及
snapshot-pull-proxy-retirement-{red,green,strict}.log。

32842只是此固定测试条件，不是任意TLS配置上界。任意post-handshake、旧数据/自动
控制残余、WebSocket内部事件队列、MessagePort/WASM复制及固定1秒公网RTT仍需门禁。
无真实凭据、云服操作或生产Browser默认入口变更。
## P2g：Browser 显式协商配置（2026-11-17）

peerInstall 新增可选 snapshotPull: "snapshot-pull-v1"，仅允许显式 live-v1 +
Controller；缺省/旧 manual/live 不自动增加能力。JS 严格字段/值检查与 Rust
String-only Deserialize 双端拒绝未知值及无效组合；Rust 实际 WASM owner 仍要求
原 application，并经原 bind factory 创建绑定本连接和角色的 P1 fact，不由配置
直接构造 authority。尚未接 RPC kind5，此阶段不能发起 Browser pull。

有效 RED：JS 新配置先拒绝，Rust 同样拒绝；首 GREEN 的负测发现 derive enum
会接受对象形式，已改只接收字符串而非放宽测试。最终 JS 3/3、Rust schema 5/5、
原 bind 22/22，原生 security strict Clippy、真实 wasm32 client strict Clippy
及 strict TS 均通过。首次 WASM 缺 clang 的环境失败保留，复用既有 LLVM21.1.8
工具路径后完成，不安装软件、不更改系统 PATH。日志 browser-pull-schema-*。
独立只读审查通过。非 application / mixed mode 的实际 WASM 动态拒绝仍待后续，
不得以 schema 和原生 bind 测试冒充 Browser 端到端验证；重复 JSON 原字节键
并非当前 JSON Value 测试的覆盖项。默认入口和云服均不变。