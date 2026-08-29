---
name: CC Desk
description: 面向 Claude Code 重度用户的桌面多会话工作台 — 工匠终端视觉系统
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
    size: "40px"
  icon-button-active:
    backgroundColor: "rgba(212, 165, 116, 0.15)"
    textColor: "{colors.amber-gold}"
    rounded: "{rounded.md}"
    size: "40px"
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

**Creative North Star: "工匠终端 (Artisan Terminal)"**

CC Desk 的界面是一件放在木工坊里的精密仪器：温暖米灰的纸面质感铺底，深邃墨蓝负责所有"可操作"的承诺，琥珀金像一枚黄铜镶件，只镶嵌在"当前激活"的位置上。GUI 是安静的工装，终端永远 是主角——Chrome 的视觉音量被刻意压到内容之下。

这个系统的性格是"克制而精准"（用户确认）：描边优先于填充，hover 才浮现次要操作，圆角克制在 3–12px 之间，动效统一 0.15s。信息密度偏高（侧边栏 11–13px 文字阶梯），因为用户是熟练的多会话重度用户，密度即效率。双主题（浅色「温暖米灰」/ 暗色「温暖深炭」）是同一套语义的两套值：墨蓝与琥珀在暗色下整体提亮为"温暖墨蓝 + 璀璨琥珀"，暖棕倾向贯穿两套基底，避免纯中性灰的冷感。

**Key Characteristics:**
- 墨蓝 = 可交互（按钮、链接、焦点），琥珀金 = 激活态（选中、光标、徽标），两者职责绝不互换
- 温暖中性色基底：米灰/深炭都带暖棕倾向，不用纯灰
- 平铺为主 + 极轻阴影，深度靠背景三级分层表达
- 高密度排版：10–16px 字号阶梯，14px 为全局基线
- 终端层拥有独立主题，不随 GUI 主题混合
- 微交互统一 0.15s ease、具名过渡属性（不用 `transition: all`）

## Colors

色板是"墨水与黄铜"的组合：低饱和暖中性铺底，一支深墨蓝、一支琥珀金，辅以四支压暗的状态色。所有颜色在 `src/styles/global.css` 以 CSS 自定义属性定义，浅色主题为规范基准，`[data-theme="dark"]` 提供整套对应值。

### Primary
- **Ink Blue / 墨蓝** (#1e3a5f；暗色 Ink Blue Night #4a7aad): 主强调色。主按钮、链接、输入框聚焦边框、focus ring、info 语义。它是"可点击/可操作"的统一信号。次级墨蓝 Ink Blue Soft (#2a5082；暗色 #6a9acd) 用于 hover 递进与 info 状态。
- **Amber Gold / 琥珀金** (#d4a574；暗色 Amber Glow #f0d4a8，深态 #b8956a): 特质色与激活色。终端光标、选中态背景与边框、图标激活指示条、"已激活 Provider"徽标。浅态 #e8c9a8 用于暗色下的奶色提亮。**作文字使用时**必须用 Amber Ink(#7a5c3a，暗色即 #f0d4a8)——琥珀金本身在浅色米灰底上仅 ~2:1，只作装饰与填充，不作文字。

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
- **Headline** (600, 16px, 1.5): 欢迎页/设置区标题、弹窗标题。整个系统最大的字号。
- **Title LG** (500, 15px, 1.5): 项目选择页的项目行主名。
- **Title** (500, 14px, 1.5): 设置区列表条目主名称（Provider 名、卡片名称）。
- **Title SM** (500, 13px, 1.5): 侧边栏条目主名（会话、Skill/Agent/MCP/Plugin 名）；主按钮文字也是 13px。
- **Body** (400, 14px, 1.5): 正文、描述文字。全局 body 基线。
- **Label** (500, 12px): 分组头、次级信息、表单标签。
- **Label SM** (400, 11px): 侧边栏副文字（版本号、完整名、时间戳、路径）。
- **Micro** (400, 10px): 类型标签 chip、徽标内文字。下限，不再更小。
- **Mono** (400, 14px): 终端内容、代码片段、路径。

### Named Rules
**The 14px Baseline Rule.** 全局基线 14px / 1.5 行高；侧边栏高密度区用 11–13px 阶梯；10px 只给 tag。需要"更大"时直接跳 16px，不设中间档。

## Layout

三栏空间模型：**IconBar (48px 固定) → 侧边栏面板 (可折叠) → 终端/内容区 (弹性)**。窗口本身使用自定义标题栏（Windows），无系统边框。

- **密度**：高密度工具布局。列表条目紧凑（上下 padding 4–6px），分组间 8–12px。
- **间距节奏**：4px 基数（4/6/8/12/16/24），组件内 gap 常用 4–6px，区块间 12–16px。
- **分栏边界**：1px `--border-color` 分隔，不用阴影分栏。
- **侧边栏**：无遮罩、不抢焦点（与 GUI 主区并存），面板宽约 280–320px。
- **终端区**：占据剩余全部空间，`--radius-lg` 圆角容器，与 GUI 层之间由背景色差分层。
- **响应式**：桌面固定窗口场景，无断点系统；窗口尺寸约束由 Tauri 配置管理。

## Elevation & Depth

平铺为主 + 轻阴影（用户确认）。静态界面完全靠**背景三级分层**（bg-primary → bg-secondary → bg-tertiary）表达深度，层级之间以 1px 边框勾勒。阴影词汇存在四级但透明度极低（浅色 0.04–0.12），只用于真正浮起的临时层：弹窗、下拉菜单、悬浮卡片。暗色主题下阴影透明度整体加重（0.25–0.55）以补偿暗底对比。

### Shadow Vocabulary
- **shadow-sm** (`0 1px 2px rgba(26,24,22,0.04)`；暗色 `rgba(0,0,0,0.25)`): 微提示，极慎用。
- **shadow-md** (`0 2px 8px rgba(26,24,22,0.06)`；暗色 0.35): 下拉菜单、小型 popover。
- **shadow-lg** (`0 4px 16px rgba(26,24,22,0.08)`；暗色 0.45): 侧边栏浮层、较大菜单。
- **shadow-xl** (`0 8px 32px rgba(26,24,22,0.12)`；暗色 0.55): 模态弹窗（Settings Overlay 等）。

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

### Icon Bar（签名组件）
左侧 48px 窄条导航，`--bg-secondary` 底 + 1px 右边框。图标按钮 40×40px、6px 圆角、透明底。状态机：静默（`--text-secondary`）→ hover（`--hover-bg` + 主文字）→ **active（琥珀选中 `--selected-bg` + 琥珀金图标 + 左缘 3px 琥珀指示条）**。角标为 8px 圆点（2px 底色描边），`pulse 2s` 呼吸动画，红=错误/更新、琥珀=权限提醒。

### Cards / List Items
条目式卡片（Provider 卡、会话条目）：`--bg-tertiary` 或透明底、6–8px 圆角、1px 边框或无边框。名称 14px/500 主文字 + 12px tertiary 副文字的单行结构。**hover 才浮现操作区**（`opacity: 0 → 1`, 0.15s），激活条目带琥珀 `active-badge`（12px/600 琥珀深色文字）。

### Chips / Tags
类型标签：10px 字号、2px 6px padding、4px 圆角、类型色淡底+同系深字（MCP 蓝 #e3f2fd/#1565c0、Skills 琥珀、Agents 紫；暗色换半透明底+亮字）。仅用于元数据分类，不做可交互筛选。

### Inputs / Fields
`--bg-primary` 底、1px `--border-color`、6px 圆角，继承 14px 字号。聚焦：无 outline，`border-color → var(--focus-ring)` 墨蓝，0.15s 过渡。禁用 opacity 0.5。多行编辑用 CodeMirror（One Dark 仅限 JSON 编辑器）。

### Navigation
项目树/会话列表：分组头 11–12px、条目 12–13px；选中态与 IconBar 同语言（`--selected-bg` 琥珀底或 `--hover-bg` 墨蓝底，视层级而定）。Tab 栏为终端切换器，激活 Tab 带状态色指示（运行=绿、pending=琥珀）。

### Status Indicators
8px 圆点徽标（50% 圆角 + 2px 底色描边），语义色填充，重要事件 `pulse` 呼吸。会话状态：运行/思考/等待以圆点+颜色区分，绝不加文字噪音。

### Terminal（独立主题层）
终端拥有独立于 GUI 的主题变量（`--terminal-*`）：浅色 GUI 下终端仍是 VS Code 风格浅灰（#f8f9fa/#1e1e1e 系），暗色 GUI 下为 #1e1e1e 底。唯一跨越两层的是琥珀金——终端光标恒为 `--accent-gold`。GUI 主题切换不重映射终端 ANSI 色。

## Do's and Don'ts

### Do:
- **Do** 一律引用 CSS 自定义属性（`var(--accent-primary)`），新代码不得裸写色值；浅色/暗色两套值必须成对修改。
- **Do** 用背景三级分层（bg-primary → secondary → tertiary）表达静态层级。
- **Do** 次要操作藏进 hover 浮现（opacity 0→1, 0.15s），保持界面安静。
- **Do** 所有过渡统一 0.15s ease，且具名属性列表（`background-color, color, border-color, opacity, transform, box-shadow`），禁用 `transition: all`（防 layout 属性泄漏）；徽标脉冲用 `pulse 2s ease-in-out infinite`。
- **Do** 可交互元素用墨蓝、激活态用琥珀，焦点环保持 2px outline + 2px offset。

### Don't:
- **Don't** 引入墨蓝/琥珀/类型标签色之外的新色相（Agents 紫是既有例外，不扩散）。
- **Don't** 使用 >12px 圆角、胶囊按钮或渐变。
- **Don't** 给静态卡片/面板加常驻阴影——阴影只给弹层与 hover 响应。
- **Don't** 在 GUI 层模仿终端配色；终端主题独立（`--terminal-*`），两层不混用。
- **Don't** 让琥珀金出现在非激活的可点击元素上。
- **Don't** 使用小于 10px 的字号。
