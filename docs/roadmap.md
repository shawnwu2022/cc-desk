# CC Desk 开发路线图

本文档记录 CC Desk 的开发进度和下一步规划。

---

## 当前版本：v0.6.0

### 已完成功能

| 功能模块 | 状态 | 说明 |
|---------|------|------|
| 终端集成 | ✅ 完成 | xterm.js + portable-pty 直连 Claude CLI |
| 多会话管理 | ✅ 完成 | 多终端并行、Tab 切换、状态监控 |
| Hook 监控 | ✅ 完成 | 11 个 hook 事件采集、Tab 状态指示 |
| 项目选择 | ✅ 完成 | 项目列表、最近项目、隐藏项目 |
| 配置读取 | ✅ 完成 | Skills/Agents/MCP/Plugins/Hooks 只读展示 |
| 环境变量 | ✅ 完成 | GUI 管理环境变量并同步到 Claude settings |
| 快捷键系统 | ✅ 完成 | 三场景输入处理、全局快捷键 |
| 自动更新 | ✅ 完成 | GitHub Releases 发布流程 |
| 日志系统 | ✅ 完成 | FileLogger、文件轮转、级别策略 |

### 当前局限

| 问题 | 影响 |
|------|------|
| **配置只读展示** | Skills/MCP/Agents/Plugins 无法编辑，日常可用性低 |
| **无预设库** | 用户需要手动配置，开箱即用体验不足 |
| **无快速配置** | 新用户配置门槛高，需要了解 Claude 配置格式 |
| **多实例置顶/存档/别名 stale-write（已修复）** | 多进程并发改 projects.json 走独立 `projects.json.lock` 跨进程排他锁 + pin/unpin/archive/restore/set_display_name 5 增量 async command（`spawn_blocking` 内锁定读最新→canonicalize→应用→原子写），Windows 用 `ReplaceFileW` 覆盖已有文件；前端 `opLock` 串行完整 action/reload request + apply。详见 [multi-instance spec](superpowers/specs/2026-07-15-multi-instance-stale-write-design.md) |

---

## 下一步开发：配置管理功能

### 目标定位

将 CC Desk 设定为 **"开箱即用的 Claude 软件"**：

- 从"只读展示"升级到"可编辑管理"
- 提供常用预设库，降低配置门槛
- 保持"CLI 优先，GUI 增强"的设计原则

### 设计原则

1. **透明性**：所有配置直接修改 Claude 原生配置文件，用户可通过 CLI 验证
2. **轻量性**：不做独立数据存储层，只作为配置文件的 GUI 编辑器
3. **一致性**：与 `claude` CLI 命令行为保持一致
4. **渐进性**：从只读展示逐步过渡到可编辑，优先满足高频需求

---

## 分阶段开发计划

### 阶段 1：MCP Server 管理（优先级最高）

**目标**：实现 MCP Server 的添加/编辑/删除功能，这是最常用且最有价值的配置管理需求。

**功能范围**：
- MCP Server 的 CRUD 操作
- MCP 预设库（50+ 常用预设）
- JSON 配置编辑器
- 命令验证（检查命令是否在 PATH 中可用）
- Windows 平台 npx/npm 命令自动包装

**新增文件**：

| 文件路径 | 说明 |
|---------|------|
| `src-tauri/src/mcp_config.rs` | MCP 配置读写模块 |
| `src/components/mcp/McpFormModal.vue` | MCP 添加/编辑表单 Modal |
| `src/components/mcp/McpPresets.vue` | MCP 预设选择面板 |
| `src/components/common/JsonEditor.vue` | 通用 JSON 编辑器组件 |
| `src/config/mcpPresets.ts` | MCP 预设数据 |

**修改文件**：

| 文件路径 | 说明 |
|---------|------|
| `src-tauri/src/commands.rs` | 新增 `upsert_mcp_server`、`delete_mcp_server`、`validate_mcp_command` 命令 |
| `src-tauri/src/lib.rs` | 注册新命令 |
| `src/components/mcp/McpPanel.vue` | 添加 "+" 按钮、编辑/删除操作入口 |
| `src/components/mcp/McpItem.vue` | 添加编辑/删除按钮（hover 显示） |
| `src/api/tauri.ts` | 新增 MCP CRUD API 函数 |
| `src/stores/sidebar.ts` | 新增 MCP 操作状态管理 |

**技术实现要点**：

1. **配置文件操作**：
   - 读取/写入 `~/.claude.json` 的 `mcpServers` 字段
   - 支持 stdio、http、sse 三种类型
   - Windows 平台自动处理 `npx`/`npm` 命令包装（`cmd /c npx ...`）

2. **Rust 后端命令**：
   ```rust
   // MCP 配置读写
   pub fn read_mcp_servers_map() -> Result<HashMap<String, Value>>
   pub fn upsert_mcp_server(id: &str, spec: Value) -> Result<bool>
   pub fn delete_mcp_server(id: &str) -> Result<bool>

   // 命令验证
   pub fn validate_command_in_path(cmd: &str) -> Result<bool>
   ```

3. **前端组件架构**：
   - `McpFormModal.vue`：全屏表单面板，包含 ID、类型选择、预设、JSON 编辑器
   - `McpPresets.vue`：预设选择器，点击后填充表单
   - `JsonEditor.vue`：基于 textarea 的 JSON 编辑器，支持语法高亮和校验

---

### 阶段 2：Skills 安装与管理

**目标**：实现 Skills 的安装、卸载、更新功能。

**功能范围**：
- 从 GitHub 仓库安装 Skill
- 从 ZIP 文件安装 Skill
- 卸载 Skill（自动备份到 `~/.claude/skills-backups/`）
- 检查并更新 Skill
- 跨项目同步开关

**新增文件**：

| 文件路径 | 说明 |
|---------|------|
| `src-tauri/src/skills_manager.rs` | Skills 管理模块 |
| `src/components/skills/SkillsManager.vue` | Skills 管理面板 |
| `src/components/skills/SkillInstallModal.vue` | Skill 安装 Modal |
| `src/components/skills/SkillDiscovery.vue` | Skills 发现/推荐页面 |
| `src/hooks/useSkills.ts` | Skills 操作 hooks |
| `src/config/skillPresets.ts` | Skills 预设推荐 |

**修改文件**：

| 文件路径 | 说明 |
|---------|------|
| `src-tauri/src/commands.rs` | 新增 Skill 安装/卸载/更新命令 |
| `src-tauri/src/lib.rs` | 注册新命令 |
| `src/components/skills/SkillsPanel.vue` | 添加管理入口按钮 |
| `src/api/tauri.ts` | 新增 Skills API |

**技术实现要点**：

1. **Skills 目录结构**：
   - 用户级：`~/.claude/skills/<skill-name>/SKILL.md`
   - 项目级：`<project>/.claude/skills/<skill-name>/SKILL.md`

2. **安装流程**：
   - GitHub 仓库：`git clone` 到 skills 目录
   - ZIP 文件：解压到 skills 目录
   - 验证 `SKILL.md` 文件存在

3. **卸载流程**：
   - 移动到备份目录 `~/.claude/skills-backups/<timestamp>-<name>`
   - 不直接删除，保留恢复能力

4. **更新流程**：
   - 检查 GitHub 仓库最新 commit
   - 执行 `git pull` 更新

---

### 阶段 3：Agents 管理

**目标**：实现用户级 Agent 的创建/编辑/删除。

**功能范围**：
- 创建新 Agent（基于模板）
- 编辑 Agent 的 MD 文件内容
- 删除 Agent（备份机制）
- Agent 模板库

**新增文件**：

| 文件路径 | 说明 |
|---------|------|
| `src-tauri/src/agents_manager.rs` | Agents 管理模块 |
| `src/components/agents/AgentFormModal.vue` | Agent 创建/编辑 Modal |
| `src/components/common/MarkdownEditor.vue` | Markdown 编辑器 |
| `src/config/agentTemplates.ts` | Agent 模板 |

**修改文件**：

| 文件路径 | 说明 |
|---------|------|
| `src-tauri/src/commands.rs` | 新增 Agent CRUD 命令 |
| `src-tauri/src/lib.rs` | 注册新命令 |
| `src/components/agents/AgentsPanel.vue` | 添加 "+" 按钮 |
| `src/components/agents/AgentItem.vue` | 添加编辑/删除按钮 |
| `src/api/tauri.ts` | 新增 Agents API |

**技术实现要点**：

1. **Agent 文件格式**：
   - 位置：`~/.claude/agents/<agent-name>.md`
   - 支持 YAML frontmatter（description、model 等）

2. **创建流程**：
   - 提供模板：通用 Agent、Plan Agent、Code Review Agent 等
   - 用户编辑名称和内容
   - 保存为 MD 文件

---

### 阶段 4：Plugins 管理

**目标**：实现 Plugin 的安装、卸载、启用/禁用。

**功能范围**：
- 安装 Plugin（从 GitHub 或官方仓库）
- 卸载 Plugin
- 启用/禁用 Plugin
- Plugin 推荐库

**技术实现要点**：

利用 Claude CLI 命令（而非直接操作文件）：
- `claude plugins install <repo>`
- `claude plugins uninstall <id>`
- `claude plugins enable/disable <id>`

**新增文件**：

| 文件路径 | 说明 |
|---------|------|
| `src-tauri/src/plugins_manager.rs` | Plugins 瑞管理模块（CLI 命令封装） |
| `src/components/plugins/PluginsManager.vue` | Plugins 管理面板 |
| `src/config/pluginPresets.ts` | Plugins 推荐 |

**修改文件**：

| 文件路径 | 说明 |
|---------|------|
| `src-tauri/src/commands.rs` | 新增 Plugin 安装/卸载命令 |
| `src/components/plugins/PluginsPanel.vue` | 添加管理入口 |
| `src/components/plugins/PluginItem.vue` | 添加启用/禁用开关 |

---

### 阶段 5：配置预设与快速切换

**目标**：提供常用配置预设，实现一键配置。

**功能范围**：
- MCP 预设库扩展（50+ 常用 MCP）
- Skills 预设库（常用 Skills 推荐）
- 配置模板（开发环境、写作环境、研究环境等）
- 一键导入预设

**新增文件**：

| 文件路径 | 说明 |
|---------|------|
| `src/components/common/PresetsGallery.vue` | 预设画廊组件 |
| `src/components/settings/sections/PresetsSection.vue` | 设置页预设选择 |
| `src/config/configTemplates.ts` | 配置模板 |

**扩展文件**：

| 文件路径 | 说明 |
|---------|------|
| `src/config/mcpPresets.ts` | 添加更多 MCP 预设（50+） |
| `src/config/skillPresets.ts` | Skills 预设推荐 |

---

### 阶段 6：高级功能（可选）

**目标**：增强用户体验的高级功能。

**功能范围**：
- 配置备份与恢复
- 多配置文件切换（工作/个人）
- 导入/导出配置
- 配置同步提示（提醒用户可用 iCloud/Dropbox 同步）

**新增文件**：

| 文件路径 | 说明 |
|---------|------|
| `src-tauri/src/config_backup.rs` | 配置备份模块 |
| `src/components/settings/sections/BackupSection.vue` | 备份管理面板 |
| `src/components/settings/sections/ImportExportSection.vue` | 导入导出面板 |

---

## 优先级排序与依赖关系

```
阶段 1 (MCP 管理) ──┐
                    │
阶段 2 (Skills)    │──> 阶段 5 (预设与切换)
                    │
阶段 3 (Agents)    │
                    │
阶段 4 (Plugins)   ──┘

阶段 6 (高级功能) [可选，依赖阶段 1-4]
```

**建议实现顺序**：

1. **阶段 1 - MCP 管理**（最高优先级）
   - 高频需求，配置格式相对简单
   - 可立即提升日常可用性

2. **阶段 2 - Skills 管理**（高优先级）
   - 社区活跃，Skills 生态丰富
   - 需要阶段 1 的基础设施（JsonEditor 等）

3. **阶段 3 - Agents 管理**（中优先级）
   - 复用阶段 2 的 MarkdownEditor

4. **阶段 4 - Plugins 管理**（中优先级）
   - 利用 CLI 命令，实现相对简单

5. **阶段 5 - 配置预设**（完善阶段）
   - 扩展阶段 1-4 的预设库

6. **阶段 6 - 高级功能**（可选）
   - 根据用户反馈决定是否实现

---

## 与 cc-switch 的差异化定位

| 功能 | cc-switch | CC Desk |
|------|-----------|--------|
| 跨应用同步 | 支持 Claude/Codex/Gemini/OpenCode 等 | 仅聚焦 Claude Code |
| 配置存储 | 独立的 `~/.cc-switch/` SQLite 数据层 | 直接操作 Claude 原生配置文件 |
| 云同步 | WebDAV/iCloud 等方案 | 暂不实现（保持轻量） |
| 供应商管理 | API Key 切换、代理配置 | 暂不实现（CLI 已有） |
| 设计理念 | 多工具统一管理面板 | CLI 优先，GUI 增强 |

**核心差异**：
- CC Desk 保持对 Claude 原生配置文件的直接操作，用户可随时回归纯 CLI
- 不引入独立数据层，保持轻量透明
- 聚焦 Claude Code，而非多工具管理

---

## 关键文件清单

实现配置管理功能需要重点关注的现有文件：

| 文件 | 作用 |
|------|------|
| `src-tauri/src/commands.rs` | 添加所有 CRUD 命令的核心入口 |
| `src-tauri/src/store.rs` | 现有配置读取逻辑，需扩展写入能力 |
| `src/components/mcp/McpPanel.vue` | MCP 面板，需添加管理功能入口 |
| `src/api/tauri.ts` | API 层，需新增所有 CRUD 函数 |
| `src/stores/sidebar.ts` | 数据状态管理，需添加操作状态跟踪 |

---

## 参考资源

- **cc-switch 项目**：https://github.com/farion1231/cc-switch
- **Claude Code 文档**：https://code.claude.com/docs/llms.txt
- **MCP 规范**：https://modelcontextprotocol.io/

---

## 更新记录

| 版本 | 日期 | 更新内容 |
|------|------|---------|
| v0.6.0 | 2025-05-11 | 完成 Hook 监控、环境变量管理、UTF-8 编码修复 |
| - | 2025-05-11 | 制定配置管理功能开发路线图 |
