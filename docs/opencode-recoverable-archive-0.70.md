# OpenCode 原生可逆归档：执行与产品边界

2026-11-12。本轮已将轻量官方 helper 接到 Windows 产品的认证命令与独立原生归档/恢复面板。没有开放 OpenCode 永久删除，不能把归档说成删除；没有启动用户现有服务、修改生产权限或处理真实记录。安装包和真实 WebView 全链验收仍须在集成后完成。

## 实际实现

- `scripts/opencode-archive-bridge.mjs` 直接调用官方 `@opencode/core` 的 `Session.make().forSession(id).archive()/unarchive()`，没有自行 UPDATE 会话字段，也没有创建全功能 SDK/HTTP 服务。
- 用公开 `Database.Service` 和 `EffectDrizzleSqlite.makeWithDefaults()` 接入同一个 SQLite 客户端。不创建默认 `Database.layer`，因此不做自动迁移、chmod，也不调用默认 Database 层的显式 checkpoint 或宿主恢复/调度。SQLite 连接关闭时可能执行自身的自动 checkpoint。Instance、Inbox、Execution、FS 服务一旦被调用即失败。
- 精确 schema/迁移 ID 清单、原生 ID/cwd 和目标大小检查，与原生操作共用 `BEGIN IMMEDIATE` 事务。竞争 writer 拒绝，原生方法失败回滚。请求只执行一次；原生调用后出现失败保守返回 unknown，不自动重放。
- 原生 Bus 显式 `persist:true`。重复目标归档状态由原生方法 no-op，不写重复事件。恢复只改归档状态，不回滚整库，不覆盖期间的标题或其他会话更新。
- 固定构建工具链是供方依赖的供应链约束，**不是用户 CLI 版本白名单**。helper 不执行 `--version`、不检查会话 version 字段。当前完整结构指纹来自官方 CLI/SDK 2.2.6 的两个全新库，二者相同；其他 CLI 只要结构兼容即可通过，不兼容安全报错，绝不自动迁移。
- schema 排除规则只忽略字面 `sqlite_` 内部前缀，不使用 SQL LIKE 的 `_` 通配符。额外触发器、表或未知迁移都会改变摘要。查询最多 257 个 schema 项、513 个迁移项，字段有界；目标 session_v2 与 event_sequence 各自行的全部列按 BLOB 实际字节总量限 1 MiB，主库限 1 GiB。
- `opencode-archive-sqlite.mjs` 是仅本 helper 的构建适配层：强制 `allowExtension:false`，拒绝启用/加载扩展和备份。它修复 Effect 4.1.0 固定请求扩展能力与 Node 权限模式的冲突，不修改上游包或全局运行时，不放宽 Node 权限。

构建得到约 4.66 MB 的单文件，不携带约 325 MB 开发依赖目录。只选择 jsonc-parser 官方 ESM 入口，排除未调用的可选 FFI；任何运行时外部模块 require 均拒绝。构建资源、188 个实际 bundle 输入包的许可清单以及安装资源映射已接入，详见 [构建说明](opencode-archive-build.md)。这不等于已经验证签名安装包。

## 已执行证据

全部在 WSK-MSI、任务新建 HOME/XDG/项目/数据库上执行，旧合成源仅用于复制实验，未接真实用户源。

1. 官方安装 CLI 2.2.6，exe SHA-256 `98cb0aa854c7c015cf6febafa7afb872c359bfe5a823a526f3b4fd347a9e9677`：6 项通过。归档/恢复后的原生列表可见性正确；子会话和无关会话保留；重复请求 no-op；晚到元数据保留；仅在合成树上验证 DELETE 递归范围，不把它称作安全拒忙删除。
2. 官方 npm SDK 2.2.6 完整宿主原型：5 项通过。公开 overrides 替换恢复/调度/执行；无关悬挂记录和到期合成哨兵不变；其他关系表与迁移记录不变；非法项目配置未加载。哨兵是测试直写记录，不是真实已执行计划任务。该较大宿主不是最终 helper。
3. 最终单文件 helper：10 项通过，Node 24.18.0 `--permission` 下只允许 bundle 读取以及准确 db/WAL/SHM 文件读写，不授予 child/addon 权限。覆盖原生归档/恢复、重复不增事件、其他连接晚到写入保留、错误项目、竞争 writer、未知 schema、恶意触发器、超预算/NUL/非 STRICT 字段与事件序列、migration 替换视图、不创建缺失库。
4. SQLite 适配：1 项通过，强制传入 `allowExtension:true` 仍不能开启/加载扩展或备份，普通内存 SQLite 操作可用。
5. 独立审查发现 `NOT LIKE 'sqlite_%'` 会漏掉合法 `sqliteXarchive` 触发器。真实 bundle RED 确认错误接受，改为严格前缀后 GREEN 确认拒绝且目标/无关记录未变。不是把内存模型测试写成产品成功。随后修复全列 BLOB 字节预算及先校验 schema、后查询 migration 的顺序；migration 视图错误码先 RED 后 GREEN，NUL/非 STRICT 字节计数由独立内存实证和真实 bundle 拒绝回归共同覆盖。

上述条目是独立 helper 阶段的证据；本轮产品接线的 Rust 与前端增量证据见下文，不冒充全量或安装包验收。没有执行真实活动模型任务、真实用户删除、生产 ACL 变更或 CLI 升级。最初直接 bundle 缺动态 CommonJS/JSONC 入口，以及未适配 SQLite 扩展请求的权限运行均失败；它们不计通过。

## 复跑

独立工具链位于 `scripts/opencode-archive-runtime/package.json` 与 lock 文件，不修改产品根依赖。开发依赖可用 `npm ci --prefix scripts/opencode-archive-runtime --ignore-scripts --no-audit --no-fund` 准备；不运行安装脚本。

```text
node scripts/prepare-opencode-archive-sdk.mjs scripts/opencode-archive-runtime/node_modules
node tests/scripts/opencodeArchiveSqlite.node.mjs
node tests/scripts/opencodeArchiveBridge.real.mjs .audit/opencode-archive-bundle/bridge.mjs scripts/opencode-archive-runtime/node_modules
node tests/scripts/opencodeRecovery.real.mjs <explicit installed opencode.exe>
```

测试会保留自己的隔离数据与报告，绝不接受用户数据库作为测试输入。helper 的脚本原型是后端组件，不是用户直接传路径的公共接口。

## Windows 产品接线

- 认证 document bridge 命令 `cli_set_opencode_archive` 只接受 `SourceRef`、已验证 session key、UUID 和明确的目标布尔状态。前端不提供 DB 路径。ScopeRegistry 先验证调用者/完整来源，然后固定本地 NTFS 目录链和主库/侧车，再读 catalog 身份，持有这些句柄直到 helper/Job 全部收口，消除读取身份与写入之间的同名换库窗口。
- 只接受已有 WAL 主库，主库加侧车合计最多 1 GiB。db/WAL/SHM 允许 SQLite 写入但禁止删除共享、重命名、替换、reparse 或硬链接。缺失侧车只按固定名称 OPEN_IF 创建，不截断。`-journal` 必须为空并持有拒写句柄，拒绝结构核验前的热回滚日志恢复。UNC、网络源、非 NTFS、rollback 模式或未知 schema 均拒绝。
- 单文件 helper 从编译期嵌入的字节安装到本次专属目录，以拒写/拒删除句柄固定并比较字节。Node 权限只允许这份脚本读取、准确 DB/侧车读写，不授予 child/addon 能力；扩展加载由适配层进一步拒绝。不继承 NODE_OPTIONS、CLI 凭据或项目配置。最低 Node 24.18 是运行时能力要求，**不是 CLI 版本白名单**。
- 后台 owner 以 suspended spawn 加 kill-on-close Job 控制精确子进程，来源与 admission 直到 child 已 reap 且 Job empty 才释放。IPC 最多等 35 秒，超时/失权/断连/错误回执为 `ARCHIVE_OUTCOME_UNKNOWN`，后台继续保留所有权；不是 kill 后立即宣布完成。
- per-document operation ledger 拒绝相同 operation ID 的不同请求，保留未知状态不重放。UI 在成功回执后重读原生历史，仅同来源/配置修订/项目/会话的正向期望状态消除 pending，缺席或失败不算成功。Desk 归档索引独立，恢复不隐式打开终端。

### 本轮短验证

Rust 定向用例覆盖请求/账本、WAL 写兼容、固定文件禁止换名、缺失/非 WAL/硬链接/危险路径、未知 schema、撤销、真实 native archive/restore 与竞争 writer、历史归档位、热 journal、断连/错误身份回执、读取前固定来源、IPC 超时后仍保留 owner/freeze。真实往返仅用官方 SDK 刚创建的假库，同时保留其他会话晚到标题及 job/schedule 哨兵。

前端定向测试覆盖独立确认、同 tick 来源变化、严格 API、只读确认与未决状态；构建测试覆盖隔离 npm ci、缓存、确定性、错误依赖版本和离线失败保留旧产物。最终命令与数量以本轮提交报告为准。没有运行真实用户数据、真实活动模型或生产权限变更。

### 保留边界

Unix 尚无同等来源句柄方案，后端明确拒绝。本轮没有执行 OpenCode 的真实 WebView 全链、签名安装包/升级回滚验收，不把单元和 helper 往返写成这些证据。永久删除的跨实例活动保护仍未解决，保持独立禁用；原生可逆归档只调整所选记录的元数据，不停止外部任务或删除子会话/附件。
