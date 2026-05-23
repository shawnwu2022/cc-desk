# 手动测试条目

自动化测试无法覆盖的 UI 交互与端到端测试。按功能模块分类。

---

## i18n 多语言支持

### I18n_LanguageSwitch_001 — 设置中切换语言立即生效

**目标**：验证在设置 > 外观中切换语言后，所有 UI 文本立即更新

**前置条件**：应用已启动，语言为 English

**操作步骤**：
1. 打开 Settings（`Ctrl+,` 或点击图标栏齿轮图标）
2. 在 Appearance 区域找到 Language 选项
3. 点击「中文」卡片

**预期结果**：
- 设置导航标签立即变为中文：外观、启动、接口、快捷键、更新、关于
- 页面标题「Appearance」变为「外观」
- Font Size 标签变为「字体大小」
- Theme 标签变为「主题」
- Light/Dark 标签变为「浅色」/「深色」
- Language 标签变为「语言」

### I18n_LanguageSwitch_002 — 切换到英文后各页面文本正确

**目标**：切换到英文后，验证所有主要页面的文本都为英文

**前置条件**：语言已切换为中文

**操作步骤**：
1. 在设置中切换回「English」
2. 依次点击设置导航中的每个 section（Appearance、Startup、Providers、Shortcuts、Update、About）
3. 关闭设置，查看图标栏 tooltip
4. 打开项目，查看侧边栏面板（Sessions、Skills、Agents、MCP Servers、Plugins）

**预期结果**：
- 所有页面标题、描述、按钮文本为英文
- 无残留中文字符
- 图标栏 tooltip 为英文

### I18n_LanguageSwitch_003 — 切换到中文后各页面文本正确

**目标**：切换到中文后，验证所有主要页面的文本都为中文

**前置条件**：应用已启动

**操作步骤**：
1. 设置 > 外观中切换为「中文」
2. 依次点击设置导航中的每个 section
3. 关闭设置，查看图标栏 tooltip
4. 打开侧边栏面板
5. 查看 Provider 设置中的按钮和对话框

**预期结果**：
- 所有页面标题、描述、按钮文本为中文
- Provider 卡片的按钮：使用、编辑、测试、删除 为中文
- 删除确认对话框为中文
- 技术术语（Claude Code、MCP、Provider 等）保持英文

### I18n_LanguageSwitch_004 — 重启后语言设置保持

**目标**：切换语言后重启应用，语言设置持久化

**前置条件**：应用已启动

**操作步骤**：
1. 设置 > 外观中切换为「中文」
2. 关闭并重新启动应用
3. 打开设置查看语言选项

**预期结果**：
- 重启后 UI 文本全部为中文
- 语言选择器中「中文」卡片为选中状态
- `~/.cc-box/config.json` 中 `language` 字段为 `"zh"`

### I18n_LanguageSwitch_005 — 欢迎页和项目选择页文本随语言切换

**目标**：欢迎页和项目选择页的文本也随语言变化

**前置条件**：应用已启动，语言为 English

**操作步骤**：
1. 设置中切换为「中文」
2. 回到欢迎页（关闭所有项目）
3. 查看标题、副标题、按钮文本
4. 打开项目选择页，查看搜索框、按钮、分组标题

**预期结果**：
- 欢迎页标题保持「Claude Code」（品牌名不翻译）
- 副标题变为中文
- 按钮文本变为中文
- 项目选择页搜索框 placeholder 为中文
- 「近期会话」「项目」「启动选项」等标签为中文

### I18n_LanguageSwitch_006 — 环境检查页面文本随语言切换

**目标**：首次安装时的环境检查界面也支持多语言

**前置条件**：删除 Claude CLI 或 Git（或模拟环境检查失败）

**操作步骤**：
1. 设置语言为中文
2. 重启应用触发环境检查

**预期结果**：
- 环境检查标题为中文
- 「自动安装」按钮为中文
- 「重试」按钮为中文
- 安装状态消息为中文

### I18n_KeyboardShortcuts_007 — 快捷键面板文本随语言切换

**目标**：Shortcuts 设置页和快捷键弹窗文本随语言切换

**前置条件**：应用已启动

**操作步骤**：
1. 设置语言为中文
2. 打开设置 > 快捷键页面
3. 按 `Ctrl+Shift+/` 打开快捷键弹窗

**预期结果**：
- 快捷键分组标题为中文（应用快捷键、会话管理等）
- 快捷键描述为中文
- 弹窗标题为中文

---

## PTY 环境变量注入

环境变量从 PTY 启动时注入进程，不写入 `~/.claude/settings.json`。以下测试验证注入行为、设置面板交互、与 Provider 的兼容性。

### EnvVars_PtyInject_001 — 新建 Claude 终端默认环境变量已注入

**目标**：验证默认的 6 个环境变量在新建 Claude 终端时正确注入

**前置条件**：应用已启动，环境变量为默认值（未修改过）

**操作步骤**：
1. 选择一个项目，新建 Claude 终端
2. 等待终端出现输入提示符
3. 在终端中逐个输入以下命令查看值：
   - `echo $LANG`
   - `echo $LC_ALL`
   - `echo $PYTHONUTF8`
   - `echo $CLAUDE_CODE_SCROLL_SPEED`
   - `echo $PYTHONIOENCODING`
   - `echo $CLAUDE_CODE_NO_FLICKER`

**预期结果**：
- `LANG` → `en_US.UTF-8`
- `LC_ALL` → `en_US.UTF-8`
- `PYTHONUTF8` → `1`
- `CLAUDE_CODE_SCROLL_SPEED` → `5`
- `PYTHONIOENCODING` → `utf-8`
- `CLAUDE_CODE_NO_FLICKER` → `1`

### EnvVars_PtyInject_002 — 新建 Shell 终端环境变量已注入

**目标**：验证环境变量在 Shell 终端（非 Claude）中同样注入

**前置条件**：应用已启动

**操作步骤**：
1. 选择一个项目，新建 Shell 终端（通过终端标题栏的 `+` 按钮下拉选择 Shell）
2. 在终端中输入 `echo $CLAUDE_CODE_NO_FLICKER`
3. 输入 `echo $PYTHONUTF8`

**预期结果**：
- `CLAUDE_CODE_NO_FLICKER` → `1`
- `PYTHONUTF8` → `1`

### EnvVars_SettingsEdit_003 — 设置面板修改值后新终端生效

**目标**：在设置面板修改环境变量的值，新建终端后生效

**前置条件**：应用已启动

**操作步骤**：
1. 打开 Settings → Startup → Environment Variables
2. 将 `PYTHONUTF8` 的值从 `1` 改为 `0`
3. 关闭设置，回到终端
4. 新建一个 Claude 终端
5. 输入 `echo $PYTHONUTF8`

**预期结果**：
- 新终端输出 `0`
- 已运行的终端不受影响（仍为旧值）

### EnvVars_SettingsAddRemove_004 — 设置面板新增/删除环境变量

**目标**：验证新增和删除环境变量后，新终端正确反映变更

**前置条件**：应用已启动

**操作步骤**：
1. 打开 Settings → Startup → Environment Variables
2. 点击 `+` 按钮，添加 `MY_TEST_VAR` = `hello`
3. 关闭设置，新建 Claude 终端
4. 输入 `echo $MY_TEST_VAR`
5. 回到设置，删除 `MY_TEST_VAR` 这一行
6. 再新建一个 Claude 终端
7. 输入 `echo $MY_TEST_VAR`

**预期结果**：
- 第一个新终端输出 `hello`
- 第二个新终端输出空（变量不存在）

### EnvVars_SettingsRename_005 — 设置面板重命名 key

**目标**：验证修改 key 名称后，旧 key 不再注入，新 key 注入

**前置条件**：应用已启动

**操作步骤**：
1. 打开 Settings → Startup → Environment Variables
2. 新增一行 `TEST_OLD_KEY` = `test_value`
3. 关闭设置，新建终端验证 `echo $TEST_OLD_KEY` → `test_value`
4. 回到设置，将 `TEST_OLD_KEY` 的 key 改为 `TEST_NEW_KEY`（点击 key 名编辑）
5. 关闭设置，新建终端
6. 输入 `echo $TEST_OLD_KEY`
7. 输入 `echo $TEST_NEW_KEY`

**预期结果**：
- `TEST_OLD_KEY` 输出空
- `TEST_NEW_KEY` → `test_value`

### EnvVars_NoSettingsJsonWrite_006 — 不写入 ~/.claude/settings.json

**目标**：验证环境变量的修改不再写入 Claude Code 的 settings.json

**前置条件**：应用已启动

**操作步骤**：
1. 先记录 `~/.claude/settings.json` 当前内容（若文件存在）
2. 在 cc-box 设置中修改 `PYTHONUTF8` 为 `0`，新增 `MY_TEST` = `val`
3. 新建终端（触发 PTY 注入）
4. 重新打开 `~/.claude/settings.json` 查看内容
5. 检查 `env` 字段中是否包含 `PYTHONUTF8` 或 `MY_TEST`

**预期结果**：
- `settings.json` 的 `env` 字段中**没有** `PYTHONUTF8` 或 `MY_TEST`（如果 `env` 字段存在的话）
- 如果之前通过旧版 cc-box 写入过这些 key，它们可能仍残留，但本次修改不会更新它们

### EnvVars_ProviderNoConflict_007 — Provider 激活后环境变量仍生效

**目标**：验证激活 Provider 后，cc-box 环境变量注入不受影响

**前置条件**：至少配置了一个 Provider

**操作步骤**：
1. 激活一个 Provider（在 Settings → Providers 中点击「Use」）
2. 新建 Claude 终端
3. 输入 `echo $CLAUDE_CODE_NO_FLICKER`
4. 输入 `echo $PYTHONUTF8`

**预期结果**：
- `CLAUDE_CODE_NO_FLICKER` → `1`
- `PYTHONUTF8` → `1`
- 两者均不受 Provider 激活影响（Provider 写 settings.json，cc-box 注入 PTY 进程，互不干扰）

### EnvVars_PersistAcrossRestart_008 — 重启后环境变量配置保持

**目标**：验证环境变量配置在重启应用后保持不变

**前置条件**：应用已启动

**操作步骤**：
1. 打开 Settings → Startup → Environment Variables
2. 新增 `PERSIST_TEST` = `survived`
3. 关闭应用
4. 重新启动应用
5. 打开 Settings → Startup → Environment Variables，查看列表
6. 新建终端，输入 `echo $PERSIST_TEST`

**预期结果**：
- 设置面板中 `PERSIST_TEST` = `survived` 仍在
- 新终端输出 `survived`
- `~/.cc-box/config.json` 中 `claudeEnvVars` 包含 `PERSIST_TEST`

---

## 应用更新

### Update_OneClick_001 — 一键更新完整流程

**目标**：验证有新版本时，点击"下载并安装"能完成下载→安装→重启

**前置条件**：当前版本低于远程最新版本（可在 tauri.conf.json 中临时降低 version 触发）

**操作步骤**：
1. 启动应用，等待自动检查更新
2. 观察图标栏设置图标是否出现红色角标
3. 点击设置图标，确认自动跳转到"更新"设置页
4. 查看新版本号、release notes 内容
5. 点击「下载并安装」按钮
6. 观察进度条从 0% 推进到 100%
7. 观察状态从"下载中"变为"安装中"
8. 等待应用自动重启

**预期结果**：
- 设置图标右上角出现红色小圆点（脉冲动画）
- 点击设置图标直接跳转到更新 section
- 进度条正常推进，显示百分比和已下载/总大小
- 安装阶段显示脉冲动画文字和"将自动重启"提示
- 应用自动重启后版本号已更新

### Update_ConfirmActivePtys_002 — 有活跃 PTY 时更新确认提示

**目标**：有终端在运行时点击更新，弹出确认对话框

**前置条件**：应用已启动，至少有一个 Claude 终端在运行

**操作步骤**：
1. 打开一个项目，新建 Claude 终端，等待 Claude CLI 启动
2. 设置图标应显示红色更新角标
3. 点击设置图标进入更新页
4. 点击「下载并安装」

**预期结果**：
- 弹出系统确认对话框，内容提示"有正在运行的终端会话，更新将重启应用并关闭所有会话"
- 点击「取消」→ 不执行更新，停留在更新页面
- 再次点击「下载并安装」→ 点击「确定」→ 正常进入下载流程

### Update_ConfirmNoPtys_003 — 无活跃 PTY 时不弹确认

**目标**：没有终端运行时点击更新，直接进入下载流程

**前置条件**：应用已启动，未打开任何项目/终端

**操作步骤**：
1. 在项目选择页（或欢迎页），不打开任何终端
2. 点击设置图标（有红色角标时）进入更新页
3. 点击「下载并安装」

**预期结果**：
- 不弹出任何确认对话框
- 直接进入下载进度界面

### Update_ManualDownload_004 — 手动下载按钮打开 GitHub

**目标**：验证"手动下载"按钮在浏览器中打开正确的 Releases 页面

**前置条件**：应用已启动，检测到有新版本

**操作步骤**：
1. 进入设置 > 更新页
2. 找到「手动下载」按钮
3. 点击该按钮

**预期结果**：
- 系统默认浏览器打开 `https://github.com/orczh-hj/cc-box/releases`
- 页面正常加载，能看到各版本的 Release 列表
- 按钮样式为边框按钮（非主按钮），与"下载并安装"并排

### Update_ErrorRetry_005 — 更新失败后重试

**目标**：验证下载失败时显示错误信息，重试能重新下载

**前置条件**：当前版本低于远程最新版本

**操作步骤**：
1. 进入更新页，点击「下载并安装」
2. 下载过程中断开网络（或模拟网络故障）
3. 等待下载失败
4. 查看错误信息显示
5. 恢复网络，点击「重试」链接

**预期结果**：
- 显示红色错误提示区域，包含具体错误信息
- 出现「重试」链接按钮
- 点击重试后重新开始下载流程（进度归零重新推进）

### Update_UpToDate_006 — 已是最新版本时的反馈

**目标**：验证没有新版本时，检查更新给出正确反馈

**前置条件**：当前版本等于远程最新版本

**操作步骤**：
1. 进入设置 > 更新页
2. 点击「检查更新」按钮

**预期结果**：
- 按钮显示旋转图标 + "检查中..."文字
- 检查完成后显示绿色提示"已是最新版本！"
- 不显示"下载并安装"按钮

### Update_ManualDownloadI18n_007 — 更新页文本随语言切换

**目标**：验证更新页所有文本随语言切换正确变化

**前置条件**：应用已启动，检测到有新版本

**操作步骤**：
1. 设置 > 外观中切换为中文
2. 进入更新页查看所有文本
3. 切换回 English
4. 再次查看更新页

**预期结果**：
- 中文时：软件更新、当前版本、检查更新、版本可用、更新内容、下载并安装、手动下载、已是最新版本
- 英文时：Software Update、Current Version、Check for Updates、is available、What's New、Download & Install、Manual Download、You're up to date!
- 有 PTY 时确认对话框文本也随语言切换

---

## 时间格式显示

### TimeFormat_TimeAgo_001 — 会话列表时间显示正确

**目标**：验证侧边栏会话列表中的相对时间文本正确

**前置条件**：应用已启动，有历史会话

**操作步骤**：
1. 打开一个项目，查看侧边栏会话列表
2. 观察 1 分钟内的会话显示"刚刚"（中文）或"Now"（英文）
3. 观察 5 分钟前的会话显示"5 分钟前"或"5 minutes ago"
4. 观察 2 小时前的会话显示"2 小时前"或"2 hours ago"
5. 观察 3 天前的会话显示"3 天前"或"3 days ago"

**预期结果**：
- 时间文本随当前语言正确显示
- 各时间段使用正确的格式（刚刚/N分钟前/N小时前/N天前）
- 切换语言后时间文本立即更新

---

## 资源管理器右键菜单

### ContextMenu_FolderRightClick_001 — 右键文件夹显示"使用 CC-Box 打开"

**目标**：验证在 Windows 资源管理器中右键文件夹时出现"使用 CC-Box 打开"菜单项

**前置条件**：CC-Box 已通过 NSIS 安装包安装（非 portable/开发模式）

**操作步骤**：
1. 打开 Windows 资源管理器
2. 右键点击任意文件夹

**预期结果**：
- 右键菜单中出现"使用 CC-Box 打开"
- 菜单项左侧显示 CC-Box 图标

### ContextMenu_BackgroundRightClick_002 — 右键空白处显示"在此处打开 CC-Box"

**目标**：验证在文件夹内空白区域右键时出现"在此处打开 CC-Box"菜单项

**前置条件**：CC-Box 已通过 NSIS 安装包安装

**操作步骤**：
1. 打开 Windows 资源管理器
2. 进入一个文件夹（如 `D:\projects`）
3. 在文件列表的空白区域右键

**预期结果**：
- 右键菜单中出现"在此处打开 CC-Box"
- 菜单项左侧显示 CC-Box 图标

### ContextMenu_OpenExistingProject_003 — 右键打开已有项目

**目标**：右键点击一个 CC-Box 中已有缓存的项目文件夹，CC-Box 启动后直接打开该项目

**前置条件**：CC-Box 已安装，`~/.claude/projects/` 中存在该项目的会话记录

**操作步骤**：
1. 确认 CC-Box 未运行
2. 在资源管理器中右键一个之前使用过的项目文件夹
3. 点击"使用 CC-Box 打开"
4. 等待 CC-Box 启动

**预期结果**：
- CC-Box 启动后直接进入终端视图（不显示欢迎页或项目选择页）
- 终端工作目录为右键点击的文件夹
- 侧边栏显示该项目的会话数据

### ContextMenu_OpenNewProject_004 — 右键打开新项目

**目标**：右键点击一个 CC-Box 中没有缓存记录的文件夹，作为新项目打开

**前置条件**：CC-Box 已安装，目标文件夹不在 `~/.claude/projects/` 中

**操作步骤**：
1. 确认 CC-Box 未运行
2. 创建一个新文件夹（如 `D:\test-new-project`）
3. 右键该文件夹，点击"使用 CC-Box 打开"
4. 等待 CC-Box 启动

**预期结果**：
- CC-Box 启动后直接进入终端视图
- 终端工作目录为 `D:\test-new-project`
- Claude CLI 在该目录下启动新会话（不尝试 resume）

### ContextMenu_PathWithSpaces_005 — 路径含空格时正确打开

**目标**：验证路径中包含空格时右键菜单能正确传递目录

**前置条件**：CC-Box 已安装

**操作步骤**：
1. 创建一个名称含空格的文件夹（如 `D:\My Projects\web app`）
2. 右键该文件夹，点击"使用 CC-Box 打开"

**预期结果**：
- CC-Box 启动后工作目录为 `D:\My Projects\web app`
- 路径完整无误，未截断

### ContextMenu_ChinesePath_006 — 中文路径正确打开

**目标**：验证路径中包含中文字符时右键菜单能正确传递目录

**前置条件**：CC-Box 已安装

**操作步骤**：
1. 创建一个中文名称的文件夹（如 `D:\项目\前端开发`）
2. 右键该文件夹，点击"使用 CC-Box 打开"

**预期结果**：
- CC-Box 启动后工作目录为中文路径
- Claude CLI 正常启动，路径无乱码

### ContextMenu_UninstallCleanup_007 — 卸载后右键菜单项消失

**目标**：验证通过 NSIS 卸载 CC-Box 后，右键菜单项被清除

**前置条件**：CC-Box 已安装且右键菜单正常工作

**操作步骤**：
1. 通过系统"设置 > 应用"或卸载程序卸载 CC-Box
2. 在资源管理器中右键文件夹
3. 在文件夹空白区域右键

**预期结果**：
- 右键菜单中不再出现"使用 CC-Box 打开"和"在此处打开 CC-Box"
- 注册表 `HKCU\Software\Classes\Directory\shell\cc-box` 和 `HKCU\Software\Classes\Directory\Background\shell\cc-box` 不存在

### ContextMenu_BackgroundDir_008 — 右键空白处传入当前目录

**目标**：验证右键文件夹空白处时，传入的是当前目录而非空值

**前置条件**：CC-Box 已安装

**操作步骤**：
1. 在资源管理器中进入 `D:\projects\my-app` 目录
2. 在文件列表空白区域右键
3. 点击"在此处打开 CC-Box"

**预期结果**：
- CC-Box 启动后工作目录为 `D:\projects\my-app`
- 不是父目录 `D:\projects`

### ContextMenu_CliDirectInvoke_009 — 命令行直接传入目录路径

**目标**：验证通过命令行直接调用 `cc-box.exe <dir>` 也能正确打开（无需右键菜单）

**前置条件**：CC-Box 已安装，`cc-box.exe` 在 PATH 中或使用完整路径

**操作步骤**：
1. 打开命令提示符或 PowerShell
2. 执行 `"C:\Program Files\CC-Box\cc-box.exe" "D:\projects\test"`

**预期结果**：
- CC-Box 启动后工作目录为 `D:\projects\test`
- 行为与右键菜单打开一致

---

## Claude CLI 更新与安装

### ClaudeCli_VersionDetect_WinNpm_001 — Windows npm 安装的 claude 版本检测

**目标**：验证通过 npm 全局安装的 claude（`.cmd` 文件）能被正确检测到版本号

**前置条件**：Windows 系统，claude 通过 `npm install -g @anthropic-ai/claude-code` 安装

**操作步骤**：
1. 启动 CC-Box，确认启动检查通过（显示 Claude CLI ✓）
2. 进入设置 > 更新页
3. 点击 Claude CLI 区域的「检查更新」按钮

**预期结果**：
- 版本号区域显示 `vX.Y.Z`（而非"未安装"）
- 检查结果正确显示是否有更新可用

### ClaudeCli_VersionDetect_MacArm_002 — macOS ARM 版本检测

**目标**：验证 Apple Silicon Mac 上 claude 版本能被正确检测

**前置条件**：macOS Apple Silicon，claude 通过任意方式安装

**操作步骤**：
1. 启动 CC-Box
2. 进入设置 > 更新页
3. 点击 Claude CLI 区域的「检查更新」按钮

**预期结果**：
- 版本号区域显示 `vX.Y.Z`
- 不显示"未安装"

### ClaudeCli_InstallPlatform_MacArm_003 — macOS ARM 下载正确架构

**目标**：验证 Apple Silicon Mac 下载安装 `darwin-arm64` 二进制文件

**前置条件**：macOS Apple Silicon，有新版本可用

**操作步骤**：
1. 进入设置 > 更新页
2. 点击 Claude CLI 的「下载并安装」
3. 等待安装完成
4. 在终端执行 `file ~/.local/bin/claude`

**预期结果**：
- 下载进度正常推进
- 安装完成后版本号更新
- `file` 命令输出包含 `arm64` 或 `Mach-O 64-bit executable arm64`
- 无 "Platform xxx not supported" 错误

### ClaudeCli_InstallPlatform_MacIntel_004 — macOS Intel 下载正确架构

**目标**：验证 Intel Mac 下载安装 `darwin-x64` 二进制文件

**前置条件**：macOS Intel

**操作步骤**：
1. 进入设置 > 更新页
2. 点击 Claude CLI 的「下载并安装」
3. 等待安装完成
4. 在终端执行 `file ~/.local/bin/claude`

**预期结果**：
- 下载 `darwin-x64` 二进制文件
- `file` 命令输出包含 `x86_64`

### ClaudeCli_InstallPlatform_LinuxArm64_005 — Linux ARM64 下载正确架构

**目标**：验证 ARM64 Linux 桌面下载安装 `linux-arm64` 二进制文件

**前置条件**：Linux ARM64 桌面环境

**操作步骤**：
1. 启动 CC-Box（从桌面环境启动，非终端）
2. 进入设置 > 更新页
3. 点击「下载并安装」
4. 在终端执行 `file ~/.local/bin/claude`

**预期结果**：
- 下载 `linux-arm64` 二进制文件（不是 `linux-x64`）
- `file` 命令输出包含 `aarch64`

### ClaudeCli_PathPriority_Windows_006 — Windows 安装后 PATH 优先级最高

**目标**：验证安装完成后 `~/.local/bin` 在 PATH 最前面，优先于其他 claude

**前置条件**：Windows 系统，PATH 中已有其他 claude（如 npm 全局安装的）

**操作步骤**：
1. 进入设置 > 更新页，安装或更新 Claude CLI
2. 安装完成后，打开 cmd 执行 `where claude`
3. 观察 `claude.exe` 出现的顺序

**预期结果**：
- `%USERPROFILE%\.local\bin\claude.exe` 排在第一位
- 用户 PATH 环境变量中 `~/.local/bin` 位于最前面

### ClaudeCli_PathPriority_Mac_007 — macOS 安装后 PATH 持久化

**目标**：验证安装完成后 `~/.local/bin` 被持久写入 shell 配置

**前置条件**：macOS，默认 shell 为 zsh

**操作步骤**：
1. 进入设置 > 更新页，安装或更新 Claude CLI
2. 检查 `~/.zshenv` 文件内容
3. 打开新终端窗口，执行 `which claude`
4. 重启 CC-Box，确认启动检查仍通过

**预期结果**：
- `~/.zshenv` 中包含 `export PATH="$HOME/.local/bin:$PATH"`
- 新终端中 `which claude` 返回 `/Users/xxx/.local/bin/claude`
- 重启后启动检查通过

### ClaudeCli_PathPriority_Linux_008 — Linux 安装后 PATH 持久化

**目标**：验证安装完成后 `~/.local/bin` 被持久写入 shell 配置

**前置条件**：Linux，默认 shell 为 bash

**操作步骤**：
1. 进入设置 > 更新页，安装或更新 Claude CLI
2. 检查 `~/.bashrc` 文件内容
3. 打开新终端窗口，执行 `which claude`
4. 重启 CC-Box，确认启动检查仍通过

**预期结果**：
- `~/.bashrc` 中包含 `export PATH="$HOME/.local/bin:$PATH"`
- 新终端中 `which claude` 返回 `/home/xxx/.local/bin/claude`
- 重启后启动检查通过

### ClaudeCli_ReinstallPathPriority_009 — 重复安装后 PATH 不重复

**目标**：验证多次安装/更新后 PATH 中不会出现重复条目

**前置条件**：已安装 Claude CLI

**操作步骤**：
1. 进入设置 > 更新页，点击安装/更新
2. 安装完成后，再次点击安装/更新
3. 检查用户 PATH（Windows: 用户环境变量；macOS: `~/.zshenv`）

**预期结果**：
- PATH 中只有一条 `~/.local/bin` 相关条目
- 该条目在 PATH 最前面
