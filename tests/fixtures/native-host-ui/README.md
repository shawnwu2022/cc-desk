# Isolated native Host UI fixture

该夹具在独立 Tauri Builder 内加载真实 `RemoteHostPanel`、Pinia store、authenticated document bootstrap 和 `cli_start`。原生 supervisor 只运行合成 Node probe；workspace、WebView data directory、TEMP/TMP/TMPDIR 和证据均放在本项目 `.audit` 内。它不运行普通产品启动或 `NativeRuntime::production`，不读写安装版配置。

## 安全边界

- 只注册 `cli_start`、Host configure/discard/request/current/cancel。**不注册 `remote_host_start`，不管理 HostService**；没有网络启动或命名密钥创建/打开步骤。
- 候选中的两个 SPKI 是不同的公开 P-256 测试点。`FrozenHostBinding::validate` 在使用前预检；测试 key name 不能用于真实配置。
- `preparedIdentity` 仅传 tab/request/run ID。运行态必须来自真实收据经 `applyLaunchStatus` 校验，不手写 running。
- 每个 UI 场景必须提供项目 `.audit` 下的新建空目录；`create_new` 占用标记拒绝复用。不要清空、覆盖或复用旧场景证据。
- shutdown 逐项记录 supervisor 事实和原生批准 callback outstanding。整体 `Unknown` 仍是未知，局部 reap/join 完成不是 clean/retired 证明；callback 为 null 也不是完成证明。

## 非 GUI 验证

在仓库根运行，先将 TEMP、TMP、TMPDIR 统一指向 `.audit/runtime-temp`，设 `CARGO_INCREMENTAL=0`。Windows 直接执行 Cargo 前运行 `node scripts/prepare-conpty.mjs`；本轮只使用已验证缓存，不联网下载。

```powershell
node tests/fixtures/native-host-ui/build.mjs
node node_modules/vitest/vitest.mjs run tests/native-cli/hostUiFixture.test.ts
npm run typecheck
cargo +1.98.1 check --offline --locked --manifest-path src-tauri/Cargo.toml --features remote-host --all-targets --target-dir target-browser-profile
cargo +1.98.1 clippy --offline --locked --manifest-path src-tauri/Cargo.toml --features remote-host --all-targets --target-dir target-browser-profile -- -D warnings
cargo +1.98.1 test --offline --locked --manifest-path src-tauri/Cargo.toml --features remote-host --lib --no-run --target-dir target-browser-profile --message-format=json
```

从同次 Cargo JSON 记录取出 lib test executable，以过滤名 `tests::native_cli_run_supervisor::host_ui::` 执行。**不要加 `--ignored`**：默认只运行三个非 GUI 测试，手动 worker 保持 ignored。前端 DOM 测试覆盖真实 store/组件的收据准入，但其 IPC 为合成输入，不能代替真实 WebView。

## 后续人工验收

只有原生窗口操作获准后，才在新的空场景目录设置 `CC_DESK_HOST_UI_ROOT`，显式执行同次 executable 的 `tests::native_cli_run_supervisor::host_ui::HostUi_Manual_Worker_099 --exact --ignored --nocapture`。保持同样的项目内临时目录和独立 data directory，不操作或关闭用户安装版应用。夹具有 30 分钟 watchdog；不能把 watchdog、进程消失或 libtest 输出视为 UI 通过。

需要记录真实面板 configure/discard、原生批准拒绝/接受、current/cancel 和关闭后的状态及 callback 事实。不得点击或注册真实网络 start。任何未操作或未观察到的场景继续标为未验证。

## 本轮证据边界（2026-11-13 UTC）

原生窗口操作授权超时，未获批准；本轮没有再次请求授权或进行 UI 操作。首次 `preparedIdentity` 数字字段错误由 DOM 回归复现并修复。旧 `.audit/host-ui-isolation-20261114/scenario-no` 原样保留，其中 `ownedRunPhysicallyRetired: true` 是旧版过度命名，不能证明整体资源退出、callback 完成或 UI 验收成功。进程退出原因未作推断。

修复验证日志及源码/产物哈希见 `.audit/host-ui-isolation-20261114/repair/manifest.json`。实际 WebView、原生批准窗口和新 shutdown 现场行为均仍未验证。
