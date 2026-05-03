# Tauri Capabilities 权限管理

权限配置文件：`src-tauri/capabilities/default.json`

## 查询权限

### 1. 查看 default 权限集包含哪些命令

构建后生成 `src-tauri/gen/schemas/acl-manifests.json`，其中记录了每个插件的 `default_permission` 和所有可用 `permissions`。

```bash
# 查看 core:webview 的 default 权限集
node -e "const m=JSON.parse(require('fs').readFileSync('src-tauri/gen/schemas/acl-manifests.json','utf8')); console.log(JSON.stringify(m['core:webview'].default_permission.permissions,null,2))"

# 查看 core:window 的 default 权限集
node -e "const m=JSON.parse(require('fs').readFileSync('src-tauri/gen/schemas/acl-manifests.json','utf8')); console.log(JSON.stringify(m['core:window'].default_permission.permissions,null,2))"
```

### 2. 搜索某个 API 对应的权限标识符

在 `gen/schemas/windows-schema.json` 中搜索，权限标识符格式为 `<plugin>:allow-<command>`。

```bash
# 例：搜索 focus 相关权限
grep -i "focus" src-tauri/gen/schemas/windows-schema.json
```

已知对照：

| JS API | 权限标识符 | 所属 default 集 |
|--------|-----------|----------------|
| `getCurrentWebview().setFocus()` | `core:webview:allow-set-webview-focus` | 不在 default 中，需显式添加 |
| `getCurrentWindow().setFocus()` | `core:window:allow-set-focus` | 不在 default 中，需显式添加 |
| `getCurrentWindow().onFocusChanged()` | 事件监听，由 `core:event:default` 覆盖 | 已包含 |
| `getCurrentWindow().setPosition()` | `core:window:allow-set-position` | 不在 default 中 |
| `getCurrentWindow().setSize()` | `core:window:allow-set-size` | 不在 default 中 |
| `getCurrentWindow().setTitle()` | `core:window:allow-set-title` | 不在 default 中 |
| `getCurrentWindow().currentMonitor()` | `core:window:allow-current-monitor` | 在 default 中 |

## 确认权限

添加新 Tauri JS API 调用时，按以下步骤确认权限：

1. 在 JS 源码中找到 API 调用（如 `getCurrentWebview().setFocus()`）
2. 在 `gen/schemas/windows-schema.json` 中搜索对应权限标识符
3. 用 node 命令检查该权限是否已包含在对应插件的 `default_permission` 中
4. 如果不在 default 中，需在 `capabilities/default.json` 的 `permissions` 数组中显式添加

## 添加权限

编辑 `src-tauri/capabilities/default.json`，在 `permissions` 数组中添加权限标识符字符串。

**重要**：`<plugin>:default` 只包含一组基础只读命令，大部分写操作（set-focus、set-size、set-position 等）都不在 default 中，必须显式添加。
