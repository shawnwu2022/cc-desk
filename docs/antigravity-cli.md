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


### 真实 CC Desk 核心 PTY 验收（后续检查）

重新检查时既有程序已变为 `1.5.9`；没有执行覆盖安装，版本变化原因未直接观察。Google LLC 签名为 Valid，SHA-512 与上述官方 Windows x64 清单一致：

`539a9415a686d9ec45f5c59fe7d32c485c6fa9abaea50208ae8ede4ed3622b9fb2bfd6680c87dc52344981172a96ddacb261ff4231276b5572fb5b62a9b0221e`

可复现命令（须已安装官方签名 CLI，并先完成仓库 ConPTY/SDK 准备）：

```powershell
./scripts/native-cli/run-antigravity-smoke.ps1
```

验收仅由显式 `antigravity-acceptance` feature 编译，默认产品和默认测试不会自动运行真实 CLI。脚本先核验签名、记录版本及 SHA-256；`--version` 探测在真实宿主环境执行，不属于隔离 HOME/APPDATA 的测试阶段。测试走生产 `discover_programs`、配置 CAS、冻结 snapshot、LaunchCoordinator 与 OwnedPty，使用空临时项目和隔离的 HOME/APPDATA，不写入用户启动配置。

实际结果：真实 PATH 发现并选择精确的 agy 文件；收到 TUI 欢迎输出；PTY resize 调用成功；Desk 持有的进程句柄停止成功，确认退出码 1，读线程关闭。

保留失败证据：第一次尝试要求两次 Ctrl+C 优雅退出，未认证启动阶段 15 秒内未退出，故该测试失败并清理。后续通过的是 **Desk owned stop**，不是把键盘取消标为成功。resize 仅证明系统调用成功，不证明画面布局正确。环境隔离并非 OS 沙箱或网络隔离，不宣称 CLI 完全不访问网络或系统凭据服务。

没有确认登录、首次偏好、工作区信任，也没有发送任务。未完成已认证真实会话恢复；没有 WebView 可视控制工具，因此不宣称完成应用窗口点击、画面布局或文档桥全链路验收。`target-agy/` 仅为未跟踪构建缓存，不纳入提交，不删除用户文件。


### 2026-11-17 认证隔离边界补核

官方 [Local silent keyring sign-in](https://antigravity.google/docs/install/#local-silent-keyring-sign-in) 明确说明：本机 agy 启动可访问操作系统安全钥匙串（Windows Credential Manager）；有有效令牌可静默认证，没有保存会话可打开默认浏览器。因此，历史 smoke 的临时 HOME/USERPROFILE/APPDATA/LOCALAPPDATA 只隔离部分文件路径，不能证明隔离同一 OS 用户的凭据。

这是依据官方行为发现的测试隔离风险，不是已证明历史测试读取、使用或泄漏了凭据。本轮没有读取凭据库、真实会话或运行 agy，也未使用已有登录。后续真实认证/恢复验收须在经授权的独立 OS 用户或 VM 与专用测试身份中进行，先核其钥匙串、浏览器与数据边界；不能把临时目录、--sandbox 或未人工点击登录当作无认证证明。历史 banner/PTY 验收仍仅保留原层级，不升级为认证恢复成功。
