# Tauri Capabilities 权限管理

权限配置文件：`src-tauri/capabilities/default.json`

Tauri 2.11.1 升级时同步生成 ACL manifest/schema。上游 `core:app:default`
新增 `supports_multiple_windows`，`core:window:default` 新增 `activity_name` /
`scene_identifier`；CC Desk 当前未使用，主窗口显式 deny 这三个权限以保持既有授权面。
版本管理窗口仍无插件权限。生成 manifest 中出现新接口不等于应用批准了它；
`tests/config/tauriCapabilities.test.ts` 从实际 manifest 展开 default 并核对 allow/deny，
它是配置边界证据，不替代安装版 WebView IPC 验收。

## 查询权限

### 查看 default 权限集包含哪些命令

构建后生成 `src-tauri/gen/schemas/acl-manifests.json`，记录每个插件的 `default_permission`。

```bash
# 查看 core:webview 的 default 权限集
node -e "const m=JSON.parse(require('fs').readFileSync('src-tauri/gen/schemas/acl-manifests.json','utf8')); console.log(JSON.stringify(m['core:webview'].default_permission.permissions,null,2))"
```

### 搜索某个 API 对应的权限标识符

在 `gen/schemas/windows-schema.json` 中搜索，权限标识符格式为 `<plugin>:allow-<command>`。

```bash
grep -i "focus" src-tauri/gen/schemas/windows-schema.json
```

已知对照：

| JS API | 权限标识符 | 是否在 default 中 |
|--------|-----------|------------------|
| `getCurrentWebview().setFocus()` | `core:webview:allow-set-webview-focus` | 否，需显式添加 |
| `getCurrentWindow().setFocus()` | `core:window:allow-set-focus` | 否，需显式添加 |
| `getCurrentWindow().setPosition()` | `core:window:allow-set-position` | 否 |
| `getCurrentWindow().setSize()` | `core:window:allow-set-size` | 否 |
| `getCurrentWindow().setTitle()` | `core:window:allow-set-title` | 否 |
| `getCurrentWindow().currentMonitor()` | `core:window:allow-current-monitor` | 是 |
| `Image.close()` / `Resource.close()` | `core:resources:allow-close` | 主窗口显式授权，不能由其他 core 子集推断 |

## 确认与添加权限

添加新 Tauri JS API 调用时：
1. 在 JS 源码中找到 API 调用
2. 在 `gen/schemas/windows-schema.json` 中搜索对应权限标识符
3. 检查是否已包含在 default 中
4. 不在则需在 `capabilities/default.json` 的 `permissions` 数组中显式添加

**重要**：`<plugin>:default` 只包含基础只读命令，大部分写操作需显式添加。
