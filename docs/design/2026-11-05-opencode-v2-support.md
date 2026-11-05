# OpenCode v2 支持设计

日期：2026-11-05。状态：设计提案，未实施；本文不宣布任何 v2 能力已完成。断裂面证据来自本机 v2.2.5 实测与官方 v2 文档（2026-11-05 查证）。

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

- **L0 披露（先行）**：支持矩阵与 README 声明"已认证 v1.x；v2 已知断裂未支持"，列出用户可见症状与原因，避免静默失效被当 bug 追。
- **L1 存储适配（比预想轻，主菜）**：读取器改读 `session_v2`（v1 列超集，`session` 表旧数据可继续读或按 v2 迁移语义处理）；归属逻辑（project/directory/`ses_` id）不变；`parent_id IS NULL` 语义需按 v2 复核（subagent 排除）；fixture 用 v2 真实 db dump。
- **L2 prepare v2**：v2 serve/client 契约调研（`@opencode/client` 或直接 HTTP）；确认 Desk 私有 prepare 与 v2 背景服务（`--standalone`）的关系；保持"创建请求不重放"语义。
- **L3 observer v2**：按 `Plugin.define` 包结构重写观察插件；hooks 列表调研（v2 文档 `/build/plugins/server` 子页）；保持"仅状态元数据、不读正文"的边界与 transport 重试语义。
- **L4 rename v2**：v2 metadata API 契约适配。
- 每层独立验收；最终以真实 v2 CLI 的 Layer C 认证收口，Windows WebView 验收不可省。

## 检测与披露原则

遵循项目既有方向（0.26.0-rc.1 已将 Codex 准入从版本字符串改为同 run 结构 + 认证事件）：**不做 CLI 版本字符串探测**。升级场景的披露用契约/结构特征（如 prepare v1 契约失败 + observer 不 announce 的组合），具体特征在 L2/L3 实施期定。

## 开放问题

- v2 server plugin hooks 的完整列表与参数形态（`/build/plugins/server`）。
- v2 背景服务与 Desk 私有 prepare server 的生命周期交互。
- 另一台机器"关闭消失"的确切归因（需那边版本与路径数据）。
- v2 `session` 子命令（Manage sessions）是否提供比 HTTP API 更稳的受控面。
