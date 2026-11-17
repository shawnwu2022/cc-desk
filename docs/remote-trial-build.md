# 本地 remote-trial 构建档

这是 **0.28 开发中的本地远程试用构建**，不是正式 0.28 发布或安装包验收。版本与应用 identity 继续继承当前 main 的 `0.27.0`；不修改 productName、identifier、版本或升级配置。不能将此目录中的可执行文件当作正式 0.27.0 产物发布。

## 显式、默认关闭

标准 `npm run build`、默认 Cargo features 与正式 release workflow 不变。仅下列专用命令启用编译 feature `remote-host`，运行时仍需原真实登记、精确目标和两次本机批准；AGY 仍默认关闭。试用授权最长 60 秒，不是长期无人值守或跨 App 恢复。Host 未配对等待最多 3 次与浏览器安全闲置替代最多 3 次是独立预算，不是连接保证；未结算输入不会自动重放。

## 前置条件与离线门

仅支持 Windows x64。先准备已锁定的项目依赖、ConPTY 缓存、Rust wasm32 target、Cargo/npm 缓存及原 SDK 依赖。此构建档不安装工具、不下载 ConPTY、不配置凭据；缺失缓存即失败。ConPTY 只走原 manifest 的 `--verify`。

必须由调用方显式提供绝对、现存的工具路径：

- `CC_DESK_TRIAL_CARGO`：选定的 `cargo.exe`
- `CC_DESK_TRIAL_RUSTC`：同目录的 `rustc.exe`
- `CC_DESK_TRIAL_CLANG`：已核实的 `clang.exe`
- `CC_DESK_TRIAL_AR`：已核实的 `llvm-ar.exe`
- `CC_DESK_TRIAL_BINDGEN`：与锁定 wasm-bindgen 版本匹配的 `wasm-bindgen.exe`

可显式选择已有 `CARGO_HOME`。原生 Cargo 由选定 rustc 编译的 std-only runner 启动，编译时冻结本项目根目录与选定 cargo。它只接受本档有限 build 参数，强制根目录 cwd、原 `src-tauri/Cargo.toml` manifest、`--offline --locked`；因此显式采用根目录已验证缓存，不加载 `src-tauri/.cargo` 的镜像覆盖，也不修改该配置。runner 拒绝额外 manifest/config/feature 替换，不通过 shell 拼接参数。脚本设置 Cargo/npm offline；WASM 仅使用所选工具，不自动安装或替换。工具来源审核是前置条件，文件名和存在性检查不等于供应链认证。

```powershell
npm run build:win:remote-trial -- --debug --no-bundle
```

只接受 `--debug` 和 `--no-bundle`；后者始终强制。拒绝额外 config、features、bundles 或版本检查绕过参数。配置关闭 updater artifacts，命令固定 `--no-sign`。这不是禁用运行时 updater 插件或更改其既有端点；本片不启动应用。本片不验证 NSIS 离线工具供应，**不生成安装器、不安装、不签名、不发布**。

## 原构建链与输出

专用 beforeBuildCommand 依次验证缓存 ConPTY、构建原 session/OpenCode SDK helper、构建原生产 Client 与 Registration WASM、核验来源，然后 Vite 构建专用前端目录。SDK 依赖不足时原 npm 子进程也受 offline 限制。原 WASM 构建仍使用 `--offline --locked`。

两份 manifest 必须覆盖当前完整源码清单、专用 browser target 内各角色唯一的实际 rlib 输入、产物大小与 SHA-256；实际 WASM 导出必须含原生产入口且无 TEST_ONLY。前端复制后的 WASM/glue/manifest 再逐字节验证。任何不匹配终止构建，不能用历史成功日志代替。

- 前端：`.audit/remote-trial-dist/`，附 `remote-trial-build.json`，类型为 `LocalRemoteTrialNotRelease`。
- 原生：`.audit/remote-trial-target/native/debug/cc-desk.exe`（不加 `--debug` 时在 `release/`）。
- WASM 缓存：`.audit/remote-trial-target/browser/`。

以上均为本地生成目录，不提交二进制。原应用内部名称仍为 CC Desk，输出目录的 remote-trial 标记不可剥离后冒充正式发行。

## 验证边界

构建通过不证明用户手机登记、浏览器可信 HTTPS/WSS、真实 native 对话框或 WebView2 交互完成。现有合成 HTTPS/虚拟认证器/Node WSS 代理证据与真实 TLS 信任分开。真实登记与部署仍须独立授权及验证；本档不提供用户配置或密钥。

## 实测中的前置失败

首次构建暴露 Windows rustup 的 cargo/rustc 别名不能替换为 realpath 后的 rustup.exe，现保留显式入口名。之后 Tauri 原默认 cwd 会读到子目录 aliyun 配置，现存缓存缺 arc-swap/coset；离线 vendor 又因缺 crunchy 0.2.4 拒绝。上述失败均保留，未联网补包、未伪装镜像缓存。专用 root runner 是经明确选择的独立构建来源，不代表原镜像已修复。

这里的可复现是指固定入口、明确工具/来源、离线失败门及可重复执行的验证步骤，不是跨机器或不同构建目录的位级确定性证明。Registration manifest 中保留本次构建证据目录，因此不能承诺重跑二进制哈希恒定。

## 本轮验证记录

- 入口缺失 RED：`.audit/remote-trial-entry-red.log`。
- 最终策略门 8 项：`.audit/remote-trial-policy-final3.log`；std runner 2 项经 `rustc -D warnings`：`.audit/remote-trial-runner-tests-final.log`，并通过 rustfmt 检查。
- 最终真实命令：`.audit/remote-trial-build-final2.log`，根目录 runner + offline/locked，原生 dev 构建 27.76 秒，实际生成独立目录的未签名可执行文件；没有运行应用。
- 最终专用 dist 的 operator 6 项与真实 EntryWorker 4 项：`.audit/remote-trial-operator-final2.log`、`.audit/remote-trial-entry-final2.log`。前者包含 manifest 门、4 个拒绝分支和 reader 正选，不是 6 个完整认证成功；后者使用仅响应 close 的隔离 MessagePort，对端不是实际 socket/TLS。
- 版本保持 0.27.0 的 5 项：`.audit/remote-trial-version-final.log`。共 25 个独立定向检查，不把重复构建或此前整合阶段 383/45 测试重复计入本片。
- 完整应用/安装包/真实桌面 WebView2/真实手机与公网均未验收，不能凭此宣称正式 0.28 可发布。
