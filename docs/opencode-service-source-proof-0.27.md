# OpenCode 现有服务与 held DB 的同源证明

本次切片基于 `feat/v0.27.0` / `1c018f24bf52e6de421a84a397e28d052e1c5b8e`，固定官方 `v2.2.5` / `9bbb8ce18c80f099e0e448775f59d965ead40a21`。只读源码与协议分析，没有启动 OpenCode、读取真实注册文件/凭据/会话、初始化或迁移数据库、写入 nonce 或调用 mutation。

结论：当前已有服务身份机制，但已核对的公开数据及现有 Desk 适配链不能组成“HTTP 响应来自该实际进程正在使用的同一个物理 DB”的可靠证明。本轮不增加一个接受自报证据或永远无法签发的伪能力，不新增纯状态模型、只读预览 IPC 或 UI。原生 mutation 继续关闭。

## 信任根和已证实字段

| 维度 | 当前真实证据 | 能证明的范围和缺口 |
| --- | --- | --- |
| 请求来源与授权 | authenticated document、caller instance、profile ID/revision、ScopeRegistry owner、SourceRef、逐注册项目 admission | Desk 已有的调用者/配置/项目授权；不自动授权整个 DB，也不是服务接入凭证。 |
| held 来源 | `native_projection/scoped_fs.rs::Root` 保留目录 handle、目录物理身份和选定 SQLite 文件名；`source_current` 重验目录入口 | 可以拒绝根替换。`open_sqlite` 的 scope key 是目录身份+文件名摘要，不是 SQLite 主文件的物理 file ID，更不是服务打开的 DB handle 证明。 |
| 本次文件读取 | `opencode_db.rs` 的 Windows 私有快照从 held root 打开 DB/WAL/SHM、锁定并有界复制，Unix 使用 nofollow/重验 | 保护 Desk 自己那次读取；没有观测任意现有 HTTP 服务实际持有哪个 DB 对象。不能把这个保证外推为服务器同源。 |
| 服务注册 | 官方 managed service 每次生成随机 instanceID，注册保存 id/version/url/pid/password，并检查记录是否被替换 | 确有实例记录，不应误写为“没有服务身份”。记录没有 DB 物理身份、profile/root 或查询快照；读取真实注册会读取凭据，本轮不做。 |
| HTTP 健康/实例 | `/api/info` 实际字段为 version、pid、urls、tmp 及可选能力；客户端 discovery 比较响应 PID/version 与注册记录 | 是应用级认证/健康检查；没有 OS peer 身份、进程创建期、实例 ID 回显或 DB identity。loopback、PID/version 自报不能独立证明来源。 |
| 认证 | password 或 pairing session；服务保存的 password 可跨重启复用 | 证明请求方知密，不证明 DB，也不是仅限某 profile/只读查询的授权。不能提取现有长期凭据补链路。 |
| 物理 DB | CLI 从 data root、OPENCODE_DB/channel 计算 path，server start 接收 `database.path` | 这是启动配置的字符串来源。managed service 可先应用其保存的 env；Desk 的当前 profile 环境不必等于已运行服务当时的环境，且没有返回实际已打开 DB 对象的接口。 |

官方证据链接和准确来源：

- [实例创建、配置环境与 DB path](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/cli/src/server-process.ts#L70)：先应用 service env；`#L98` 创建 instanceID；`#L114` 向 server 传 DB path；`#L152` 才注册服务。
- [注册内容与五秒自检](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/cli/src/services/service-registration.ts#L24)：id/version/url/pid/password；替换后自停，不包含 DB 绑定。
- [客户端注册 schema/读取](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/client/src/effect/service.ts#L142)：id 和 password 兼容性上可缺失。该模块另有 ensure/stop 的启动、替换与终止行为，本轮不能用 ensure 当纯发现。
- [discovery 验证内容](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/client/src/service-probe.ts#L49)：发 `/api/info`，比较 PID/version；`same` 比较记录 id/version/url/pid。这不是物理 DB 或 OS 连接所有者验证。
- [公开 ServerInfo schema](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/protocol/src/groups/server.ts#L5)、[实际 handler](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/server/src/handlers/server.ts#L18)、[启动中/失败时响应](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/server/src/process.ts#L208) 均无 DB identity。
- [DB path 选择](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/cli/src/database-path.ts#L4)、[password 跨重启保存](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/cli/src/services/service-config.ts#L183)、[认证及 pairing token](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/server/src/auth.ts#L28)。均未声明提供物理 DB/只读 scope 证明。

Desk 现有 `opencode_prepare.rs::with_server` 保留自己创建的 Child/私有 Job、临时认证和有界请求，最后 kill/reap；它是创建或重命名时的短命 helper，不是可供本切片复用的长期原服务。v2 watcher 的已认证 refresh cue 也只证明同 run 通知来源，不公开 HTTP endpoint、DB 所有者或完整关系。

## 无法补出的关键边

目前只有“Desk caller → held scope”和“注册/认证 → 服务健康观察”两条链，缺少 **HTTP 连接的 OS 进程/生命周期 → 该进程的实际 DB 物理对象 → held DB 对象** 的连接。下面两种合法情况即可产生反例，无需假设恶意 CLI：

1. 同一版本服务按保存的 service env 使用 DB-B，当前 profile 选择 DB-A。两库可以具有相同 session ID/目录/元数据；注册、认证与健康检查正确，也不能证明 A/B 是同一物理对象。
2. 在允许已打开文件被替换的系统/文件机制中，服务可持有旧 DB 对象，而 Desk 后续 held-root 读取到同名新对象。相同路径、root key、元数据甚至相同完整字节都不能证明是原先打开的文件。这里不声称所有 Windows SQLite 打开模式都允许此操作；平台在某次拒绝替换也不能补出原服务 DB 与 held DB 的映射。

不能用读取另一份配置、比对版本或轮询 session ID 解决；也不能读取现有凭据、扫描所有进程 handle/配置、借用服务 ensure、写入 DB nonce 或再启动服务绕开。新服务初始化会执行 [WAL/checkpoint/migration](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/core/src/database/database.ts#L33)，不属于本次纯只读能力。

独立复核又确认 [server process 启动](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/server/src/process.ts#L109) 在存在 lifecycle 的 managed service 中安装恢复连续性，[实际调用](https://github.com/anomalyco/opencode/blob/9bbb8ce18c80f099e0e448775f59d965ead40a21/packages/server/src/process.ts#L245) 会启动 `resumeSuspendedSessions`；`#L114–115` 对 service/serve/standalone 都启动调度循环。是否存在待恢复/调度任务由数据决定，因此不能保证仅启动服务无副作用。本轮没有执行启动；这不把既有 create/rename 的不同授权范围定性为漏洞。

## 陈旧和并发条件

| 变化 | 安全证明必须具备的失效条件 | 当前不能代替它的观察 |
| --- | --- | --- |
| 重启、PID/端口复用 | 保留实际进程生命周期及连接对端绑定，generation 变更立即失效；不能把旧 credential 当新实例证明 | 注册 PID、`/api/info` 自报 PID、loopback、同版本或 password 仍相同 |
| symlink/目录/DB 替换 | 同时验证 held root 和实际打开的 DB 对象；服务、DB 或 sidecar epoch 改变不得沿用旧证明 | 相同字符串路径/文件名/root key，或只有 Desk 快照成功 |
| profile/revision/root 切换、跨 profile | 证明只由后端可信 issuer 签发，精确绑定 document/profile/revision/物理源/授权域；切换撤销原请求 | 前端传入“同源”布尔值、通用 scope ID 或全库可读的推断 |
| 并发查询、取消、迟到结果 | 每个请求独立 owner/sequence；撤销后不再发布，取消不停止外部服务、不重放、不赋予删除权 | 只在 UI 判断当前选项或把 requested 当已结束 |
| 查询截断、过滤、跨页变化 | 同一已授权域的单次有界 SELECT，明确 cap 且返回数小于 cap；达到 cap、越界、关系缺项/循环拒绝完整性 | next cursor 的有无、跨页去重或静默隐藏未授权后代 |
| 活动变化 | 显示该服务的观察时间和覆盖范围；其他实例未知。读取后不承诺持续静止，不能复用成 mutation admission | 未在 `/api/session/active` 出现就推定全部实例 idle |

即使来源身份得到解决，查询本身仍须满足 [单次完整投影读取边界](native-cleanup-readonly-preview-0.27.md)。正常数据变化不等于永久撤销读取权限，但旧观察不能被称为新状态；没有共同快照的关系/活动响应必须分开标注，不能拼成一个原子快照。

## 最小解阻机制与可执行验证方案

要实现该服务预览，需要先有一种可实际取得的信任根，而非先定义可由任意 JSON 构造的 Proof：

1. 已授权服务提供经 OS 可核实对端、生命周期绑定的私有只读通道，并把其实际 DB 连接所用物理对象/数据集 epoch 与 held DB 对上；字段必须来自真实已打开对象，单独增加自报 path/UUID 不够。授权不能靠扫描或取用现有长期凭据获得。
2. 或在未来另行授权的正常启动生命周期中，由 Desk 保留准确原服务及其 DB 对象的受控来源机制，并独立证明 CLI 确实打开该对象。本轮不为预览创建服务或改上游。仅保留启动 argv/env 和 Child 仍不足以跨越实际 DB 打开点。

当前固定版本/现有 Desk 都没有把这些条件接通，故本轮没有合法 issuer、没有可接入的纯验证模型。另一条后端直接只读 DB 快照可以支持“持久投影关系观察”，但它不能回答“某个现有服务是否使用该 DB”，不能拿它替换本切片的目标。

机制具备后的验证顺序可直接执行，全部数据限定任务内合成根；当前未执行以下未来验收，不计测试通过：

1. 创建 A/B 两个物理身份不同、元数据相同的合成 SQLite；使用隔离测试 helper，禁止连接现有 CLI 或服务。先写 RED：正确实例+错误 DB、正确 DB+替换实例、伪造 PID/version/path/registration ID、重启后复用端口/password 均不得签发。
2. 对真实 held 文件/目录做测试内替换与 symlink/reparse fixture；保留原 handle，验证同名/同内容仍拒绝物理对象误绑，未替换对照才可通过。只在合成 fixture 上操作。
3. 覆盖 profile/revision/root/调用文档变化、跨 profile 回执、并发请求互换、取消后迟到、截断/分页/过滤/越界关系；失效后无数据发布、无额外副作用或 mutation。测试必须覆盖可信 issuer 的生产入口，不能仅给 reducer 注入 `trusted=true`。
4. 新代码届时先跑固定 `1.98.1`、CI dev/test debug=0 的受影响 Rust 域、all-target Clippy/fmt 和前端受影响测试，并由独立审查核对证据来源；清点实际非零测试库存。纯 fixture GREEN 仍不证明 OpenCode 原服务完成真实 DB 绑定。
5. 只有另行具备已证明的任务隔离实例和授权，才执行原 CLI 的真实只读验证；仍不调用 ensure/创建会话/归档/删除，不以真实用户数据或凭据代替输入。本轮未申请这些权限或账号。

当前限制是技术契约不具备，不是工具审批拒绝或待用户批准的操作。完整预览与执行继续关闭；[mutation 原子条件](native-cleanup-blocker-0.27.md) 仍是独立边界。

## 后续有界核对：进程/连接归属加文件身份仍缺一条边

Microsoft 的 [TCP owner 结构](https://learn.microsoft.com/en-us/windows/win32/api/tcpmib/ns-tcpmib-mib_tcprow_owner_pid) 给出连接地址、端口和发起 context bind 的 PID，不含 DB 信息。[原始进程 handle](https://learn.microsoft.com/en-us/windows/win32/procthread/process-handles-and-identifiers) 在进程终止后仍可保持有效，PID 本身只在进程寿命内有相应保证，因此 retained handle 能加强生命周期识别，不能让重连/复用端口继承旧连接证明。[FILE_ID_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info) 的卷标识+file ID 可比较两个已打开 handle 是否指向同一物理文件，能加强 Desk 这端读取见证。

这三种见证仍没有连接到“该次 HTTP 查询实际使用的 SQLite connection”。最小契约反例是：同一个受控进程 P 同时打开 A、B，TCP 确属 P，Desk 对 A 的 file witness 正确，但 HTTP SessionStore 查询 B。即使证明 P 也持有 A 的 handle，仍不能推出查询使用 A。这里是一般契约反例，不是声称已在真实 OpenCode 复现了该情形。

现有 AdmittedChild/OwnedPty、ServerChild/Job 和 private SQLite snapshot 各自已有准确所有权/读保护；它们没有提供 HTTP request→SQLite connection→文件对象映射。文件锁成功/冲突也不能补这条关系。故本轮没有枚举真实连接/进程 handle、复制外部 fd 或生成伪 issuer。可在合成 fixture 验证同进程双 DB、连接池重连、端口复用等反例和两端 witness 正向对照，但对照通过仍不证明 OpenCode 原服务契约已具备。

本节只约束“现有服务同源证明”切片。后续已获准分层推进 Desk 元数据隐藏/撤销和可恢复原生归档，应按各自真实副作用和现有授权链设计；不能把这里的强服务证明机械加到仅改 Desk 状态的低风险操作上，也不把软归档与永久删除混为同一能力。
