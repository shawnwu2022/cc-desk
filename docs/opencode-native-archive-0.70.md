# OpenCode 原生归档与恢复界面

0.27 的归档抽屉提供独立的“OpenCode 原生归档”面板。它与 Claude 恢复区、永久删除按钮、Desk 行恢复入口分别保留，不用隐藏、删除或启动会话代替原生归档状态变化。

## 来源与确认

候选只接受已关闭、Desk 已归档、没有 preparation/resume pending 的 Native CLI OpenCode 行。原生历史必须加载成功，且 source/profile revision/root/project/session key/native ID 精确匹配唯一记录。当前状态从该条已验证历史的 `archived` 读取，界面称为 `nativeArchived`，不从 Desk 的 `archived` 推断。

面板明确显示“原生归档”或“从原生归档恢复”，勾选确认仅适用于当前来源和期望状态。来源、目标或原生状态变化同步撤销勾选，关闭/取消不发送写操作。恢复不启动终端，也不修改 Desk 归档元数据。根记录、子会话及附件不删除，不启动或停止外部 CLI。后端操作串行期间可短暂阻止 Desk 新启动。

## 独立协议

通过既有 authenticated document bridge 调用 `cli_set_opencode_archive`。

- 严格请求为 `{ operationId, source, sessionKey, archived, acknowledgeRecoverable: true }`。`operationId` 是 UUID，`source` 是当前已验证的 OpenCode configured-profile `SourceRef`，`archived` 是明确的期望状态，不是 toggle 指令。
- 严格回执为 `{ operationId, source, sessionKey, archived, state: 'archived' }`。即使恢复，`state` 仍是固定协议标签；实际状态以匹配请求的布尔值为准。
- 请求/回执额外字段、错误来源、操作 ID、目标、状态或已失效 document 都拒绝。接口不发送物理路径或 helper 参数，不展示原始异常。
- 错误仅使用既有 source/auth 白名单及 `OPENCODE_ARCHIVE_UNSUPPORTED`、`ARCHIVE_SCHEMA_UNSUPPORTED`、`ARCHIVE_SOURCE_UNAVAILABLE`、`ARCHIVE_OUTCOME_UNKNOWN`、`ARCHIVE_OPERATION_CONFLICT`、`NODE_RUNTIME_UNAVAILABLE`、`SDK_HELPER_UNAVAILABLE`。`SOURCE_BUSY` 沿用现有白名单。

## 未知结果与只读核对

发送前以同一物理根与原生 session ID 保留 Pinia 未决意图，跨组件关闭/重开保留。重复点击、重开、新 operation ID、不同 scope 或反向状态均不能绕过未决目标；单个 API client 也拒绝重用 operation ID。

成功回执后强制重新读取 native history，再刷新现有统一目录。未知/断连不自动重发或自动切到相反状态。用户可明确点击“刷新原生状态”，仅刷新历史，不调用写协议。未决操作只有同一 backend instance、根、配置修订、项目、session key 和 native ID 的正向期望状态证据能清除未决；缺席、错误、旧状态和不同来源均不算成功。刷新可使用新准入 scope，但不能借它替换原目标身份。未决状态不是磁盘操作日志，应用进程重启不继承前端执行身份。

## 本地验证边界

在 WSK-MSI 的独立 worktree 中先编写 API/UI 行为测试。API 占位实现 5 条 RED；UI 的同 tick 来源替换漏洞单独复现 RED，再用同步确认撤销修复为 GREEN。新测试覆盖精确协议、明确恢复、一次发送、失败重开、缺席/换根/读取失败、运行态排除、关闭中断，以及归档抽屉与永久删除入口隔离。

前端测试使用合成 authenticated bridge 和历史投影，不写真实 OpenCode 数据。Vue/TypeScript 检查、Claude 恢复区、永久删除、原生归档与归档树回归独立执行。本切片不包含 Rust、官方 helper 真实运行、真实用户会话或安装版 WebView/缩放验收；这些证据不能由前端 mock 测试代替。

后端保证 unsupported/schema/source/node/helper/busy 这六类固定错误只在发出 mutation 前返回，因此仅在写调用本身收到这些错误时释放该意图，允许用户重新选择和确认，绝不自动重试。成功回执之后的历史读取即使返回 SOURCE_BUSY，也不能沿用这个例外清除 pending。冲突、未知、失权与断连仍等待只读核对。

本切片最终前端结果：11 个受影响行为文件共 151 条通过（新 OpenCode API/UI 16 条），另 3 个 capabilities/product-boundary/design-token 文件共 22 条通过。Vue/TypeScript typecheck 与 git diff --check 通过。除 API 占位 RED 外，同 tick 来源替换、前置 busy 错误重试均各自实测 RED → GREEN。未运行全量前端、Rust、生产构建或真实 CLI/WebView 验收。

## 原生归档行的普通恢复边界

统一目录额外投影展示用 nativeArchived。OpenCode 原生已归档行不显示普通恢复主动作或菜单；行尾提供明确标注的独立原生归档入口，打开面板并预选同一行。后端历史仍为原生归档时，adapter 也在 cliUnarchive 或 Desk 元数据修改前拒绝普通恢复，旧事件不能调用不受支持的 OpenCode unarchive verb。独立原生恢复并刷新后，如仍有 Desk 归档标记，普通 Desk 恢复重新可用，不自动恢复或启动终端。

此路径新增两条 adapter/UI 联动用例，先实测 RED，再修复通过。它们从 archived=true 的 native history 生成统一行，验证独立面板入口、无普通恢复副作用，以及原生恢复后 Desk 恢复动作重新出现。
