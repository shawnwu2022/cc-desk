# OpenCode v2 支持设计

日期：2026-11-05。状态：L1 存储/ L2 prepare+rename 已实施（106a3def、4a8e418a、7da6854c），L3 以 interim db watcher 过渡（152203f1+116dc405）；**真实 v2 CLI 的 Layer-C 认证收口与 Windows WebView 验收未做**。断裂面证据来自本机 v2.2.5 实测与官方 v2 文档（2026-11-05 查证）。

## 背景

`@opencode/cli` 2.0.0 于 2026-09-11 起 npm stable；用户环境已升级（本机 v2.2.5，npm 全局）。Desk 的 OpenCode 集成（2026-10-20 收口，随 0.24 发布）按 v1.19 开发验收。官方迁移文档明确三项有意 breaking：**v1 plugin 不再运行**、**server API 换新契约**、TUI 配置从 `tui.json(c)` 改为全局 `cli.json`。

## 断裂与兼容矩阵

| 层 | v1 集成现状 | v2 实测/文档事实 | 状态 |
| --- | --- | --- | --- |
| 存储 | `~/.local/share/opencode/opencode.db`，`session`/`project` 表（`opencode_db.rs`） | 路径不变；**v2 迁移把会话写入新表 `session_v2`（v1 列超集：新增 `fork_session_id`/`slug`/`time_idle`/`time_busy`/`idle_outcome` 等），不再写 `session` 表**（本机 2026-11-05 实测：用户 v2 会话只出现在 `session_v2`，`session` 表停留 32 条 v1 旧数据）；新增 `worktree`/`session_inbox`/`session_part`/`blob`/`kv`/`job`/`schedule`/`instruction_*` 表；`session_input` 表被移除；`project` 表保留且 `project_id` 仍关联；会话 id 仍为 `ses_` 前缀；db 路径随 release channel 变化（`opencode debug paths db`） | **断裂（表级）**：Desk 读 `session` 表，v2 会话完全不可见——"关闭后消失"即此 |
| CLI 启动 | TUI 以 `-s <id>` 启动 | `--session, -s` 保留且语义扩展（"Session ID to continue, or to create if it does not exist"）；新增 `--standalone`/`--server` 背景服务模型 | 兼容，待验收 |
| prepare | `opencode serve` + v1 `POST /session` + `Health{healthy}` | `serve` 子命令存在但为 **v2 API**；官方要求用 `@opencode/client` 新契约；认证改 `OPENCODE_PASSWORD`，用户名固定 `opencode` | 断裂 |
| 观察 | v1 plugin 形态 `export const CCDeskObserver = async (input, options)`（`opencode_observer.mjs`），env 注入 | v1 plugin 实现在 v2 不运行；v2 为 `@opencode/plugin` 包结构 + `Plugin.define({id, setup(ctx)})`，`exports` 分 `./server`/`./tui`/`./gui-experimental`/`./rpc` | 断裂 |
| rename | v1 scoped HTTP metadata API | v2 API 新契约 | 断裂 |

**v2 侧命名与数据本身正常**（实测：用户会话 `ses_e56cac…` 由 CLI 成功命名为 "Ready acknowledgment"，`session_v2` 的 `title`/`time_idle`/`idle_outcome` 均有值）；问题全部在 Desk 的 v1 读取面。

## 用户症状归因（2026-11-04/05 报告）

- **会话名无效**：plugin 断裂 → 无认证身份事件 → tab 拿不到身份，标题不继承历史标题；prepare 断裂 → 降级 unknown-identity new 启动。
- **关闭后消失**：**已实锤（本机 2026-11-05 复现）**——v2 会话写入 `session_v2` 表，Desk 读 v1 `session` 表，会话对 Desk 完全不可见；CLI 侧命名与数据正常。另一台机器无需再归因，同一机制。

## 适配方案（分层交付）

- **L1 存储适配（✅ 2026-11-05 已完成，106a3def）**：`session_table` 探测——`session_v2` 存在即代表迁移完成只读它，纯 v1 库回退 `session` 表；不与 v1 镜像合并去重（重复行比缺行更糟）；title 兼容 NULL 落空串。fixture 为本机 dump 的真实 v2 DDL；回归 27（迁移形态）/28（v1 回退）。
- **L0 披露（随 L2/L3 一并）**：支持矩阵与 README 声明"已认证 v1.x；v2 历史读取已支持，prepare/观察/rename 待 L2-L4"。
- **L2 prepare v2**：`POST /api/session`（OpenAPI 实测支持预指定 id、title、location，见 `docs/research/opencode-v2.2.5-openapi.json`）；v2 认证为 basic（用户名固定 `opencode`，serve 自动生成密码打印于 stderr）；保持"创建请求不重放"语义；确认 Desk 私有 prepare server 与 v2 背景服务（`--standalone`）的生命周期关系。
- **L3 observer v2（方向已定：优先 API 事件流而非 plugin）**：`GET /api/event` 提供全量事件订阅（`session.created`、`session.rena*`、`location.shutdown` 等），相对 plugin 路线上游演进更稳——plugin API 在 v1→v2 已断一次，API 契约有 OpenAPI 版本化；`Plugin.define` 形态仅作 API 不可行时的备选。保持"仅状态元数据、不读正文"边界与 transport 重试语义。
- **L4 rename v2**：`PATCH /api/session/{sessionID}`。
- 每层独立验收；最终以真实 v2 CLI 的 Layer C 认证收口，Windows WebView 验收不可省。
- **全 CLI 健壮性（用户 2026-11-05 要求，独立调研任务）**：参考同类开源项目的多版本兼容策略（10-23 调研已覆盖竞品定位，未覆盖其版本兼容机制）；核心方向是把对上游的依赖从易断的 plugin/hook 内部机制迁向带 OpenAPI 契约的公开 API，并保留契约探测与显式降级披露。

## 检测与披露原则

遵循项目既有方向（0.26.0-rc.1 已将 Codex 准入从版本字符串改为同 run 结构 + 认证事件）：**不做 CLI 版本字符串探测**。升级场景的披露用契约/结构特征（如 prepare v1 契约失败 + observer 不 announce 的组合），具体特征在 L2/L3 实施期定。

## 开放问题

- v2 server plugin hooks 的完整列表与参数形态（`/build/plugins/server`，仅当 API 事件流不可行才需要）。
- v2 背景服务与 Desk 私有 prepare server 的生命周期交互。
- v2 `session` 子命令（Manage sessions）是否提供比 HTTP API 更稳的受控面。
