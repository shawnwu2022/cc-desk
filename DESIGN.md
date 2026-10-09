---
name: CC Desk
description: 面向 Claude Code 与 Codex CLI 重度用户的桌面多会话工作台 — 工匠终端视觉系统
colors:
  paper-warm: "#faf9f6"
  sand-soft: "#f5f3ee"
  sand-card: "#ebe8e0"
  ink-charcoal: "#1a1816"
  gray-mist: "#5a5550"
  gray-faint: "#69645e"
  border-refined: "#d9d5cc"
  border-deep: "#b5b0a8"
  ink-blue: "#1e3a5f"
  ink-blue-soft: "#2a5082"
  amber-gold: "#d4a574"
  amber-light: "#e8c9a8"
  amber-deep: "#b8956a"
  amber-ink: "#7a5c3a"
  status-green: "#3d8c6e"
  status-amber: "#c4964a"
  status-red: "#c45c4a"
  status-blue: "#2a5082"
  tag-mcp-bg: "#e3f2fd"
  tag-mcp-text: "#1565c0"
  tag-skill-bg: "#fff3e0"
  tag-skill-text: "#bf360c"
  tag-agent-bg: "#f3e5f5"
  tag-agent-text: "#7b1fa2"
  white: "#ffffff"
  charcoal-warm: "#1c1a17"
  ink-blue-night: "#4a7aad"
  amber-glow: "#f0d4a8"
  text-warm-white: "#f8f6f3"
typography:
  headline:
    fontFamily: "'SF Pro Text', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', sans-serif"
    fontSize: "16px"
    fontWeight: 600
    lineHeight: 1.5
  title-lg:
    fontFamily: "'SF Pro Text', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', sans-serif"
    fontSize: "15px"
    fontWeight: 500
    lineHeight: 1.5
  title:
    fontFamily: "'SF Pro Text', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', sans-serif"
    fontSize: "14px"
    fontWeight: 500
    lineHeight: 1.5
  title-sm:
    fontFamily: "'SF Pro Text', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', sans-serif"
    fontSize: "13px"
    fontWeight: 500
    lineHeight: 1.5
  body:
    fontFamily: "'SF Pro Text', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "'SF Pro Text', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', sans-serif"
    fontSize: "12px"
    fontWeight: 500
  label-sm:
    fontFamily: "'SF Pro Text', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', sans-serif"
    fontSize: "11px"
    fontWeight: 400
  micro:
    fontFamily: "'SF Pro Text', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', sans-serif"
    fontSize: "10px"
    fontWeight: 400
  mono:
    fontFamily: "'Cascadia Code', 'Fira Code', 'JetBrains Mono', Consolas, 'Microsoft YaHei', 'PingFang SC', 'Noto Sans CJK SC', monospace"
    fontSize: "14px"
    fontWeight: 400
rounded:
  indicator: "2px"
  badge: "3px"
  sm: "4px"
  md: "6px"
  lg: "8px"
  card: "10px"
  xl: "12px"
  dot: "50%"
spacing:
  xs: "4px"
  sm: "6px"
  md: "8px"
  lg: "12px"
  xl: "16px"
  2xl: "24px"
components:
  button-primary:
    backgroundColor: "{colors.ink-blue}"
    textColor: "#ffffff"
    rounded: "{rounded.md}"
    padding: "8px 16px"
  button-secondary:
    backgroundColor: "transparent"
    textColor: "{colors.gray-mist}"
    rounded: "{rounded.sm}"
    padding: "4px 12px"
  button-danger:
    backgroundColor: "transparent"
    textColor: "{colors.status-red}"
    rounded: "{rounded.sm}"
    padding: "4px 12px"
  icon-button:
    backgroundColor: "transparent"
    textColor: "{colors.gray-mist}"
    rounded: "{rounded.md}"
    size: "32px"
  icon-button-active:
    backgroundColor: "rgba(212, 165, 116, 0.15)"
    textColor: "{colors.amber-gold}"
    rounded: "{rounded.md}"
    size: "32px"
  tag-type:
    backgroundColor: "#e3f2fd"
    textColor: "#1565c0"
    rounded: "{rounded.sm}"
    padding: "2px 6px"
    typography: "{typography.micro}"
  input-default:
    backgroundColor: "{colors.paper-warm}"
    textColor: "{colors.ink-charcoal}"
    rounded: "{rounded.md}"
---

# Design System: CC Desk

## Overview

当前实现对应[冻结规格](docs/superpowers/specs/2026-09-28-unified-workspace-ux-design.md)。代码与组件契约已接入统一外壳；真实像素、Windows 缩放和安装验收尚未完成，状态见 [U01–U10 证据](docs/superpowers/execution/U01-U10.md)。

**Creative North Star: "工匠终端 (Artisan Terminal)"**

CC Desk 的界面是一件放在木工坊里的精密仪器：温暖米灰的纸面质感铺底，深邃墨蓝负责所有"可操作"的承诺，琥珀金像一枚黄铜镶件，只镶嵌在"当前激活"的位置上。GUI 是安静的工装，终端永远 是主角——Chrome 的视觉音量被刻意压到内容之下。

这个系统的性格是"克制而精准"（用户确认）：描边优先于填充，hover 才浮现次要操作，圆角克制在 3–12px 之间，动效统一 0.15s。信息密度偏高（侧边栏 11–13px 文字阶梯），因为用户是熟练的多会话重度用户，密度即效率。双主题（浅色「温暖米灰」/ 暗色「温暖深炭」）是同一套语义的两套值：墨蓝与琥珀在暗色下整体提亮为"温暖墨蓝 + 璀璨琥珀"，暖棕倾向贯穿两套基底，避免纯中性灰的冷感。

**Key Characteristics:**
- 墨蓝 = 可交互（按钮、链接、焦点），琥珀金 = GUI 激活态（选中、徽标），两者职责绝不互换
- 温暖中性色基底：米灰/深炭都带暖棕倾向，不用纯灰
- 平铺为主 + 极轻阴影，深度靠背景三级分层表达
- 高密度排版：10–16px 字号阶梯，14px 为全局基线
- 终端层拥有独立主题，不随 GUI 主题混合
- 微交互统一 0.15s ease、具名过渡属性（不用 `transition: all`）

## Colors

色板是"墨水与黄铜"的组合：低饱和暖中性铺底，一支深墨蓝、一支琥珀金，辅以四支压暗的状态色。所有颜色在 `src/styles/global.css` 以 CSS 自定义属性定义，浅色主题为规范基准，`[data-theme="dark"]` 提供整套对应值。

### Primary
- **Ink Blue / 墨蓝** (#1e3a5f；暗色 Ink Blue Night #4a7aad): 主强调色。主按钮、链接、输入框聚焦边框、focus ring、info 语义。它是"可点击/可操作"的统一信号。次级墨蓝 Ink Blue Soft (#2a5082；暗色 #6a9acd) 用于 hover 递进与 info 状态。
- **Amber Gold / 琥珀金** (#d4a574；暗色 Amber Glow #f0d4a8，深态 #b8956a): 特质色与激活色。GUI 选中态背景与边框、图标激活指示条、当前启动配置徽标。终端光标由所选终端主题决定。浅态 #e8c9a8 用于暗色下的奶色提亮。**作文字使用时**必须用 Amber Ink(#7a5c3a，暗色即 #f0d4a8)——琥珀金本身在浅色米灰底上仅 ~2:1，只作装饰与填充，不作文字。

### Secondary
- **Status Green / 墨绿** (#3d8c6e；暗色 #5dad8e): 成功、运行中状态。
- **Status Amber / 琥珀警示** (#c4964a；暗色 #f0b460): 警告、pending 状态。与品牌琥珀同族但更饱和。
- **Status Red / 赭红** (#c45c4a；暗色 #e8705a): 错误、危险操作、失败状态。
- **Status Blue / 墨蓝** (#2a5082；暗色 #6a9acd): 信息类语义。

### Neutral
- **Paper Warm / 温暖米灰** (#faf9f6；暗色 Charcoal Warm #1c1a17): 主背景，GUI 最底层。
- **Sand Soft / 柔和沙灰** (#f5f3ee；暗色 #252220): 次级背景（图标栏、面板底）。
- **Sand Card / 沙灰卡片** (#ebe8e0；暗色 #302d2a): 卡片、悬浮层第三层背景。
- **Ink Charcoal / 深炭** (#1a1816；暗色 Text Warm White #f8f6f3): 主文字与终端浅色主题前景。
- **Gray Mist / 中灰** (#5a5550；暗色 #c4c0b8): 次级文字。
- **Gray Faint / 暖深灰** (#69645e；暗色 #9c9788): 辅助文字、占位符。2026-08 调深以满足 WCAG AA（原 #8a8680/#7a7568 仅 ~3.5:1）；对三档背景均 ≥4.5:1，由 `tests/designTokens.test.ts` 回归锁定。
- **Border Refined / 精致灰** (#d9d5cc；暗色 #3a3734): 默认 1px 边框。
- **Border Deep / 深灰** (#b5b0a8；暗色 #4a4640): 强边框、滚动条。

### Named Rules
**The Quiet Chrome Rule.** GUI 是工装，终端是主角。强调色（墨蓝+琥珀合计）在任一屏占比 ≤10%；界面文字层级默认压到 secondary/tertiary，hover 才升到 primary。
**The Amber Activation Rule.** 琥珀金只表示"当前激活/选中/品牌温度"（选中项、光标、激活徽标），绝不用于普通可点击元素——那是墨蓝的职责。两者不可互换。
**The Warm Neutral Rule.** 中性色必须带暖棕倾向（米灰 #faf9f6、暖炭 #1c1a17），禁止引入纯中性灰或冷蓝灰做背景。

## Typography

**Body Font:** SF Pro Text 系统栈（`'SF Pro Text', -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', sans-serif`）
**Label/Mono Font:** Cascadia Code 栈（`'Cascadia Code', 'Fira Code', 'JetBrains Mono', Consolas` + 中文回退「Microsoft YaHei / PingFang SC / Noto Sans CJK SC」）

**Character:** 纯系统栈，零外部字体加载——桌面工具的性能纪律。无衬线 UI 与等宽终端形成"操作面板 vs 机器输出"的材质对比；中英文混排通过中文回退字体保持等宽节奏。

### Hierarchy
- **Headline** (600, 16px, 1.5): 空状态/设置区标题、弹窗标题。整个系统最大的字号。
- **Title LG** (500, 15px, 1.5): 项目管理页的项目行主名。
- **Title** (500, 14px, 1.5): 设置区列表条目主名称（启动配置名、卡片名称）。
- **Title SM** (500, 13px, 1.5): 侧边栏条目主名（会话、Skill/Agent/MCP/Plugin 名）；主按钮文字也是 13px。
- **Body** (400, 14px, 1.5): 正文、描述文字。全局 body 基线。
- **Label** (500, 12px): 分组头、次级信息、表单标签。
- **Label SM** (400, 11px): 侧边栏副文字（版本号、完整名、时间戳、路径）。
- **Micro** (400, 10px): 类型标签 chip、徽标内文字。当前会话日期尾槽为 9px 的固定宽度兼容例外，必须在最终真实缩放验收中检查可读性。
- **Mono** (400, 14px): 终端内容、代码片段、路径。

### Named Rules
**The 14px Baseline Rule.** 全局基线 14px / 1.5 行高；侧边栏高密度区用 11–13px 阶梯；10px 只给 tag。需要"更大"时直接跳 16px，不设中间档。

## Layout

唯一 `AppShell` 使用四列空间：**主导航 44px → 项目/会话栏 288px → 弹性主内容 → 可选资源栏 344px**。一级入口只有工作区、项目、设置；内容视图不复制全局导航。自定义标题栏保留平台窗口控件。

- **密度**：标准会话行 38px，紧凑行 34px；项目行 40px。状态、CLI 图标、标题、时间/快捷操作、菜单的五列固定保留。
- **间距节奏**：4px 基数（4/6/8/12/16/24），组件内 gap 常用 4–6px，区块间 12–16px。
- **分栏边界**：1px `--border-color`，不使用常驻阴影。会话栏可设为 240–360px；资源栏约束为 300–420px。
- **终端区**：使用剩余空间，隐藏时仍保留终端实例与后台输出；导航或抽屉开合不重建终端。
- **响应式**：按逻辑 CSS 宽度判断。低于 1180px，资源使用共享模态抽屉覆盖；低于 900px，会话栏可折叠且不覆盖桌面宽度偏好。最小支持窗口为 1024×640；不再次按 DPR 乘宽度，不允许全局横向滚动。
- **缩放证据**：5 种尺寸、DPR 1/1.25/1.5、中英文、浅暗主题矩阵已编写；jsdom 契约不能代替 Windows 100%/125%/150% 的真实渲染验收。

## Elevation & Depth

平铺为主 + 轻阴影（用户确认）。静态界面完全靠**背景三级分层**（bg-primary → bg-secondary → bg-tertiary）表达深度，层级之间以 1px 边框勾勒。阴影词汇存在四级但透明度极低（浅色 0.04–0.12），只用于真正浮起的临时层：弹窗、下拉菜单、悬浮卡片。暗色主题下阴影透明度整体加重（0.25–0.55）以补偿暗底对比。

### Shadow Vocabulary
- **shadow-sm** (`0 1px 2px rgba(26,24,22,0.04)`；暗色 `rgba(0,0,0,0.25)`): 微提示，极慎用。
- **shadow-md** (`0 2px 8px rgba(26,24,22,0.06)`；暗色 0.35): 下拉菜单、小型 popover。
- **shadow-lg** (`0 4px 16px rgba(26,24,22,0.08)`；暗色 0.45): 侧边栏浮层、较大菜单。
- **shadow-xl** (`0 8px 32px rgba(26,24,22,0.12)`；暗色 0.55): 共享模态弹窗（AppDialog/AppDrawer）。

### Named Rules
**The Flat-First Rule.** 静态表面永远无阴影。阴影只作为对"临时浮起"（hover、弹出、模态）的响应出现，不作为卡片/面板的常驻装饰。

## Shapes

小而克制的圆角语言：控件 6px（radius-md）、卡片/容器 8px（radius-lg）、侧边栏分组容器 10px（card）、弹窗 12px（radius-xl）、内部小元素与 tag 4px、状态徽标点 3px 或 50% 圆形、指示条端角 2px。整体没有直角也没有胶囊形——不设 >12px 的圆角。边框统一 1px 实线，三级深浅（border-light / border-color / border-dark）。图标为 20px 线性 PNG/SVG，激活态以 3px 琥珀侧条（`border-radius: 0 2px 2px 0`）标记。滚动条 6px 细条、3px 圆角、透明轨道。

## Components

### Buttons
- **Shape:** 小圆角 6px，主按钮 padding 8px 16–20px，字号 13px。
- **Primary:** 墨蓝实心（`--accent-primary`）白字，无描边；hover `opacity: 0.9`，disabled `opacity: 0.5`。
- **Secondary / Ghost:** 透明底 + 1px `--border-color` 描边 + `--text-secondary` 文字；hover 换 `--hover-bg`（墨蓝 6–12% 透明度）底。描边变体 `.primary`：墨蓝字+墨蓝描边，hover 实心反转。
- **Danger:** 透明底赭红字（`--status-error`）；hover 赭红 8% 透明底。
- **Focus:** 全局 `outline: 2px solid var(--focus-ring); outline-offset: 2px`。
- **Transition:** 统一 0.15s ease、具名属性列表（`background-color, color, border-color, opacity, transform, box-shadow`），不用 `transition: all`。

### Primary navigation
44px 导航列内使用 32×32px 按钮，只包含工作区、项目、设置。静默态为 `--text-secondary`，hover 使用 `--hover-bg`，选中态为琥珀背景/图标与左侧 3px 指示条。旧 IconBar 和 Native 顶层入口已退役。

### Cards / List Items
项目和启动配置优先使用紧凑列表。会话行按状态图标、CLI 应用图标、标题、极简时间、尾部菜单排序；只有标题省略。时间使用 `刚刚`/`now`、`Nm`、`Nh`、`Nd`、`M/D`、`YY/M/D`，完整时间通过 Tooltip 提供。

每行最多一个状态相关快捷动作，hover 或键盘 focus 后覆盖固定时间槽；其余动作在同一个共享菜单中，右键与 overflow 内容一致。状态使用六种自有轮廓图标及可访问名称，不增加状态文字，不仅靠颜色。CLI 标记为自有中性图形；图片失败时才使用 CC/CX 回退。

### Chips / Tags
类型标签：10px 字号、2px 6px padding、4px 圆角、类型色淡底+同系深字（MCP 蓝 #e3f2fd/#1565c0、Skills 琥珀、Agents 紫；暗色换半透明底+亮字）。仅用于元数据分类，不做可交互筛选。

### Inputs / Fields
`--bg-primary` 底、1px `--border-color`、6px 圆角，继承 14px 字号。聚焦：无 outline，`border-color → var(--focus-ring)` 墨蓝，0.15s 过渡。禁用 opacity 0.5。多行编辑用 CodeMirror（One Dark 仅限 JSON 编辑器）。

### Navigation
项目下直接混排 Claude Code 与 Codex CLI。项目/会话树是唯一会话 Tab 系统，没有第二个 Native Tab 栏。切换只改变显示/选择；归档是 Desk 索引状态，重命名是 Desk 显示元数据，都不改写 CLI 历史或向终端注入命令。

### Shared interactions
公共控件使用 `src/components/ui/`。compact/normal/primary 高度至少为 28/32/36px；会话尾部固定 20×28px。菜单支持方向键、Home/End、Enter/Escape；模态对话框陷阱焦点，危险操作不自动聚焦。长标签可以换行，不能压缩点击高度。Tooltip 的变换/裁切祖先真实像素检查仍待执行。

### Terminal（独立主题层）
GUI 和终端主题独立，四种浅暗组合均受支持。`app.terminalPreferences` 同时供 Legacy Claude、Native Claude、Native Codex 使用；16 个原有终端主题保留自己的前景、背景、ANSI、光标和选区颜色，不把 GUI 琥珀强加到所有主题。

纯颜色/光标修改原地更新 xterm options，不 fit、重启、清空缓冲、改变选择或重放输入。字体/字号/行高只对当前可见终端合并一次 fit，后台延迟至显示。renderer 选择只影响新建终端；WebGL 失败保留原终端及同一配色。设置预览是静态非 PTY 内容。旧主题 ID、fontSize、webglRenderer 继续兼容，详见[终端偏好](docs/terminal-preferences.md)。

## Do's and Don'ts

### Do:
- **Do** 一律引用 CSS 自定义属性（`var(--accent-primary)`），新代码不得裸写色值；浅色/暗色两套值必须成对修改。
- **Do** 用背景三级分层（bg-primary → secondary → tertiary）表达静态层级。
- **Do** 次要操作在 hover/focus 时浮现（opacity 0→1, 0.15s），保持界面安静。
- **Do** 所有过渡统一 0.15s ease，且具名属性列表（`background-color, color, border-color, opacity, transform, box-shadow`），禁用 `transition: all`（防 layout 属性泄漏）；徽标脉冲用 `pulse 2s ease-in-out infinite`。
- **Do** 可交互元素用墨蓝、激活态用琥珀，焦点环保持 2px outline + 2px offset。

### Don't:
- **Don't** 引入墨蓝/琥珀/类型标签色之外的新色相（Agents 紫是既有例外，不扩散）。
- **Don't** 使用 >12px 圆角、胶囊按钮或渐变。
- **Don't** 给静态卡片/面板加常驻阴影——阴影只给弹层与 hover 响应。
- **Don't** 在 GUI 层模仿终端配色；终端主题独立（`--terminal-*`），两层不混用。
- **Don't** 让琥珀金出现在非激活的可点击元素上。
- **Don't** 扩散小于 10px 的字号；当前会话日期尾槽的 9px 例外需实际像素验收。
