# OpenCode 准备链修复证据（本地隔离补丁）

本轮基于审查时冻结的 `fix/opencode-backend` 工作树快照（HEAD `0bcc8674749cbc145224389b1144a3eb19fcbf1a` 加用户原有未提交改动）。所有修复仅在 Temp 副本制作，未修改用户根工作树，也未把用户原有改动当作本轮补丁。此文件记录准备链 F06–F08，不覆盖数据库修复。

## 行为与边界

- F06：健康检查使用 700ms 单次超时及 5s 探测窗口；`POST /session` 使用独立 5s 超时。客户端禁止重定向，创建只发送一次。创建请求发送后，任何丢失、无效或清理失败的结果保持 `OPENCODE_SESSION_OUTCOME_UNKNOWN`，不降级或自动重试。
- F07：准备不支持非空 default/extra argv 时，保留其明确的 unsupported 结果。只有该结果，或后端明确保证创建请求尚未发送的 `OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE`，允许按原始 New/default/extra argv 启动原生 CLI。通用失败、权限、请求、profile revision 错误及 unknown 不允许降级。降级身份保持未知；不据最新历史推断 ID，因此不承诺此路径可按 ID 恢复或获得即时标题。
- F08：全局收据锁只负责登记、查找和清理。每项 pending 收据有独立等待；相同完整请求复用结果，冲突请求拒绝。活跃文档收据和所有 pending 收据保持固定，只有已撤销文档且已 settled 的收据可清理；panic 固定为 unknown 并唤醒等待者。新请求达到原 4096 容量时，在精确旧收据/冲突检查之后返回明确 pre-create unavailable，使基础 New 仍可降级；不删除旧 unknown，也不发送 POST。创建前、POST 前及返回前重查 caller/shutdown。未将这一重查声明为跨线程原子事务。

## 实际验证

先运行旧逻辑，OpenCode Rust 过滤组确认三个新增 RED：New 仍返回 PREPARE_REQUIRED、原生 argv 降级失败、撤销文档收据占满 REGISTRY_CAPACITY。前端新增两个明确 pre-POST 降级用例先失败（124 passed / 2 failed）。

修复后：

- `npm test -- tests/stores/unifiedWorkspaceRuntime.test.ts` 的等价直接 Vitest 入口：126/126 通过，含 unknown 无第二个 tab / 一次 prepare。`npm run typecheck` 完整入口通过。首次直接运行 vue-tsc 缺失 referenced node build 输出；项目入口生成后通过，未修改断言。
- `cargo test --locked --offline --lib Opencode_ -- --nocapture`：46 项显示通过，其中两项显式 SKIP（无可用 Python 的并发写探针、未设置官方 shim）。44 项实际执行通过。093 初次失败来自 HTTP fixture 未读取 JSON 请求体就关闭连接可能触发 RST；读取 Content-Length 和正文后，新 helper 的 1.1 秒响应通过，未放宽断言或修改创建时限换绿。
- 093 是真实 loopback HTTP 层对照：旧等价 700ms client 在服务端已接收一个 POST 后超时；新 helper 同样 1.1 秒响应成功并解析未知附加字段。它不是完整旧 Desk 进程复演。
- 094 是真实 loopback HTTP 层对照：旧 reqwest 默认 307 策略实际发送两个 POST；新 helper 禁重定向，仅一个 POST，结果 unknown。
- 095 证实 POST 前 caller 拒绝时没有 TCP 连接；096 证实同项等待不阻塞其他请求、unknown 缓存及完整 request 冲突；097 证实 revoked pending 保留、settled 后回收；091/098 另补 RED（6 passed / 2 failed），确认活跃文档容量满时会阻断基础启动；最小修复保持 4096 上限与 exact unknown/冲突优先，仅对尚未登记的新请求返回 pre-create unavailable。换文档后已撤销 settled 收据恢复准入。修改后 `cargo test --locked --offline --lib Opencode_Preparation -- --nocapture` 8/8 实际执行通过（3.38s，无 SKIP）。
- 单独设置当前子进程的 `CC_DESK_OPENCODE_TEST_SHIM=D:\nodejs\node_global\opencode.cmd` 后，`Opencode_OfficialApiCreatesSessionThroughSelectedShim_03` 实际运行 1/1 通过（2.02s），经已安装官方 wrapper 创建 session、校验返回目录并从隔离 SQLite 读回同 ID，helper 退出及端口释放检查通过。未调用模型。

官方 CLI fixture 仅继承 OS 启动变量白名单，排除用户 OPENCODE 覆盖、模型凭证及 NODE_OPTIONS；HOME/USERPROFILE/OPENCODE_TEST_HOME 与 XDG 四目录全部设置为 Temp。按已审查 v1.19.0 源码禁用自动升级、模型列表下载、默认插件、外部 skills、LSP 下载、Claude/项目配置和 channel DB。未安装、未修改全局配置。

旧二进制、macOS/Linux helper 进程树、真实登录后 TUI create/resume/stop/restart 四格、WebView2、完整 Windows 基准套件不在本轮证据内。Python 并发探针仍未验证。原有完整 Windows 305 失败不能由这些定向通过改称全量成功。
