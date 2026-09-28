# CC Desk 统一工作区与全应用交互重构设计规格

- 日期：2026-09-28
- 状态：设计冻结，等待用户书面规格复核
- 实施分支：`feat/unified-workspace-ux`
- 基线：`7e31cff37b199ba3b10cadf2fe038ed03fed45c2`
- 适用范围：桌面端全部用户交互页面、会话导航、终端承载、设置、菜单、弹层、反馈与视觉规范

## 1. 摘要

CC Desk 的 Native CLI v3 已具备 Claude Code 与 Codex CLI 的底层运行、输入输出、恢复、隔离和安全能力，但当前产品界面将 Native CLI 暴露为独立工作区，并在其中再次提供项目选择、Profile、启动参数和第二套 Tab。该结构对用户呈现了底层实现，而不是符合人类阅读习惯的工作模型。

本次重构将产品统一为一个清晰的心智模型：

> 一个项目下面有多个 Claude Code 或 Codex CLI 会话。左侧选择会话，中间继续工作，右侧按需查看项目资源。

本设计同时统一所有交互页面，保留现有“工匠终端”视觉基调和独立终端主题，不重写已经验证的 Native CLI runtime、安全边界与传输协议。

## 2. 现状问题

当前结构存在以下产品层问题：

1. `App.vue` 将 `welcome`、`projects`、`terminal`、`native` 作为平行顶层视图。
2. `NativeCliWorkbench.vue` 在独立页面中再次维护项目、Profile、启动方式、Tab、状态和资源。
3. 用户必须理解 Native、Legacy、Profile、Revision、Generation、Session ID、Raw argv 等技术概念。
4. Claude 与 Codex 会话未统一进入现有“项目 → 会话”结构。
5. 顶部工具条横向堆积大量控件，信息密度高但操作层级不清晰。
6. 资源面板直接输出 JSON，错误区域直接显示内部错误码。
7. 欢迎页、项目页、旧终端工作区、Native 工作区与设置页的布局和反馈模式不一致。
8. 功能测试通过无法保证页面可理解、可扫描、无重叠或适应 Windows 缩放。

## 3. 目标

### 3.1 用户目标

用户首次打开应用后，应能在无需说明文档的情况下完成：

1. 添加项目。
2. 在项目下创建 Claude Code 或 Codex CLI 会话。
3. 在左侧切换会话。
4. 识别会话状态、使用工具和最近活动时间。
5. 重命名、停止、重启、关闭、归档和恢复会话。
6. 恢复历史会话。
7. 按需查看项目资源。
8. 修改 GUI 和终端主题。

### 3.2 产品目标

- 项目是第一层，会话直接位于项目下面。
- Claude 与 Codex 会话混排，不增加 CLI 中间分组。
- 左侧会话树是唯一的 Tab 系统。
- 主界面不显示底层技术字段。
- 高频操作直接可用，低频操作进入二级菜单。
- 所有页面使用同一应用外壳、组件、反馈和文字规范。
- 终端始终是视觉和操作主角。

### 3.3 工程目标

- 复用现有 Legacy Claude 与 Native CLI 运行实现。
- 建立统一会话投影和适配器，而不是一次性破坏性迁移。
- 不削弱 Native CLI 的认证、run identity、no-replay、ACK、observer 和资源 scope 边界。
- 保持旧 Claude 会话、旧 Native workspace、终端主题和快捷键兼容。
- 开发阶段减少 CI 触发，冻结后执行集中验证。

## 4. 非目标

本次不做：

- Provider 或 API Key 管理。
- Claude Code / Codex CLI 的安装器或版本管理器。
- 重写 CLI 的对话、slash command、审批、认证或扩展运行时。
- 强制迁移或删除旧 Claude 会话数据。
- 删除 CLI 原生会话文件。
- 把项目资源抽屉变成原生 CLI 配置写入器。
- 通过本次 UI 重构恢复正式发布或 updater promotion。
- 按 Claude/Codex 分别增加独立终端主题；本次保留全局终端偏好。

## 5. 冻结的产品原则

1. **人类语言优先**：普通界面使用“启动配置”“恢复会话”“项目资源”等自然语言。
2. **技术细节后置**：ID、Revision、Generation、Runtime 等只在诊断详情出现。
3. **一个主要入口**：同一操作不在多处重复创造不同流程。
4. **一个列表即一个 Tab 系统**：左侧会话树承担切换职责，终端上方不再出现第二套 Tab。
5. **渐进披露**：普通新建只选择 Claude 或 Codex；高级参数逐层展开。
6. **布局稳定**：Hover、加载、错误和快捷按钮出现时不推动标题或改变行高。
7. **终端优先**：GUI 外壳克制，中央终端获得最大可用空间。
8. **安全边界不退化**：界面简化不得换取身份、权限或 no-replay 规则的削弱。

## 6. 视觉方向

保留现有“工匠终端（Artisan Terminal）”基调：

- 温暖米灰 / 深炭背景。
- 墨蓝表示可操作。
- 琥珀表示当前激活或选中。
- 暖色中性色、低饱和状态色。
- 静态表面平铺，阴影仅用于临时浮层。
- 4px 基础间距体系。
- 小而克制的圆角。
- 系统字体，不引入外部字体下载。
- 统一 `150ms ease` 微交互，禁止 `transition: all`。

终端配色是独立主题层，不随 GUI 主题强制改变。

## 7. 全局信息架构

### 7.1 一级入口

全局导航只保留：

1. 工作区
2. 项目
3. 设置

底部可保留帮助/关于入口和更新提示。

Skills、Agents、MCP、Plugins、Instructions 不再作为全局一级导航，它们属于当前项目/会话的上下文资源。

### 7.2 统一应用外壳

```text
┌──────────────────────────────────────────────────────────────┐
│ 当前项目 / 当前会话                                窗口控制 │
├──────┬────────────────┬──────────────────────┬───────────────┤
│ 导航 │ 项目与会话     │ 当前主要内容         │ 上下文抽屉    │
│ 44px │ 默认 288px     │ 自动占满             │ 默认关闭      │
│      │                │                      │ 默认 344px    │
└──────┴────────────────┴──────────────────────┴───────────────┘
```

### 7.3 顶层视图

顶层只保留：

- 工作区
- 项目管理
- 设置

不再保留独立的欢迎页产品路径和 Native CLI 顶层产品路径。首次使用通过统一工作区中的空状态完成引导。

## 8. 工作区设计

### 8.1 项目与会话栏

顶部包含：

- 标题“会话”。
- 添加项目按钮。
- 项目/会话搜索框。

不包含 CLI 切换、Profile、Resume scope、Session ID、Raw argv 或项目下拉框。

### 8.2 项目行

```text
[展开] [项目名称] [新建会话] [更多]
```

- 默认高度：38–40px。
- 默认宽度：288px，可在 240–360px 内调整。
- 项目名称单行省略，Tooltip 显示完整路径。
- 唯一行内高频操作为“新建会话”。
- 固定、重命名、隐藏、打开文件夹、移除等进入二级菜单。

### 8.3 会话行

最终结构：

```text
[状态图标] [应用图标] [会话标题] [时间/快捷操作] [更多]
```

推荐网格：

```css
grid-template-columns: 16px 18px minmax(0, 1fr) 38px 20px;
column-gap: 6px;
```

- 行高：36–38px。
- 只有标题允许省略。
- 状态、应用图标、尾部区域和菜单不能被标题挤出。
- 当前选中使用 3px 琥珀指示线、弱选中背景和略高标题字重。
- 不显示 Native、Legacy、Profile、Revision、Generation 等字段。

### 8.4 状态图标

会话列表不显示状态文字，状态完全由高辨识度 SVG 图标表达，颜色只作为辅助。

| 状态 | 轮廓 | 动效 |
|---|---|---|
| 启动中 | 缺口圆环 | 缓慢旋转 |
| 运行中 | 圆形内活动波形或播放形 | 静止 |
| 需要回复 | 对话气泡内实心点 | 首次进入时轻提示一次 |
| 状态确认中 | 菱形/圆形内问号 | 弱呼吸 |
| 已结束 | 方形停止符 | 静止 |
| 失败 | 三角形感叹号 | 静止 |

每个图标必须提供 Tooltip、键盘焦点提示和 `aria-label`。开启 `prefers-reduced-motion` 时关闭非必要动画。

### 8.5 Claude/Codex 应用图标

列表中使用应用图标，不显示 Claude、Codex、CC、CX、CLA 或 CDX 文本。

- 图标大小：15–16px。
- 应用图标使用低饱和品牌色或跟随主题的单色版本。
- 应用图标不随运行状态变化颜色。
- Tooltip 显示完整名称。
- 图标不可用时回退为 `CC` / `CX`。
- 仅使用仓库明确拥有使用权的资源；若官方资源授权不明确，使用 CC Desk 自有的中性 Claude/Codex 识别图形。

### 8.6 极简时间

时间表示最近一次有效会话活动：用户输入、CLI 输出、注意力事件、恢复、启动或状态变化。

| 时间差 | 显示 |
|---|---|
| 1 分钟内 | `刚刚` |
| 1 小时内 | `6m` |
| 24 小时内 | `3h` |
| 90 天内 | `6d` |
| 同一年更早 | `9/27` |
| 跨年 | `25/12/20` |

时间右对齐，使用 tabular numbers，Tooltip 显示完整本地日期时间。全应用共享一个时间刷新器，不为每个会话创建独立定时器。

### 8.7 快捷操作

默认尾部显示时间。Hover 或键盘聚焦时，在同一固定区域显示当前状态下唯一最高频操作和二级菜单；标题、状态和行宽不得移动。

| 状态 | 唯一高频操作 |
|---|---|
| 启动中 | 取消启动 |
| 运行中 | 停止 |
| 需要回复 | 无；整行点击即进入会话 |
| 状态确认中 | 确认状态 |
| 已结束 | 恢复 |
| 失败 | 重试 |
| 已归档 | 恢复归档 |
| 重命名中 | 保存 |

若没有明确高频操作，保留时间并只显示 `⋯`。重命名、重启、归档、关闭等全部能力保留在二级菜单/右键菜单中。

### 8.8 会话完整操作

会话必须完整支持：

- 重命名
- 停止
- 重启
- 关闭
- 归档
- 恢复归档
- 恢复历史会话
- 复制会话 ID
- 打开项目目录
- 查看诊断信息

行为定义：

- **重命名**：仅修改 CC Desk 显示名称，不修改原生 Session ID，不重启。
- **停止**：结束当前进程，保留会话记录。
- **重启**：使用同一项目、CLI、启动配置和启动方式创建新的 run/generation；状态不明时先确认，不得重复 spawn。
- **关闭**：从当前打开列表移除。若仍运行，先确认并停止；保留可恢复历史。
- **归档**：从普通列表隐藏并保留索引。运行中归档必须明确“停止并归档”。
- **恢复归档**：恢复到普通列表，可选择仅恢复列表或恢复并打开。

右键菜单与 `⋯` 菜单必须使用相同动作模型和可见性规则。

## 9. 统一会话架构

### 9.1 统一模型

```ts
type CliKind = 'claude' | 'codex'
type RuntimeKind = 'legacy-claude' | 'native-cli'
type ProcessState = 'starting' | 'running' | 'unknown' | 'stopped' | 'failed'
type AttentionState = 'none' | 'needs-user'

interface UnifiedSession {
  id: string
  projectKey: string
  projectPath: string
  cli: CliKind
  runtime: RuntimeKind
  title: string
  processState: ProcessState
  attentionState: AttentionState
  lastActivityAt: number
  archived: boolean
  resumable: boolean
  adapterSessionId: string
  nativeSessionId?: string
  launchConfigId?: string
  safeErrorCode?: string
}
```

`runtime`、adapter ID 和安全错误码仅用于内部逻辑，不进入普通会话行。

### 9.2 状态派生

进程状态与注意力状态分离。例如：

```text
processState = running
attentionState = needs-user
```

界面优先显示“需要回复”图标，但真实进程仍被记录为运行中。

### 9.3 适配器

```ts
interface SessionAdapter {
  listSessions(projectKey: string): Promise<UnifiedSession[]>
  createSession(input: CreateSessionInput): Promise<UnifiedSession>
  resumeSession(input: ResumeSessionInput): Promise<UnifiedSession>
  activateSession(id: string): Promise<void>
  stopSession(id: string): Promise<void>
  restartSession(id: string): Promise<void>
  closeSession(id: string): Promise<void>
  renameSession(id: string, title: string): Promise<void>
  archiveSession(id: string): Promise<void>
  restoreArchivedSession(id: string): Promise<void>
}
```

实现：

- `LegacyClaudeAdapter`
- `NativeCliAdapter`

新 Claude 与 Codex 会话默认使用 Native CLI；旧 Claude 会话继续由 Legacy adapter 支持。

## 10. 统一终端宿主

```text
UnifiedTerminalHost
├─ LegacyClaudeTerminal
└─ NativeCliTerminal
```

规则：

- 点击左侧会话只切换显示，不重启进程。
- 运行中的隐藏终端继续接收输出。
- 重新显示时执行安全 `fit`。
- 不重复恢复同一个原生 Session。
- Native terminal 继续使用 authenticated bridge、exact run/generation、ordered input、bounded output/ACK 和 no-replay 语义。
- stale async completion 不得修改新 generation 的局部 UI 状态。

## 11. 终端主题

终端主题是保留项，且独立于 GUI 主题。

### 11.1 保留内容

- 终端背景和前景。
- ANSI 16 色。
- 光标、选区、链接、搜索高亮。
- 字体、字号、行高、光标样式和闪烁。
- WebGL/Canvas/fallback renderer 的颜色一致性。

### 11.2 组合

必须支持：

- 浅色 GUI + 浅色终端。
- 浅色 GUI + 深色终端。
- 深色 GUI + 浅色终端。
- 深色 GUI + 深色终端。

### 11.3 切换行为

- 纯颜色切换不得重启 CLI、清空缓冲、改变滚动位置或重建终端。
- 字体/字号/行高变化后，当前终端重新测量并 fit。
- 后台终端更新 options，延迟至再次显示时 fit。
- 旧 Claude、Native Claude 和 Native Codex 共享同一全局终端偏好。
- 旧 `terminalTheme` 值必须兼容；失效主题安全回退并提供非阻断提示。

## 12. 新建会话

### 12.1 快捷流程

项目行点击 `＋`：

```text
[Claude 图标] Claude Code
[Codex 图标]  Codex CLI
────────────
恢复以前的会话…
更多选项…
```

普通新建最多两步：点击 `＋`，选择 Claude 或 Codex。

系统自动完成项目注册、启动配置选择、request/run/generation、observer、终端绑定和错误投影。

### 12.2 默认启动配置

优先级：

1. 当前项目 + 当前 CLI 最近一次成功使用的启动配置。
2. 当前 CLI 默认启动配置。
3. 安全默认配置。
4. 无法生成时进入明确的配置引导。

普通流程不显示 Profile 概念。

### 12.3 高级对话框

字段纵向排列：

- 项目
- 工具
- 启动方式
- 启动配置
- 权限设置
- 更多选项
- 开发者选项

启动方式包括：新会话、恢复以前的会话、CLI 原生恢复列表、按 Session ID 恢复。

Raw argv 位于开发者选项，默认使用“每行一个参数”的编辑方式，内部保持精确 `string[]`；可切换原始 JSON，但不进行 shell 拆词。

### 12.4 即时反馈

创建后立即在列表插入“启动中”会话。失败项保留，可重试或查看详情，不应无反馈消失。

## 13. 恢复会话

- 历史会话直接显示在项目下。
- 点击运行中会话直接切换。
- 点击可恢复会话恢复原生 Session。
- 相同 Session 已打开时切换到现有会话，不重复创建。
- 不存在的会话显示自然语言错误，并允许从历史列表移除。
- 大量历史会话通过恢复窗口搜索，支持标题、Session ID、CLI 和时间筛选；默认当前项目，可切换全部项目。

## 14. 项目管理

项目管理是低频维护页，采用紧凑列表，不使用大型卡片。

列：

- 项目名称
- 路径
- 最近活动
- 活动会话数量
- 菜单

能力：

- 添加项目
- 搜索
- 固定/取消固定
- 修改显示名称
- 隐藏
- 打开文件夹
- 从 CC Desk 列表移除

添加项目后自动协调 Legacy project 和 Native registration。用户只添加一次项目，不看到第二个“注册项目”步骤。

从列表移除只删除 CC Desk 记录，不删除本地项目文件。

## 15. 项目资源抽屉

右侧抽屉默认关闭并自动绑定当前会话的项目、CLI 和启动配置。

分类：

- 项目说明
- 配置
- MCP
- Skills
- Agents
- Plugins

历史会话不在资源抽屉重复展示。

资源必须使用结构化、人类可读组件，禁止主界面直接输出 `JSON.stringify`。资源抽屉保持只读，不成为第二个配置写入器。

刷新时保留旧内容并显示小型 loading。异步结果必须绑定当前会话 ID；切换会话后旧结果不得覆盖新上下文。

敏感信息，包括 API Key、Token、环境值、Headers、Credential 和任意原始配置，不得投影到 UI。

## 16. 设置中心

设置采用左侧分类 + 右侧内容布局，分类为：

1. 通用
2. 外观
3. 终端
4. 启动配置
5. 快捷键
6. 更新
7. 关于

### 16.1 通用

- 语言
- 启动行为
- 默认新建工具
- 关闭窗口行为（只显示实际支持的选项）

### 16.2 外观

- GUI 浅色/深色/跟随系统
- 紧凑/标准密度
- 会话栏宽度

GUI 外观不控制终端主题。

### 16.3 终端

- 主题预览卡
- 字体
- 字号
- 行高
- 光标样式
- 光标闪烁
- 非真实 PTY 的预览区

### 16.4 启动配置

普通用户名称统一为“启动配置”，按 Claude Code 和 Codex CLI 分组。

列表默认显示应用图标、配置名和默认标记。Hover 时最多显示一个高频“编辑”按钮，其他操作进入菜单。

复杂配置显式保存，字段逐级披露：基础、更多选项、开发者选项。敏感值只显示“已设置/未设置/继承系统”。

### 16.5 快捷键

- 搜索操作或快捷键。
- 点击进入按键捕获。
- 冲突必须明确提示，不静默覆盖。
- 支持单项恢复默认和全部恢复默认。

### 16.6 更新

严格区分正式版、候选版和测试版。测试包不得进入普通正式更新提示或 updater channel。

### 16.7 关于

显示应用、版本、构建 commit、项目定位、GitHub、Claude 文档、Codex 文档和许可证。支持复制安全诊断摘要，不包含 secrets、Prompt、回复正文或未经用户允许的隐私路径。

## 17. 公共组件

必须建立并复用：

- `AppButton`
- `IconButton`
- `AppInput`
- `AppSelect`
- `AppMenu`
- `AppDialog`
- `AppDrawer`
- `AppTooltip`
- `AppToast`
- `InlineNotice`
- `EmptyState`
- `LoadingState` / Skeleton
- `ErrorDetails`
- `SessionStatusIcon`
- `CliAppIcon`

页面不得继续各自实现不同按钮、菜单、弹窗和错误卡。

### 17.1 控件尺寸

- 紧凑按钮：28px
- 普通按钮/输入框：32px
- 主要操作：36px
- 项目行：38–40px
- 会话行：36–38px
- 顶部上下文栏：40–44px

### 17.2 操作层级

- 每页最多一个高权重 Primary。
- 常用次要操作使用 Secondary/Ghost。
- 低频操作进入菜单。
- Danger 仅用于明确危险行为，不默认聚焦。

## 18. 菜单与弹窗

### 18.1 菜单

- 一般宽度 180–220px。
- 不适用动作隐藏，不堆积禁用项。
- 危险动作位于底部。
- 支持方向键、Enter、Esc。
- 右键菜单和 `⋯` 菜单内容一致。

### 18.2 确认弹窗

仅用于有副作用或不可逆操作：

- 关闭运行中的会话
- 停止并归档
- 删除启动配置
- 从项目列表移除
- 永久移除归档索引
- 恢复全部快捷键
- 重启并安装更新

普通打开、切换、刷新不弹确认。

## 19. 反馈与错误

### 19.1 Toast

用于已经完成、无需继续处理的短反馈，例如复制、重命名、归档或固定。设置连续调整不重复弹成功 Toast。

### 19.2 内联提示

用于字段错误、配置冲突、资源失败和启动配置不可用。

### 19.3 页面横幅

用于单个 CLI 不可用、局部迁移、更新已准备好等非全局阻断问题。

### 19.4 全页面错误

仅在整个应用无法继续使用时出现。某个 CLI、项目或资源失败不得升级为全应用错误。

### 19.5 错误文案

文案必须回答：发生了什么、影响是什么、下一步怎么做。

内部错误映射示例：

- `REVISION_CONFLICT` → “配置已更新，内容已重新加载，请再试一次。”
- `CLI_NOT_FOUND` → “未找到对应的命令行工具。”
- `AUTH_REQUIRED` → “该工具尚未登录。”
- `LAUNCH_STATE_UNKNOWN` → “正在确认会话状态。”
- `SESSION_NOT_FOUND` → “该会话已无法恢复。”
- `INVALID_RAW_ARGV_JSON` → “自定义参数格式不正确。”
- `RESOURCE_UNAVAILABLE` → “当前资源暂时无法读取。”

技术错误码仅在“查看详情”中出现。

## 20. 加载与空状态

- 项目/会话首次加载使用 Skeleton，避免“暂无数据”闪烁。
- 创建会话立即插入启动中条目。
- 资源刷新保留旧内容。
- 状态确认保留最后终端内容，不清空或自动重放。
- 空状态必须解释原因并提供下一步操作。

典型空状态：无项目、项目无会话、无选择、搜索无结果、无归档、无资源、无更新。

## 21. 文字规范

固定术语：

- 新建会话
- 恢复会话
- 停止会话
- 重启会话
- 关闭会话
- 归档
- 恢复归档
- 永久移除记录
- 启动配置
- 项目资源

同一行为不得在不同页面混用删除、关闭、移除和清除。

## 22. 键盘与可访问性

全局支持：

- `↑ / ↓`：移动列表选择
- `Enter`：打开/确认
- `Esc`：关闭菜单、抽屉、弹窗
- `Ctrl/Cmd + N`：当前项目新建会话
- `Ctrl/Cmd + W`：关闭当前会话
- `Ctrl/Cmd + ,`：设置
- `Ctrl/Cmd + P`：快速切换项目/会话
- `F2`：重命名

要求：

- 清晰焦点轮廓。
- 图标含 Tooltip 与 aria-label。
- 状态不只依赖颜色。
- 菜单、对话框具备合理焦点管理。
- 支持 reduced motion。
- 中文与英文均不破坏布局。

## 23. 响应式与排版

### 23.1 基准

- 导航：44px
- 会话栏：288px，范围 240–360px
- 资源抽屉：344px，范围 300–420px
- 中央终端：自动占满，优先保证可用空间

### 23.2 窄窗口

- 低于约 1180px：资源抽屉覆盖终端，不压缩终端。
- 低于约 900px：会话栏可收起。
- 最小支持窗口：1024×640。
- 禁止全局横向滚动。

### 23.3 必测组合

- 1024×640、1280×720、1366×768、1440×900、1920×1080
- 100%、125%、150% Windows 缩放
- 中文、英文
- 浅色、深色
- 多种终端主题

## 24. 性能

- 全局共享相对时间刷新器。
- 大量会话列表支持虚拟化或等价优化。
- 资源按需加载。
- 隐藏终端不重建，不执行不必要的高频 fit。
- 稳定 `session.id` 作为渲染 key。
- 异步结果绑定 session/request identity。
- 50 项目、单项目 100 会话、30 个运行终端仍应可操作。

## 25. 安全与一致性

- Native 命令继续经过 authenticated document bridge。
- Native terminal 不回退到 Legacy PTY API。
- Raw argv 保持精确参数边界。
- 不自动重放不确定启动或输入。
- 状态不明时禁止直接重复启动。
- 资源投影不暴露 secrets。
- 项目注册、恢复、停止等有副作用操作在 revision conflict 后不自动重放。
- UI 仅展示安全错误映射。

## 26. 迁移与兼容

### 26.1 数据

- 读取旧 Claude 会话和 Native workspace，统一投影到 `UnifiedSession`。
- 不删除旧文件。
- 旧运行中会话不强制重启。
- 新 Claude/Codex 会话走 Native adapter。
- 旧 Native tabs/workspace 可映射到统一会话树。

### 26.2 设置

- 保留 GUI 主题、终端主题、字体、字号和快捷键。
- 已改名主题通过映射迁移。
- 无效值安全回退，提示但不阻断。

### 26.3 归档

归档是 CC Desk 索引状态，不删除 CLI 原生历史。

## 27. 组件迁移

### 保留并改造

- `SessionsPanel.vue`
- `ProjectNode.vue`
- `SessionItem.vue`
- `TerminalView.vue`
- `NativeCliTerminal.vue`
- `nativeTabs.ts`
- `cliWorkspace.ts`
- Settings 各 section

### 新增

- `UnifiedTerminalHost.vue`
- `SessionStatusIcon.vue`
- `CliAppIcon.vue`
- `NewSessionMenu.vue`
- `NewSessionDialog.vue`
- `ProjectResourcesDrawer.vue`
- `SessionOverflowMenu.vue`
- 统一会话 store/adapters
- 公共交互组件

### 退役

统一流程完成并验证后退役：

- `currentView === 'native'`
- 顶层 Native CLI 切换
- `NativeCliWorkbench.vue` 的产品页面职责
- Native 内部第二套 Tab
- 顶部密集启动工具条
- 重复资源入口
- 旧欢迎页多路径分流

底层 Native runtime、transport、input、observer、projection 保留。

## 28. 实施阶段

### U01 公共设计系统

建立公共组件、图标与反馈规范。

### U02 统一会话投影

建立 UnifiedSession、Legacy/Native adapters、去重、状态、时间、归档映射。

### U03 统一应用外壳

建立工作区/项目/设置三个一级入口和上下文抽屉。

### U04 统一会话树

完成混排、状态图标、应用图标、极简时间、单一快捷操作、完整菜单和会话生命周期操作。

### U05 统一终端宿主

接入 Legacy 与 Native terminal，验证切换、后台输出、主题与 resize。

### U06 新建与恢复

完成快捷新建、高级启动、配置记忆、恢复和防重复。

### U07 项目与资源

完成项目管理、自动注册、资源结构化展示和请求隔离。

### U08 设置中心

统一通用、外观、终端、启动配置、快捷键、更新、关于。

### U09 删除重复界面

移除独立 Native 产品页面和重复工具条/资源入口。

### U10 最终审查与测试包

执行自审、对抗、视觉、可访问性、完整 CI 和 Windows 安装验证。

## 29. CI 策略

- 在独立分支积累实现。
- 开发阶段优先本地/定向测试。
- 不为每个样式或小提交开放 PR。
- 完成功能、视觉和文案冻结后再触发普通 CI。
- 最终仅执行必要的普通 CI 与 Windows 测试包工作流。
- 真实失败修复后才重新触发，不用 CI 代替本地分析。

## 30. 测试策略

### 单元

- 统一会话映射、状态优先级、时间格式、归档、去重、路径、错误映射、快捷操作选择和启动配置优先级。

### 组件

- 状态只有图标。
- 应用图标正确。
- 尾部时间/快捷操作不引发布局跳动。
- 同时最多一个高频操作。
- 长标题省略。
- 菜单与弹窗键盘可用。
- 空状态和 loading 正确。

### 集成

- 新建/恢复 Claude 和 Codex。
- Legacy/Native 混排。
- 切换不重启。
- 重命名、停止、重启、关闭、归档、恢复归档。
- Session 防重复。
- Revision conflict。
- 项目只注册一次。
- 旧异步结果不覆盖当前上下文。

### 终端

- GUI × terminal theme 组合。
- 主题实时切换和持久化。
- 字体/字号/行高 fit。
- 会话栏和资源栏开合。
- 后台输出切换。
- WebGL fallback。
- 中文、Emoji、宽字符。

### 视觉回归

覆盖空状态、混排状态、Hover 单按钮、菜单、新建、高级启动、归档、资源、设置、更新和确认弹窗，在既定尺寸、缩放、语言和主题组合下验证。

### 压力

- 50 项目。
- 单项目 100 会话。
- 30 运行终端。
- 200 字符会话标题。
- 超长项目名和 Windows 路径。
- 状态快速变化与大量输出。

### 迁移

- 旧 Claude-only 数据。
- 旧 Native workspace。
- 两者并存。
- 旧终端主题和快捷键。
- 多实例并发与局部配置损坏。

## 31. 完成标准

以下全部满足后，才可声明交互重构完成：

1. 全应用使用统一外壳。
2. 仅保留工作区、项目、设置三个核心入口。
3. Claude/Codex 会话直接混排在项目下。
4. 左侧会话树是唯一 Tab 系统。
5. 独立 Native CLI 产品页面和密集工具条消失。
6. 会话行使用状态图标、应用图标、标题、极简时间和按需操作。
7. 状态不显示文字且无需颜色仍可分辨。
8. 同时最多显示一个高频快捷操作。
9. 重命名、停止、重启、关闭、归档、恢复归档完整可用。
10. 菜单和右键菜单一致。
11. 普通新建最多两步。
12. 切换会话不重启、不串线、不丢输出。
13. 终端主题完整保留且独立于 GUI。
14. 旧数据、旧终端主题和快捷键继续可用。
15. 项目只需添加一次。
16. 资源抽屉结构化展示，不显示裸 JSON 或 secrets。
17. 普通界面不显示裸错误码和技术字段。
18. 设置中心分类与控件统一。
19. 1024×640 和 125%/150% 缩放无重叠。
20. 中英文、浅暗主题及终端主题组合均通过视觉验证。
21. 自审、对抗、可访问性、完整 CI 和测试安装包全部通过。

## 32. 决策记录

以下决策已由用户确认并冻结：

- 采用项目下直接混排 Claude/Codex 会话的 A 方案。
- 界面尽可能简单易懂，符合自然阅读顺序。
- 状态不显示文字，仅使用高辨识度图标。
- Claude/Codex 在会话列表使用应用图标，不使用常驻文字缩写。
- 时间采用极简格式。
- 快捷操作按需显示，一般同时只显示一个高频操作。
- 重命名、关闭、归档、重启等能力必须保留。
- 全部交互页面统一优化。
- 保留“工匠终端”视觉方向。
- 终端主题完整保留并独立于 GUI。
