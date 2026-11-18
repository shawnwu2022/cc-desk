# Windows 标准包的远程能力

本轮只修标准构建接线，不发布新版本、不创建用户登记或凭据、不自动开启远程。不能把构建成功等同完整远程产品验收。

## 构建与默认状态

- Windows 自动平台配置在标准 Tauri build 中传入 remote-host；Cargo default 仍为 sqlite，通用/macOS/Linux 配置不增加该 feature。原 Windows ConPTY 准备、签名和 protected-main promotion 门保留。
- 标准 beforeBuild 运行 build-desktop-product.mjs：准备原 CLI SDK，离线锁定编译生产终端和登记 WASM，复核完整 source/input/file/export 库存，再构建 Vite，逐字节复核 dist 中两 runtime。缺工具、缺缓存、错误版本、错误摘要均阻止构建。
- Windows release、两个 roundtrip、ConPTY package 和 paste diagnostic 的标准 Tauri 构建统一通过本地 composite action 显式准备 Rust 1.98.1 的 wasm32 target，以及固定官方 LLVM 21.1.8、wasm-bindgen 0.2.129 归档；下载后先核 SHA256，再在 runner 临时目录提取。只向构建子进程传工具路径，不修改系统 PATH/注册表；依赖预取使用原锁文件的完整解析图，不用 --target 截断；实际 runtime 构建仍限定 wasm32 并保持 offline。
- 编入能力不创建授权。既有 Host 服务初始 Empty；用户主动选择精确 run/设备，完成原登记及本机明确批准才进入既有上限 60 秒的试用。原取消、截止、CAS、未知输入不重放和 Unknown 隔离规则未改。
- Host UI 通过原受鉴权 IPC 调用原生网络 owner，不给 Tauri CSP 增加外部 WSS 白名单。外部手机浏览器是另一角色，不能因为 dist 中存在 WASM 就访问本机应用或获得授权。

## 仍然阻挡真实交付的门

1. 手机需要可达的受信任 HTTPS 站点、正确 COOP/COEP 和 WASM MIME、匹配的 rendezvous Relay。当前构建不部署站点或 Relay，也不绕过证书。
2. 生产 browser-profile 使用现有 product reader，从同源已有安装记录取得原批准 provider；缺登记时拒绝，不创建裸凭据配置。真实手机 WebAuthn、登记、Host 双批准及断线/撤销必须在真实设备验证。
3. 本地 no-bundle 构建不证明 NSIS 安装、普通 WebView2、macOS/Linux Host 支持或平台签名。非 Windows 既有 Host 命令保持 Unsupported，不宣称本轮增加跨平台 Host。
4. 60 秒试用不是长期无人值守。长期授权、安全日志维护、跨 App 恢复的未完成门不因标准打包而消失。
5. AGY 参数/默认关闭/恢复请求契约可用隔离 fixture 验证，真实认证和恢复仍需获授权的专用 OS 身份或 VM 及专用登录。临时 HOME 不隔离系统 keyring；不借用现有登录或用户数据。详见 [AGY 验证边界](antigravity-cli.md)。

本轮实际验证：45 项构建策略/准入/原 roundtrip 测试、46 项 Host/短会话组件测试；标准 Tauri no-bundle 构建完成（native 2m14s），无 trial 配置和 CLI feature 参数，原审计 runner 仅固定离线缓存 cwd/manifest，要求 Tauri 自身传入 remote-host。实际二进制 fingerprint 含 remote-host，标准 dist 两份新生产 WASM 已核 source/input/file/export/逐字节复制。EXE SHA256 为 8cefe1e2e54544bc0d9d9e5917645f78b83de2279f74bcce69961e9b5c62c134，NotSigned。它来自本轮未提交源码，不是此前独立试用 EXE，也不是安装包或发布产物。

供应脚本另在隔离临时 runner 目录使用已有官方归档实际校验/提取成功，工具报告 LLVM 21.1.8 / wasm-bindgen 0.2.129；此例未重新联网下载，clean hosted runner 下载仍待 CI。保留首次缺 wasm32 target、原 aliyun 离线缓存缺 anyhow、相对 runner 路径错误三项环境/命令失败，不作为产品行为 RED；配置缺少 remote-host 的断言才是本轮接线 RED。

审计文件：standard-remote-policy-all-jobs-green2.log、standard-remote-package-defaults.log、standard-remote-tauri-root-cache-absolute.log、standard-tool-supply.log、standard-remote-artifact-ledger.json（均为本地 .audit 证据，不提交二进制）。TypeScript 检查 exit 0。本机标准 no-bundle 成功不替代 clean hosted runner 的供应/构建 CI。本轮未运行普通 App、安装器、真实手机或 AGY 登录。

## Clean hosted 依赖预取回归

PR #99 的首次 clean Windows 构建在预取 wasm32 成功后，原 client 离线构建仍缺 cpufeatures 0.2.17。新隔离 CARGO_HOME 用同 Rust 1.98.1、原锁文件和命令复现同错，发生在编译前；这不是允许离线构建联网的理由。供应步骤改为完整锁图 fetch，编译目标、offline/locked 和生产源码不变。原 target-filter RED 保留；完整 fetch 后同一新隔离 cache、空 target 的原 client/registration 两 runtime 离线构建均成功，进程退出 0，65/55 源库存与 input/file/export 验证通过，46 项策略回归通过。完整图显示 client 的非 wasm transport → tungstenite → sha1 → cpufeatures，wasm tree 中不存在该包；证明缺的是离线解析所需的完整缓存，不是要把它编入 wasm。日志 browser-fetch-target-red-*、browser-fetch-full-*-green.log、browser-fetch-runtime-verification.log 和 browser-fetch-contract-green.log 保留于 .audit；新 hosted CI 仍须重验，不用此前有缓存的本机绿覆盖其首次失败。

## Hosted ordinary WebView smoke (pending first hosted execution)

The installed-package job now runs `test-desktop-webview.ps1` once after its two
successful ordinary installer/ConPTY probes and before destructive relocation
probes. This launches the **ordinary installed executable**, not `--check-conpty`,
a standalone Wry fixture, a fake IPC page, or a registered test run. Its native
UI Automation observation is restricted to the exact launched PID's top-level
window and that window's descendants (WebView descendants may have child PIDs).
It captures window screenshots and bounded UI trees for General and Terminal,
requires the AGY enable control to report disabled/Toggle Off, and requires the
remote terminal Locked status and disabled Disconnect control. It does not click
AGY enable, Connect, Refresh, registration, approval, or any CLI launch action.

The script refuses a non-hosted/noninteractive/session-zero environment, an
already running CC Desk, or an existing `.cc-box` directory. It never reads or
resets an existing user's application data. The installation must be under the
disposable runner's temporary directory. There is no HOME/cwd isolation claim:
the whole hosted OS/user is disposable. An inaccessible desktop/UIA selector or
unexposed toggle state is a blocked/failed gate, not a simulated fallback.
Normal completion requires WindowPattern.Close, original process exit zero, and
exit of the observed owned WebView children. Killing only retained original
process handles is failure cleanup, never evidence of normal retirement.
The workflow preserves observation artifacts even when the step fails.

This proves only ordinary WebView startup and visible defaults/settings entry.
The Host panel requires an actual active native CLI run and is **not** covered by
this empty-workspace smoke. A visible Locked state is not a packet trace, a
backend-empty proof, or AGY authentication/recovery acceptance. The original
background updater may check after 30 seconds; neither the updater nor network
security settings are disabled here. Real phone, trusted HTTPS, registration,
Host approval, real CLI and long unattended-use gates remain outstanding.
Local validation is script contracts/PowerShell parsing and an explicit
non-hosted refusal, without launching the application; actual UI acceptance
requires the new hosted job's artifacts.

The startup guard also refuses any `WEBVIEW2_*` environment override and an
existing `%LOCALAPPDATA%/io.github.shawnwu2022.ccdesk` directory. This is the
actual default selected by locked Tauri 2.11.1 `manager/webview.rs` (LocalData +
identifier), not an assumed cwd profile. Child handles are retained only after
CIM creation time/image/session match that exact handle and its verified parent
lifetime. Unverified children are neither registered nor killed; even failure
cleanup never uses a PID-tree kill. Observed-child exit is not a claim of exhaustive
OS process auditing. The disposable hosted OS remains the isolation boundary.
