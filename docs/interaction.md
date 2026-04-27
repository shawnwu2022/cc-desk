# 交互规则

## 核心原则

**所有键盘输入直接发送到 PTY，由 Claude CLI 处理。**

GUI 层不拦截、不映射、不处理任何 Claude Code 快捷键。

## 快捷键处理机制

### xterm.js onData

```typescript
// XTermTerminal.vue
term.onData(data => {
  // 所有按键（包括 Ctrl/Alt 组合）直接发送
  window.api.ptyInput(ptyId.value, data)
})
```

### Claude CLI 处理的快捷键

| 快捷键 | 功能 | 说明 |
|--------|------|------|
| Ctrl+C | 取消输入/生成 | 标准中断 |
| Ctrl+D | 退出 Claude Code | EOF 信号 |
| Ctrl+L | 清屏 | 重绘终端 |
| Ctrl+R | 反向搜索历史 | 交互式搜索 |
| Ctrl+B | 后台运行任务 | Bash 后台 |
| Ctrl+T | 切换任务列表 | 任务状态 |
| Ctrl+G | 外部编辑器 | 打开编辑器 |
| Ctrl+O | 转录查看器 | 详细工具使用 |
| Ctrl+V | 粘贴图片 | 插入图片引用 |
| Alt+P | 切换模型 | 切换 Claude 模型 |
| Alt+T | 扩展思考 | 启用/禁用 |
| Alt+O | 快速模式 | 切换快速模式 |
| Alt+M | 权限模式循环 | 切换权限模式 |
| Esc+Esc | 回退/总结 | 恢复代码 |

### 特殊处理：Ctrl+W

Ctrl+W 默认会关闭 Electron 窗口，需要特殊处理：

```typescript
// main/index.ts
mainWindow.webContents.on('before-input-event', (event, input) => {
  if (modifiers.includes('control') && key === 'w') {
    event.preventDefault() // 阻止关闭窗口
    mainWindow.webContents.send('terminal:keydown', { key, modifiers })
  }
})

// XTermTerminal.vue
window.api.onTerminalKeydown((event, { key, modifiers }) => {
  if (modifiers.includes('control') && key === 'w') {
    window.api.ptyInput(ptyId.value, '\x17') // 发送 Ctrl+W ASCII
  }
})
```

## GUI 层快捷键

GUI 层只处理**外围功能**的快捷键：

| 快捷键 | 功能 | 处理方 |
|--------|------|--------|
| Ctrl+B | 切换侧边栏 | GUI（与 Claude Ctrl+B 不冲突，因为 Claude 的 Ctrl+B 在终端内） |

**注意**：当焦点在终端时，Ctrl+B 发送到 Claude CLI。侧边栏切换需要点击按钮。

## 文本编辑快捷键

以下由 Claude CLI 的 readline 处理：

| 快捷键 | 功能 |
|--------|------|
| Ctrl+A | 移动到行首 |
| Ctrl+E | 移动到行尾 |
| Ctrl+K | 删除到行尾 |
| Ctrl+U | 删除到行首 |
| Ctrl+W | 删除上一单词 |
| Ctrl+Y | 粘贴已删除文本 |
| Alt+B | 向后移动单词 |
| Alt+F | 向前移动单词 |

## 多行输入

Claude CLI 支持：

| 方法 | 快捷键 |
|------|--------|
| 快速转义 | `\` + Enter |
| Alt 键 | Alt+Enter（需配置） |
| Shift+Enter | Shift+Enter（部分终端支持） |
| 控制序列 | Ctrl+J |

## Slash 命令

在 Claude CLI 中输入 `/` 触发：

- `/clear` — 清除对话
- `/compact` — 压缩上下文
- `/help` — 显示帮助
- `/cost` — 显示费用
- `/model` — 切换模型
- `/config` — 打开配置
- `/init` — 初始化项目
- `/memory` — 内存管理

GUI 层不处理这些命令，完全由 Claude CLI 处理。

## Bash 模式

输入 `!` 开头进入 Bash 模式：

```
! npm test
! git status
```

输出添加到对话上下文，支持 Ctrl+B 后台运行。

## 视图切换

| 场景 | 触发 |
|------|------|
| 启动无收藏 | → WelcomeView |
| 启动有收藏 | → ProjectSelectView |
| 选择项目 | → TerminalView |
| 点击返回 | → ProjectSelectView |
| Escape（终端） | Claude CLI 处理（不退出视图） |

## 鼠标交互

- 终端内文本选择：xterm.js 处理
- 链接点击：WebLinksAddon 处理
- 复制粘贴：Ctrl+C/V 或右键菜单
- 点击侧边栏外部：关闭侧边栏

## GUI 增强边界

| 增强 | 做 | 不做 |
|------|----|----|
| 终端主题 | 浅色 + 预设 | AI 补全 |
| 多终端 | 标签切换 | 复杂布局 |
| 会话历史 | SerializeAddon | 导出文件 |
| 项目收藏 | 快速切换 cwd | 拖拽排序 |
| 快捷命令 | 命令面板（Phase 3） | 拦截 Claude 命令 |
| 搜索 | SearchAddon | 高级过滤 |