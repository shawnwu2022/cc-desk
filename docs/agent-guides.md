# Agent 指南加载与维护

CC Desk 以根 [AGENTS.md](../AGENTS.md) 为所有 coding agent 的通用标准。模块特例仍放在对应目录的 `AGENTS.md`；工具专用文件只导入或指向这些标准文件，不复制规则。测试入口和发布边界在根指南统一列出，详细契约在 [project-contracts.md](project-contracts.md) 及专题文档维护。

## 文件与作用域

| 标准文件 | 作用 |
| --- | --- |
| `AGENTS.md` | 仓库通用工作方式、架构底线、测试分层与必读索引 |
| `src-tauri/AGENTS.md` | Rust/Windows ConPTY 补充约束 |
| `src-tauri/src/cli/AGENTS.md` | Native CLI backend 的 snapshot、profile、鉴权和进程边界 |
| `docs/project-contracts.md` | 从原根指南完整迁移的项目约束，按任务阅读相关章节 |
| 各目录 `CLAUDE.md` / `GEMINI.md` | 只用 `@./AGENTS.md` 导入同目录标准文件 |
| `.github/copilot-instructions.md` | 要求 Copilot 显式读取标准指南及适用的目录规则 |

修改文件前，必须读取根指南及从仓库根到目标目录的目录指南。工具的自动发现可能取决于启动目录、触及的文件、项目设置或版本；不要只看当前目录，也不要把某个工具的自动发现语义套用到其他工具。

## 各工具入口

以下加载方式按 2026-10-11 的官方文档核对。仓库提供可识别的入口；实际是否启用还取决于工具版本、信任和项目配置。

| 工具 | 本仓库入口 | 加载与范围 |
| --- | --- | --- |
| Codex | 根及目录 `AGENTS.md` | 原生识别；从项目根到当前工作目录合并，每层更具体规则补充根规则。根目录启动后修改 Rust 文件时仍须主动读其目录指南。[官方说明](https://learn.chatgpt.com/docs/agent-configuration/agents-md) |
| Claude Code | 根及目录 `CLAUDE.md` → `@./AGENTS.md` | 使用原生 import，路径相对导入文件；建议从仓库根启动，再由目标文件访问触发目录加载。深层启动时，祖先入口的导入位于工作目录之外，可能受外部导入信任限制；增加祖先 `@` 引用不会绕过批准。保留入口兼容不能直接发现 `AGENTS.md` 的会话。[官方说明](https://code.claude.com/docs/en/memory) |
| Gemini CLI | 根及目录 `GEMINI.md` → `@./AGENTS.md` | 使用原生 Markdown import，无需修改用户的 `context.fileName`；仓库与目录规则仍须按实际工作范围读取。[官方说明](https://geminicli.com/docs/cli/gemini-md/) |
| Cursor | 根及目录 `AGENTS.md` | 原生支持根与嵌套目录指南；不另建重复 `.cursorrules` 或 `.cursor/rules` 规范。[官方说明](https://cursor.com/docs/rules) |
| Windsurf/Cascade | 根及目录 `AGENTS.md` | 根规则始终应用，子目录规则按路径范围应用；当前官方入口位于 Devin Desktop 文档。[官方说明](https://docs.devin.ai/desktop/cascade/agents-md) |
| GitHub Copilot | `.github/copilot-instructions.md`，兼容支持 `AGENTS.md` 的功能 | 官方支持能力随使用界面而异；入口显式要求读取标准文件。Markdown 链接不是通用的自动导入语法。[官方说明](https://docs.github.com/en/copilot/how-tos/copilot-on-github/customize-copilot/add-custom-instructions/add-repository-instructions) |
| 其他工具 | `AGENTS.md` 或工具要求的入口引用它 | 优先使用工具原生支持；若需要别名文件，只添加原生导入或“先读取 AGENTS.md”的指令，并在这里记录支持范围 |

`@./AGENTS.md` 必须放在普通正文中，不能包在反引号或代码块中。普通 Markdown 链接只提供定位，不应当作所有工具都会内联的指令。入口使用普通文件而非 symlink，以兼容 Windows 权限与 checkout。

## 指令大小与详细契约

Codex 默认 `project_doc_max_bytes` 为 32 KiB，限制自动合并的项目指令总量；原根指南约 100 KB，超过该预算。[官方说明](https://learn.chatgpt.com/docs/agent-configuration/agents-md)

因此根指南只保留工作规范、底线和任务索引，完整原文移动到 `docs/project-contracts.md`。后者的代码路径和命令仍以仓库根为基准，Markdown 链接已重新定位。修改对应模块前必须按索引读相关详细约束，但不把整份详细契约自动导入工具入口。

维护目标为根指南不超过 12 KiB、根加当前两层目录指南不超过 20 KiB，为其他指令留出余量。优先把长篇契约放进专题文档并补索引，而不是要求用户提高 Codex 全局预算。不要通过缩短文件删除安全、库存、真实进程或发布门禁。

## 更新规则

1. 通用执行规则改根 `AGENTS.md`；目录特例改当地 `AGENTS.md`；详细行为改对应专题文档或 `project-contracts.md` 的章节。
2. 新增目录指南时，为 Claude Code 与 Gemini CLI 同步添加仅含同目录导入的入口；原生支持 `AGENTS.md` 的工具无需再复制一份规范。
3. 新增测试命令时同步检查 `package.json`、根指南、[测试编写原则](测试编写原则.md)、README 和 [ci-performance.md](ci-performance.md)；快组不能被描述为完整 CI。
4. 检查相对链接、导入目标、作用域和大小预算。去除陈旧的副本口径，保留未运行/目标环境限制，避免把历史执行记录改写成当前通过证据。

## 验证与会话刷新

静态验证应确认：各工具入口存在；所有 `@` 导入指向同目录 `AGENTS.md`；导入不在代码块；Markdown 本地链接实际存在；根与目录指令总量符合预算；迁移后的详细契约保留原正文。无需为纯文档迁移重跑全量 Rust 或真实 CLI。

实际客户端发现必须在新会话中检查，已有会话不一定刷新指令。可用不修改文件的提示：“列出当前适用的仓库指南，说明修改 `src-tauri/src/cli/` 前还需读哪些规则，并给出前端快速组与完整 Vitest 的区别。”

- Codex：分别从仓库根及 `src-tauri/src/cli/` 开始新会话，检查根/目录来源与作用域；从根工作时检查是否主动读取目标目录指南。
- Claude Code：使用 `/memory` 查看文件、`/context` 查看活跃指令，核对根和目录入口及导入目标。
- Gemini CLI：使用 `/memory reload` 与 `/memory show` 核对合并后的根/目录内容。
- Cursor、Cascade 和 Copilot：在实际项目与工具界面查看启用的指令，并通过上面的提示核对读取来源。客户端未提供来源证据时保留证据缺口，不声称通过跨工具运行认证。

本文的静态检查无需调用模型服务；客户端加载验收单独记录工具版本、工作目录和加载来源，不能将加载成功当作模型遵循或真实 CLI Layer-C 认证。

### 本机加载检查（2026-10-11）

- Codex CLI `0.160.0`：原生 `codex debug prompt-input` 生成的上下文完整包含根指南；从 `src-tauri/src/cli` 启动时完整包含三层指南。从根启动不预载未触及的目录指南，与主动阅读目录规则的要求一致。未发送模型请求。
- Claude Code `2.1.293`：以仅回环地址的响应 fixture 检查实际客户端，不转发模型请求。根目录新会话完整加载根指南；一次真实 `Read` 读取 CLI 目录源文件的一行后，下一次上下文完整包含根、Rust 和 CLI 三层指南。检查关闭 hooks/MCP 与会话持久化，仅保留该只读工具；保存指南匹配布尔值和哈希，不保存完整提示或认证头。
- 同一 Claude 客户端从 CLI 深层目录启动时，当前目录标准完整加载，祖先标准的导入尚未获外部导入信任批准，未进入上下文。这符合[外部导入的信任边界](https://code.claude.com/docs/en/memory#import-additional-files)。推荐从仓库根启动；选择深层启动时，在客户端检查 `/memory` 和 `/context` 并由用户处理其信任提示，不自动修改全局批准状态。
- Gemini CLI `0.63.0`：直接调用该安装包实际的目录发现、文件导入与按目录补加载函数。默认 `GEMINI.md` 从根加载一层、从 CLI 目录加载三层，根会话访问 CLI 文件时补齐两层；每份标准正文均完整匹配。此处验证的是已安装 loader，不是已认证模型会话。
- Cursor、Cascade 与 Copilot 的实际界面加载仍未验证，保留上方官方入口与操作步骤。静态链接或另一工具通过不能替代各客户端证据。
