# 组件结构

## 组件树

```
App.vue
├── WelcomeView.vue          # 欢迎引导页（无收藏项目时）
├── ProjectSelectView.vue    # 项目选择页（有收藏项目时）
└── TerminalView.vue         # 终端主视图（进入项目后）
    ├── TerminalHeader.vue   # 标题栏（返回按钮、项目名）
    ├── XTermTerminal.vue    # xterm.js 终端封装
    ├── InfoBar.vue          # 信息栏（菜单按钮、状态）
    └── SidebarDrawer.vue    # 左侧抽屉（项目列表、设置）
```

共 **7 个组件**，保持精简。

## 组件详情

### App.vue — 视图切换

```vue
<template>
  <Transition name="fade" mode="out-in">
    <WelcomeView v-if="currentView === 'welcome'" />
    <ProjectSelectView v-else-if="currentView === 'projects'" />
    <TerminalView v-else />
  </Transition>
</template>

<script setup>
// 视图状态：'welcome' | 'projects' | 'terminal'
const currentView = ref('welcome')

// 启动时检查收藏项目
onMounted(async () => {
  const favorites = await window.api.getFavorites()
  currentView.value = favorites.length > 0 ? 'projects' : 'welcome'
})
</script>
```

### XTermTerminal.vue — 终端核心

职责：
- 创建和管理 xterm.js Terminal 实例
- 加载 FitAddon、SearchAddon、WebLinksAddon、SerializeAddon
- 双向数据绑定：onData → ptyInput，onPtyOutput → term.write
- resize 同步

Props：
- `cwd: string` — 工作目录

Events：
- `sessionEnd` — PTY 进程退出时触发

### TerminalView.vue — 终端容器

布局结构：
```
┌──────────────────────────────────────┐
│ TerminalHeader (38px)                │
├──────────────────────────────────────┤
│                                      │
│ XTermTerminal (flex: 1)              │
│                                      │
├──────────────────────────────────────┤
│ InfoBar (36px)                       │
└──────────────────────────────────────┤
```

职责：
- 组合 Header、Terminal、InfoBar、Sidebar
- 管理 sidebarVisible 状态
- 处理项目切换

### TerminalHeader.vue — 标题栏

元素：
- 左侧：返回按钮 `←`
- 中间：项目名（从 appStore.currentProject）
- 右侧：设置按钮（触发 toggleSidebar）

Events：
- `back` — 返回项目列表
- `settings` — 打开侧边栏

### InfoBar.vue — 信息栏

元素：
- 左侧：菜单按钮 `☰`（触发 toggleSidebar）
- 状态文本：`Ready` 或 `Terminal running`

Events：
- `menu` — 打开侧边栏

### SidebarDrawer.vue — 左侧抽屉

布局：
```
┌─────────────────┐
│ Claude Code  ✕ │  ← Header (关闭按钮)
├─────────────────┤
│ ── Projects ── │
│ ○ project-a    │
│ ○ project-b    │
│ + Add project  │
│                 │
│ ── Settings ── │
│ ○ Open Settings │
├─────────────────┤
│ v0.1.0         │  ← Footer
└─────────────────┘
```

特性：
- 从左侧滑入，宽度 260px
- 点击外部或 Escape 关闭
- Teleport to body

Events：
- `close` — 关闭抽屉
- `selectProject` — 选择项目

### WelcomeView.vue — 欢迎引导

布局：
- 全屏居中
- Logo + 标题 + "Select Project Directory" 按钮

Events：
- `selectProject` — 打开目录选择器

### ProjectSelectView.vue — 项目选择

布局：
- 项目卡片列表（显示名称、路径、最近使用时间）
- "Add new project" 按钮

Events：
- `selectProject` — 打开目录选择器
- `openProject` — 进入指定项目

## Store 结构

### terminal.ts — 终端实例管理

```typescript
interface TerminalInstance {
  id: string
  cwd: string
  createdAt: number
  serialized?: string  // SerializeAddon 数据
}

const terminals = ref<Map<string, TerminalInstance>>(new Map())
const activeId = ref<string>('')

// 方法
createTerminal(cwd): string
removeTerminal(id)
setActive(id)
saveSerialized(id, data)
```

### app.ts — 应用状态

```typescript
interface Favorite {
  path: string
  name: string
  lastOpened?: number
}

const cwd = ref<string>('')
const favorites = ref<Favorite[]>([])
const sidebarOpen = ref(false)
const theme = ref<string>('light')
const fontSize = ref<number>(14)

// 计算属性
currentProject // 从 cwd 提取项目名

// 方法
setCwd(path)
setFavorites(favs)
addFavorite(fav)
removeFavorite(path)
updateLastOpened(path)
toggleSidebar()
setTheme(theme)
setFontSize(size)
```

## 样式体系

### global.css — CSS 变量

```css
:root {
  /* 背景层级 */
  --bg-primary: #ffffff;
  --bg-secondary: #f7f8fa;
  --bg-tertiary: #eef0f4;

  /* 终端（浅色） */
  --terminal-bg: #f8f9fa;
  --terminal-fg: #1a1a2e;

  /* 文字 */
  --text-primary: #1a1a2e;
  --text-secondary: #6b7280;
  --text-tertiary: #9ca3af;

  /* 边框 */
  --border-color: #e5e7eb;

  /* 交互 */
  --hover-bg: #f3f4f6;
  --active-bg: #e5e7eb;
  --selected-bg: #ede9fe;

  /* 强调色 */
  --accent-color: #6c5ce7;
}
```

### 组件样式原则

- 使用 scoped CSS
- 继承 CSS 变量
- 不引入 CSS 框架
- 终端区域使用 xterm.js 主题系统