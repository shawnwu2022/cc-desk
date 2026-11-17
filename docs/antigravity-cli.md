# Antigravity CLI 接入（实验性，默认关闭）

## 使用方式

1. 在设置的 CLI 支持开关中显式启用 **Antigravity CLI**。新安装、旧偏好缺少字段及非法字段均默认关闭；不会安装 CLI、登录、配置凭据或自动启动进程。
2. 创建 Antigravity 启动配置，发现并显式选择已安装的 `agy` 可执行文件。已选路径失效时不会偷偷改用 PATH 中的其它程序。
3. 可新建交互式 TUI，或通过现有配置和注册项目按会话 ID 继续。停用开关隐藏新入口，不中断正在运行的终端；终端仍可关闭。

持久化和 IPC 类型始终使用 `antigravity`；`agy` 只用于官方命令名和紧凑文本标记，不作为另一种 provider 身份。

## 已核实的能力边界

官方参考：https://antigravity.google/docs/cli/reference/
官方教程：https://antigravity.google/docs/cli/tutorial

- `agy` 无动作参数启动原生交互界面。
- `--conversation <id>` 继续指定会话；ID 仅是用户提供的启动定位符，不代表 Desk 已验证该会话身份。
- `--continue` 继续最近会话，不是 picker。TUI 内 `/resume` 不能伪装成启动参数，所以宿主 picker 暂不提供。
- 用户配置的 defaultArgs/extraArgs 原顺序保留；raw argv 完整透传。不额外注入模型、权限绕过或自动同意参数。
- 历史、资源投影、归档、改名、身份观察器、版本探测尚未接入。未知根保持未知，不读取 Claude、Codex 或 OpenCode 根作为替代，也不推测 Gemini 目录格式。
- 继续使用已有文档认证、冻结配置版本、原生 PTY owner、取消/停止机制，不调用 legacy Claude PTY。

## 验证与限制

接入测试覆盖默认关闭/显式保存、原生启动入口、按 ID 继续、独立偏好槽位、精确 argv、观察器关闭和未知投影拒绝。
### 2026-11-17 实机检查（Windows x64）

用户授权按需安装后重新检查，发现已有 `%LOCALAPPDATA%/agy/bin/agy.exe`，因此没有重复安装或覆盖。

- `agy --version` 实测为 `1.5.2`；Authenticode 状态为 `Valid`，签名人为 Google LLC。
- 真实 `--help` 列出 `--conversation`、`--continue`、print 和 ACP 等能力，确认是独立 CLI，而非编辑器路径启动器。
- 空临时目录内启动真实 TUI，观察到欢迎和首次颜色配置界面；没有输入任务、确认首次设置、配置凭据或人工完成登录。两次 Ctrl+C 正常退出，退出码为 0。
- 官方安装脚本 `https://antigravity.google/cli/install.ps1` 当日指向的 Windows x64 最新清单为 `1.5.9`；现有 `1.5.2` 不等于该最新清单字节。保留现有有效签名版本，不把签名检查误称为最新版本哈希匹配。

应用契约/夹具验证与上述 CLI 冒烟是不同证据，尚未完成 Desk 宿主内、已认证会话的端到端验收。发布前仍需实测已认证 TUI 输入输出、真实按 ID 恢复、窗口调整及关闭/取消；本次不发布 RC。


### 本地验证记录

- `npm run typecheck`、`npm run build` 通过（构建保留既有大 chunk 警告）。
- `npm run test:fast`：197 个文件，3179 项通过。
- 原生参数专项：35 项通过；严格 `cargo clippy --locked --lib --tests -- -D warnings` 通过。
- `cargo test --locked --lib`：1791 通过、18 失败、41 ignored，不能称为全库绿色。
- 在同执行环境，以起点 `038f4acab7e2dce1f3ce37cdb4c7d046743870f9` 建立 detached 对照树，仅运行 `version_history::windows::manager_process::tests`：2 通过、18 失败、1 ignored。18 个失败名称与当前全库失败集合完全一致，均报告 `source host is job-contained`。这是已复现的基线/环境限制，不是仅凭名字推断；没有改变安全限制或忽略测试。
- 以上不替代 Job-free Windows 门禁、普通 CI、签名构建及已认证真实 CLI 端到端验收，不据此发布。
