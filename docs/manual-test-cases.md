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

---

## Claude CLI 历史版本切换

### Claude_Version_StartupNoHttp_001 — 启动检查零 HTTP 请求

**目标**：验证启动应用时不再调用 OSS `deps/claude/latest.json`，只读本地 Claude 版本号

**前置条件**：本机已安装 Claude CLI（任意版本）

**操作步骤**：
1. 启动 Fiddler/Wireshark 等抓包工具，过滤 `cc-box.oss-cn-beijing.aliyuncs.com`
2. 断网或保持抓包监听，启动 CC-Box
3. 等待应用完全启动（进入项目选择页或终端页）

**预期结果**：
- 应用启动速度与有网环境一致（不再等待 Claude latest.json 响应）
- 抓包记录中没有对 `deps/claude/latest.json` 的请求
- 抓包记录中仍可能有对 `cc-box/latest.json` 的请求（CC-Box 自身更新检查，保留）

### Claude_Version_StartupLocalVersion_002 — 启动显示本地 Claude 版本

**目标**：验证启动后进入设置 > 更新，Claude CLI 卡片显示本地实际版本号

**前置条件**：本机已安装 Claude CLI（如 `claude --version` 输出 `1.0.33`）

**操作步骤**：
1. 启动 CC-Box
2. 打开 Settings → Update section
3. 查看 Claude CLI 卡片顶部

**预期结果**：
- 卡片顶部显示 `Claude CLI` 与 `v1.0.33`（或当前实际版本）
- 未触发任何加载状态（直接显示版本号）

### Claude_Version_NotInstalled_003 — 未安装时显示「未安装」

**目标**：本机未装 Claude 时，启动后 Claude CLI 卡片显示「未安装」且版本列表正常加载

**前置条件**：本机 PATH 中无 `claude` 命令

**操作步骤**：
1. 启动 CC-Box
2. 打开 Settings → Update section

**预期结果**：
- Claude CLI 卡片顶部显示「未安装」
- 下方历史版本列表正常加载，可下载任意版本
- 不会有「已安装」徽标显示

### Claude_Version_List_004 — 历史版本列表正确展示

**目标**：验证 Claude CLI 卡片能从 OSS 拉取并展示所有历史版本

**前置条件**：能访问 `https://cc-box.oss-cn-beijing.aliyuncs.com/deps/claude/versions.json`

**操作步骤**：
1. 启动 CC-Box，进入 Settings → Update section
2. 等待 Claude CLI 卡片的版本列表加载完成

**预期结果**：
- 列表头部显示「历史版本」标题与版本计数（如「共 N 个版本，最新 v1.0.17」）
- 每行显示版本号、发布日期
- 当前已安装版本行右侧有绿色「已安装」徽标
- 最新版本行右侧有蓝色「最新」徽标
- 列表按版本号降序排列（1.0.17 → 1.0.16 → ...）

### Claude_Version_Refresh_005 — 手动刷新版本列表

**目标**：点击「刷新版本列表」按钮强制重新拉取

**前置条件**：版本列表已加载

**操作步骤**：
1. 点击 Claude CLI 卡片右上角「刷新版本列表」按钮

**预期结果**：
- 按钮显示旋转图标 + 「检查中...」文字
- 列表短暂闪烁后重新加载
- 5 分钟内重复点击会触发实际 HTTP 请求（缓存被强制跳过）

### Claude_Version_Download_006 — 下载历史版本到本地

**目标**：选择某个历史版本下载并自动安装，验证整个流程

**前置条件**：版本列表已加载，有可下载的版本；本机当前未运行 Claude CLI（即所有终端会话已关闭）

**操作步骤**：
1. 选择一个版本（如 v2.1.170），点击右侧「安装」按钮
2. 观察行内进度条
3. 等待下载完成 → 自动进入安装

**预期结果**：
- 按钮变为行内进度条 + 取消按钮，显示百分比
- 进度从 0% 推进到 100%
- 下载完成后短暂显示「正在安装 v2.1.170...」
- 安装完成后按钮变为「在文件夹中显示」
- 卡片顶部「使用中」徽标移到 v2.1.170 这一行
- 用户下载目录中存在 `claude-2.1.170.exe`（Windows）或 `claude-2.1.170`（Unix）
- 文件大小与 OSS 上对应版本一致

### Claude_Version_PlatformMissing_007 — 平台不支持时禁用下载

**目标**：当某版本缺失当前平台产物时，禁用下载按钮并显示提示

**前置条件**：手动改 OSS versions.json，删除某版本的 `win32-x64` 平台条目（仅测试时）

**操作步骤**：
1. 刷新版本列表
2. 查看被改动的版本行

**预期结果**：
- 该版本行的下载按钮变为「当前平台不支持此版本」灰色文字
- 鼠标悬停显示 tooltip
- 其他版本行不受影响

### Claude_Version_VersionsJsonMissing_008 — versions.json 不存在时降级

**目标**：OSS 上 versions.json 被删除时，UI 不崩溃且显示明确提示

**前置条件**：手动删除 OSS `deps/claude/versions.json`（仅测试时）

**操作步骤**：
1. 启动 CC-Box，进入 Update section

**预期结果**：
- 版本列表区域显示「暂无可用版本」灰底提示框
- 卡片不崩溃、其他区域（CC-Box 更新检查）仍正常工作
- 终端功能不受影响

### Claude_Version_DownloadError_009 — 下载失败后可重试

**目标**：网络中断或文件不存在时，显示错误并提供重试

**前置条件**：版本列表已加载

**操作步骤**：
1. 选择一个版本点击下载
2. 下载过程中断网或手动改 OSS 上该版本路径使请求 404

**预期结果**：
- 进度条变为错误状态，显示「下载失败」与「重试」按钮
- 点击「重试」后重新进入下载流程

### Claude_Version_OldCcBoxCompat_010 — 旧版 CC-Box 仍能更新 Claude

**目标**：保证旧版 CC-Box（不读 versions.json）的自动安装 Claude 流程未受影响

**前置条件**：使用改动前的 CC-Box 安装包（v0.11.1 或更早）

**操作步骤**：
1. 安装并启动旧版 CC-Box
2. 进入 Update section，点击 Claude CLI 的「检查更新」/「安装」

**预期结果**：
- 旧版仍能拉取 `deps/claude/latest.json` 并显示最新版本
- 点击「下载并安装」可成功安装最新版 Claude
- 整个流程无错误

### Claude_Version_CancelDownload_012 — 下载过程可取消

**目标**：下载进行中点击「取消」立即终止下载，并清理半成品

**前置条件**：版本列表已加载，下载目录无对应版本的缓存文件

**操作步骤**：
1. 点击某个版本「安装」按钮
2. 进度推进到 30%-60% 之间时，点击进度条旁的「取消」按钮
3. 等待状态切换

**预期结果**：
- 进度条立即消失，按钮恢复为「安装」
- 显示「已取消」灰色文字与「安装」按钮（可重新发起）
- 用户下载目录中无半成品文件（`claude-X.X.X.exe` 不存在）
- 后端日志记录 `Download cancelled`

### Claude_Version_LocalCacheReuse_013 — 本地缓存可复用

**目标**：用户下载目录已有完整文件时，跳过下载直接安装

**前置条件**：之前已成功下载过 v2.1.170，下载目录 `claude-2.1.170.exe` 仍存在且大小匹配

**操作步骤**：
1. 进入 Update section，点击 v2.1.170 行的「安装」按钮
2. 观察状态变化

**预期结果**：
- 不出现下载进度条
- 直接进入「正在安装 v2.1.170...」
- 安装完成后显示「在文件夹中显示」按钮
- 后端日志记录 `Reusing local cache`

### Claude_Version_LocalCacheInvalid_014 — 本地缓存损坏时重新下载

**目标**：本地文件 size 与 OSS 记录不一致时，自动删除并重新下载

**前置条件**：用户下载目录存在 `claude-2.1.170.exe` 但 size 小于实际大小（模拟下载中断）

**操作步骤**：
1. 手动用 `echo > claude-2.1.170.exe` 等方式制造 size 不匹配的文件
2. 点击「安装」按钮

**预期结果**：
- 后端日志记录 `Local file size mismatch, removing`
- 自动删除损坏文件并重新下载
- 流程正常完成

### Claude_Version_ClaudeRunning_015 — Claude 运行时弹窗确认

**目标**：用户在 Claude 终端会话运行时点击安装，提示终止并让用户选择

**前置条件**：在 CC-Box 中打开至少一个 Claude 终端会话（PTY 中运行 claude.exe）

**操作步骤**：
1. 下载完成后，安装阶段会检测到 claude 进程
2. 观察弹窗

**预期结果**：
- 出现确认对话框，标题「检测到 Claude 进程正在运行」
- 提示「继续安装需要终止所有正在运行的 Claude 终端会话」
- 「终止并安装」按钮 + 「取消」按钮
- 点击「取消」后状态回退为「在文件夹中显示」（下载文件保留）
- 点击「终止并安装」后所有 Claude 终端退出 → 文件被覆盖 → 「使用中」徽标更新

### Claude_Version_CancelButtonDuringInstall_016 — 安装阶段无取消按钮

**目标**：确认下载完成后进入安装阶段时不再显示取消按钮（防止部分文件覆盖）

**前置条件**：版本列表已加载，无 Claude 进程运行

**操作步骤**：
1. 点击「安装」
2. 等待下载完成进入安装阶段

**预期结果**：
- 下载阶段：进度条 + 「取消」按钮
- 安装阶段：显示「正在安装...」文字，无取消按钮
- 安装完成后：变为「在文件夹中显示」

### Claude_Version_PublishScript_011 — 发布脚本正确生成 versions.json

**目标**：验证 `npm run download-deps` 执行后，OSS 上 versions.json 被正确更新

**前置条件**：配置好 `scripts/oss-config.json` 与代理

**操作步骤**：
1. 运行 `npm run download-deps`
2. 浏览器访问 `https://cc-box.oss-cn-beijing.aliyuncs.com/deps/claude/versions.json`

**预期结果**：
- 脚本日志输出「versions.json 已更新（共 N 个版本，最新 vX.X.X）」
- 浏览器返回的 JSON 包含 `latest`、`updated_at`、`versions` 三个字段
- `versions` 数组按版本号降序排列
- 当前发布的版本在数组首位，platforms 字段完整
- `latest.json` 仍然存在且内容正确（未被破坏）

---

## 侧边栏 Plugin 子项过滤

### Sidebar_PluginFilter_DisableHidesChildren_001 — 禁用 plugin 后其 skill/agent/mcp 不再展示

**目标**：验证 user-scope plugin 被禁用时，其提供的 skill/agent/mcp 立即从侧边栏对应面板消失

**前置条件**：已安装至少一个 user-scope plugin（如 paper-tool@orczh），且该 plugin 提供 skill / agent / mcp 子项；该 plugin 当前为启用状态

**操作步骤**：
1. 打开 Plugins 面板，记住目标 plugin 提供的 skill / agent / mcp 名称
2. 打开 Skills 面板，确认 Plugin 分组下能看到该 plugin 的 skill
3. 打开 Agents 面板，确认 Plugin 分组下能看到该 plugin 的 agent
4. 打开 MCP 面板，确认 Plugin 分组下能看到该 plugin 的 mcp server
5. 回到 Plugins 面板，点击该 plugin 的 ToggleSwitch 关闭它
6. 不重启、不刷新，依次打开 Skills / Agents / MCP 面板

**预期结果**：
- 步骤 2-4：能看到目标 plugin 的子项
- 步骤 5：plugin ToggleSwitch 立即变为关闭状态（乐观更新），无报错提示
- 步骤 6：三个面板的 Plugin 分组中都不再出现该 plugin 的子项（如果某分组没有其他 plugin 子项，整组隐藏）

### Sidebar_PluginFilter_EnableShowsChildren_001 — 启用 plugin 后其 skill/agent/mcp 重新出现

**目标**：验证 user-scope plugin 从禁用切换为启用后，子项立即回到侧边栏

**前置条件**：目标 plugin 处于禁用状态（步骤同上一用例先关闭它）

**操作步骤**：
1. 在 Plugins 面板点击该 plugin 的 ToggleSwitch 开启它
2. 依次打开 Skills / Agents / MCP 面板

**预期结果**：
- 步骤 1：plugin ToggleSwitch 立即变为开启状态，无报错
- 步骤 2：三个面板的 Plugin 分组重新出现该 plugin 的子项

### Sidebar_PluginFilter_ToggleFailureRollback_001 — plugin toggle 失败时数据与子项展示均回滚

**目标**：验证 toggle API 失败时，plugin.enabled 与子项展示均保持原状

**前置条件**：构造 toggle 失败场景（例如手动停止 claude CLI 后端、或网络异常导致 `claude plugin` 命令失败）

**操作步骤**：
1. 在 Plugins 面板对一个 enabled=true 的 plugin 点击关闭
2. 观察错误反馈

**预期结果**：
- plugin ToggleSwitch 短暂变灰后弹回开启位置（乐观更新被回滚）
- 控制台/Toast 显示错误信息
- Skills / Agents / MCP 面板中该 plugin 的子项仍然展示（未被错误移除）

---

## 应用配色（GUI 主题）

### GuiTheme_PersistAcrossRestart_001 — 重启后应用配色生效

**目标**：验证设置 GUI 主题（浅/暗）并重启进程后，应用整体配色（标题栏、背景、侧边栏等 GUI 元素）正确生效，不再停留在浅色（曾因 `loadAppConfig` 异步加载持久化值后未同步到 DOM 导致重启失效）

**前置条件**：GUI 主题 = 暗色（Settings > 外观切换为「暗色」，已持久化到 `config.theme`）

**操作步骤**：
1. 关闭应用窗口重新启动（dev 下刷新窗口或重启 dev 进程，模拟「重启进程」）
2. 等待环境检查通过、进入主界面，**先不打开设置页**
3. 观察标题栏、应用背景、侧边栏等 GUI 元素配色

**预期结果**：
- 应用整体为深色配色（`data-theme="dark"`），非浅色
- 再打开 Settings > 外观，GUI 主题仍显示「暗色」，与实际配色一致

### GuiTheme_SwitchLiveUpdate_001 — 实时切换 GUI 配色立即生效

**目标**：验证在 Settings > 外观切换 GUI 主题时，配色立即变化（无需重启）

**前置条件**：打开 Settings > 外观

**操作步骤**：
1. 当前为「浅色」，点击「暗色」卡片
2. 观察应用配色变化
3. 再切回「浅色」

**预期结果**：
- 切换后应用配色立即随之变深色/浅色，无延迟、无需重启
- 应用实际配色与设置页选中态始终一致

## 终端主题

### TerminalTheme_SwitchLiveUpdate_001 — 切换终端主题所有 tab 实时变色

**目标**：验证切换终端主题后，所有已开终端 tab 的字符栅格实时更新

**前置条件**：应用已启动，已打开 ≥2 个终端 tab，当前为 CC-Box Dark

**操作步骤**：
1. 打开 Settings（`Ctrl+,`）> 外观
2. 在「终端主题」下拉选择 Dracula

**预期结果**：
- 所有已开 tab 终端立即变为 Dracula 配色（深色背景 + 对应 ANSI 色）
- 无需重启或刷新

### TerminalTheme_IndependentFromGui_001 — 切 GUI 浅/暗不影响终端

**目标**：验证终端主题与 GUI 浅/暗完全独立

**前置条件**：终端主题设为 Dracula（深色终端）

**操作步骤**：
1. 在 Settings > 外观切换 GUI 主题「浅色」↔「暗色」

**预期结果**：
- GUI 配色随浅/暗变化
- 终端字符栅格、容器背景、滚动条、空态**都不变**（保持 Dracula）

### TerminalTheme_SurfaceConsistency_001 — light GUI + dark terminal 表面不拼接

**目标**：验证 GUI 浅色 + 终端深色时，终端容器表面与字符栅格同为深色，无拼接

**前置条件**：GUI 主题 = 浅色，终端主题 = Dracula（或任意深色）

**操作步骤**：
1. 观察终端区域整体（字符区 + 容器边框 + 滚动条 + 空态）

**预期结果**：
- 终端容器背景、滚动条、空态与字符栅格同为深色
- 无"深色画布 + 浅色边框/滚动条"拼接

### TerminalTheme_EmptyStateSurface_001 — 无 PTY 空态表面色正确

**目标**：验证未启动会话时终端空态背景使用终端主题色

**前置条件**：进入项目但未开任何终端 tab（空态）

**操作步骤**：
1. 终端主题设为深色（如 Dracula）
2. 观察空态（"Start new session" 提示区）背景

**预期结果**：
- 空态背景为终端主题色（深色），不出现浅色拼接

### TerminalTheme_PersistAcrossRestart_001 — 重启后终端主题保持

**目标**：验证终端主题持久化

**前置条件**：终端主题设为 Nord

**操作步骤**：
1. 关闭应用并重新启动

**预期结果**：
- 终端主题仍为 Nord

### TerminalTheme_MigrationFromGui_001 — 老用户迁移按 GUI 映射

**目标**：验证删除 config 的 terminalTheme 后，启动按 GUI 主题映射

**前置条件**：手动编辑 `~/.cc-box/config.json` 删除 `terminalTheme` 字段；GUI 主题 = 暗色

**操作步骤**：
1. 删除 config 的 terminalTheme
2. 重启应用

**预期结果**：
- 终端主题 = CC-Box Dark（按 GUI 暗色映射）
- config.json 自动写回 `terminalTheme: "cc-box-dark"`

### TerminalTheme_InvalidIdSelfHeal_001 — 非法 id 自修复

**目标**：验证 config 终端主题为非法 id 时，启动归一化为默认

**前置条件**：手动编辑 config，`terminalTheme = "bogus-theme"`

**操作步骤**：
1. 重启应用

**预期结果**：
- 终端主题 = CC-Box Dark（归一化为默认）
- 下拉显示 CC-Box Dark，预览一致
- config 写回 `terminalTheme: "cc-box-dark"`

### TerminalTheme_PreviewLiveUpdate_001 — 下拉切换预览实时变化

**目标**：验证下拉切换主题时预览块实时变化

**前置条件**：打开 Settings > 外观

**操作步骤**：
1. 在终端主题下拉依次切换不同主题

**预期结果**：
- 右侧预览块背景、文字色、红/绿/黄示例随主题实时变化
- 中文/符号（如 ⠿ ✔ ✖）对比度可读

### TerminalTheme_256ColorFallback_001 — 256 色沿用 xterm 默认

**目标**：验证 256 色输出沿用 xterm 默认调色板（与主题不一致属预期边界）

**前置条件**：终端主题设为某主题

**操作步骤**：
1. 在终端运行 256 色命令（如 `printf '\x1b[38;5;200mtest\x1b[0m'`）与 16 色命令（`printf '\x1b[31mred\x1b[0m'`）

**预期结果**：
- 16 色 ANSI（红）随终端主题变
- 256 色按 xterm 默认调色板显示（不随主题变，属预期）


## 终端渲染后端

### TerminalRenderer_DefaultDom_001 — 默认 DOM 渲染，中文不出现留白/错位

**目标**：验证默认渲染后端（DOM）下，中文输入不会随机插入"空"或将字符替换成空（WebGL glyph atlas 的已知问题）

**前置条件**：`config.json` 无 `webglRenderer` 或为 `false`（默认 DOM）；已开一个终端会话

**操作步骤**：
1. 用中文输入法（如搜狗）在输入框连续输入中文字、混入英文、删除编辑
2. 观察输入框是否有随机出现的"空"（非真空格、删不掉、光标到不了）
3. 发送消息，确认发送内容与显示一致

**预期结果**：
- 输入框无随机留白/字符被替换成空
- 发送的文字与输入显示一致

### TerminalRenderer_SwitchWebgl_001 — 切换 WebGL 仅对新开终端生效并持久化

**目标**：验证在 Settings > 外观切换「终端渲染后端」后，仅新开终端生效，已开终端不变，且持久化到 config

**前置条件**：默认 DOM，已有至少一个开着的终端 A

**操作步骤**：
1. 打开 Settings（`Ctrl+,`）> 外观，将「终端渲染后端」从 DOM 切到 WebGL
2. 观察已开终端 A
3. 新开终端 B
4. 关闭并重启应用，再开 Settings > 外观

**预期结果**：
- 终端 A 保持 DOM（切换不影响已开终端）
- 终端 B 使用 WebGL（高频滚动更流畅）
- 重启后「终端渲染后端」仍为 WebGL（`config.webglRenderer = true`）


## 终端输入（IME）

### TerminalInput_ImeShiftToggle_001 — 搜狗 Shift 切换中英文时已输入拼音进入输入框

**目标**：验证搜狗等中文输入法在中文模式输入拼音后按 Shift 切换英文，已输入的拼音作为字母正常进入终端（修复 xterm.js `_inputEvent` 丢弃 `composed=true` 的 insertText 导致字符不进 PTY 的问题）

**前置条件**：
- 系统安装搜狗输入法（或行为类似的中文输入法），配置为「按 Shift 切换中英文」
- 已启动应用并进入某项目终端，Claude CLI 就绪

**操作步骤**：
1. 切到中文输入法，输入拼音（如 `aaa`），候选框出现
2. 按 Shift 切换到英文模式

**预期结果**：
- 候选框消失，输入法切换到英文模式
- 已输入的 `aaa` 作为字母出现在终端输入行，不丢失

### TerminalInput_ImeShiftToggle_002 — 正常英文输入与 Shift 修饰键不受影响

**目标**：验证修复不影响英文模式下的正常输入与 Shift 修饰键（大写、Shift+Enter 换行），无重复输入

**前置条件**：输入法处于英文模式，终端聚焦

**操作步骤**：
1. 按字母键（如 `a`），输入小写
2. 按住 Shift 再按字母（如 `a`），输入大写
3. 按 Shift+Enter，插入换行

**预期结果**：
- 字母正常输入，无重复、无丢失
- Shift+字母 输入大写
- Shift+Enter 插入换行

### TerminalInput_ImeShiftToggle_003 — 中文选词上屏正常

**目标**：验证中文模式下正常选词上屏不受修复影响（无重复输入）

**前置条件**：输入法处于中文模式

**操作步骤**：
1. 输入拼音（如 `ni`），候选框出现
2. 选词上屏（如选「你」）

**预期结果**：
- 选中的中文正常上屏到终端
- 无重复字符

---

## 全局项目树

**目标**：跨项目一步切换 + 并行状态可见

**前置**：收藏 ≥3 个项目，其中 ≥2 个有运行中会话

**步骤与预期**：
1. 终端视图打开 Sessions 面板 → 树显示所有收藏项目，当前项目置顶展开，有 running tab 的项目自动展开，其余折叠
2. 点折叠项目的**项目名** → 直接切到该项目最近会话，终端切换，cwd 变更，header 项目名更新
3. 点折叠项目的 **▸ 箭头** → 仅展开会话列表，不切换
4. 点**当前项目**的项目名 → 不打断（noop）
5. A 项目工作中（working）时，B 项目节点的徽标显示 `●1`；A 响应完成 pending → 徽标变琥珀
6. 顶部搜索输入项目名 → 过滤到命中项目；输入会话名 → 仅匹配已展开项目的会话，命中项高亮
7. 在未展开项目 hover 点 `+` → 在该项目新建会话并切过去
8. 点 `⋯` → 「关闭所有会话」关闭该项目全部 tab（PTY kill）
9. 未收藏项目有 running tab → 树底出现「未收藏」分组，点「加入收藏」归入正常分组
10. 连续快速点两个不同项目的历史会话 → 各自 --resume 启动正确 sessionId，不串
11. 关掉所有会话 → 空状态文案；展开无历史项目 → 「暂无历史会话」

