# ProxyProbe_BodyTimeout_003 失败证据补充

本次只补证据，不修改源码、不重跑测试、不提交生成 schema，也不操作原工作树。本记录按父任务要求从本会话保留的工具输出及只读 git 对比整理。没有原生失败日志文件；原输出转录另存为同目录 `updater-proxyprobe-64262-transcript.txt`，该文件明确标注为工具输出转录，不能当作运行时直接写出的 Cargo log。

## 精确失败

工作目录为 `E:\Codex\2026-11-06\task-2\updater-backend`。当时未提交的源码后来提交为 `9812ae0880b28ba0191591dcd6d7c4fb0887888f`。失败没有记录到墙钟时间，因此本文不补造时间戳。

原 `exec_command` 的完整 cmd 参数为两条 PowerShell 命令：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path tests/updater-core/Cargo.toml --locked --offline
```

原 session ID 为 `64262`，失败输出由 `write_stdin` 返回，chunk ID 为 `8f988a`，进程退出码为 `1`。Cargo 构建耗时 `10.72s`；测试结果为 `30 passed; 1 failed`，测试耗时 `10.32s`。失败没有被后续成功覆盖。

直接失败是 `src-tauri/src/tests/desktop_updater_http.rs:287` 的 fixture accept 等待断言。线程在十秒内没有接受到连接，报 `body-timeout fixture never received a connection`。测试主线程在同文件 `:309` 的 `server.join().unwrap()` 再次失败。`bytes(...).await.unwrap_err()` 和 `assert_eq!(error.code, "UPDATER_TIMEOUT")` 没有在输出中失败。

该次输出不能证明预期的响应体超时场景已建立。fixture 在接受连接、读取请求、发送响应头之前就失败了。只有安全错误码相等，不能区分客户端在连接、响应头或响应体阶段超时。

## 当时并发命令

失败时，以下本代理原生 crate 编译/测试仍在运行，原 session ID 为 `59432`：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib UpdateCancellation_ -- --test-threads=1
```

该命令在失败复核之后才完成，完整构建耗时 `3m 10s`。同次工具观察还曾见到一个 rustc 进程约 `3.18 GB` 工作集、累计 CPU `163.03s`。这些只证明存在并发编译，不证明本次失败必由 CPU、内存、调度或编译造成。父代理当时也报告完整 Vitest 正在运行；本代理没有读取其进程或日志，不能独立确认其精确负载。

失败的核心测试命令未传 `--test-threads`。同批新增的 HTTP 包测试和取消竞争测试也处于其测试库存中。因此新增测试工作量、宿主其他工作量及原 fixture 的时间假设都可能相关，不能只凭并发编译就排除本次变更对测试调度的影响。

## 后续复核，按完成顺序

| 命令 | 工具证据 | 结果 |
| --- | --- | --- |
| `cargo test --manifest-path tests/updater-core/Cargo.toml --locked --offline -- --test-threads=1` | session `21959`，chunk `b170b0` | 31 passed，0 failed，测试 `1.47s`，BodyTimeout 通过。串行复核。 |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib UpdateCancellation_ -- --test-threads=1` | session `59432`，chunk `15c0b1` | 5 passed，1753 filtered，测试 `0.02s`。此命令不含 BodyTimeout，不是其复核。它在失败前已启动。 |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --offline --lib desktop_updater -- --test-threads=1` | session `42293`，chunk `fcdc58` | 33 passed，1725 filtered，测试 `0.75s`，BodyTimeout 通过。串行复核。 |

第一条复核的原 cmd 参数还在 cargo test 前打印了该 fixture 的源码，未改变测试命令。后两条命令不代表完整 Rust 库测试通过。独立核心测试仍报告两个原有非 snake case 测试名 warning；这与失败无直接因果证据。

## 相对 RC3 的只读源码核对

基线 `4d4674480849f1d4d8fb4edab095b388e769e189`，本代理提交 `9812ae0880b28ba0191591dcd6d7c4fb0887888f`。用 `git show <baseline>:<file>` 和当前文件提取下列完整函数，统一 LF 后进行逐字符相等比较和 UTF-8 SHA-256。四项均完全一致。

| 函数 | 基线及当前相同的 SHA-256 |
| --- | --- |
| `ProxyProbe_BodyTimeout_003` | `4bc73afdb2fa494ab5268302a61c6493ce80e124003c4f7873d7b5fd3e41f726` |
| `read_proxy_request` | `80c7ace8943cc0d0e2c6d581e13a5b04659209f42a7a1740f0580f023e7f96af` |
| `updater_http::bytes` | `5c380e6e9af821bb82ccc894a71fd6b3e0c8acb57c623771317b1827c833ec73` |
| `updater_policy::failure` | `a700af025c0a3024bc1d897a732085555ab9905e02c6c167eec5d7e382aab1c9` |

基线中测试函数起于 `desktop_updater_http.rs:234`，当前起于 `:277`；前面新增两个包 HTTP 测试导致行号移动。50 ms 客户端超时从基线 `:261` 移到当前 `:304`，值未变。服务端十秒 accept 期限、10 ms 轮询、读请求辅助函数、发送响应头后 150 ms 延迟、错误码断言和 join 均未改动。

该测试直接创建 `reqwest::Client::builder().no_proxy().timeout(Duration::from_millis(50))`，随后调用原 `bytes`。它不调用本次新增的 `package_bytes`、通道选择或更新安装流程。`reqwest` 版本及配置未改；独立核心 crate 的 Tokio 版本仍固定 `1.52.1`，本次仅增加显式 `sync` feature。

相关文件的实际 diff 保留了新增包测试、新增包 HTTP 读取函数和 import；不能说整个文件无改动，只能说上述实际超时路径函数无改动。

## 证据分类与限制

分类为 **原因未定，存在既有 fixture 负载敏感因素**。

已证明该 fixture 和其调用的读取路径未修改，且失败发生于连接前置条件，不能据此认定本次生产 HTTP 改动引入了新的超时回归。50 ms 是整个请求的总超时，包含连接与调度；客户端先超时而 fixture 未接到连接，是该测试设计可发生的失败方式。并发编译及新增并行测试提供了负载变化背景，但没有相同宿主负载下的 RC3 基线对照，也没有网络/调度 trace。

因此不能确认是既有历史 flaky，更不能断言新回归已被排除或已修复。串行复核只证明后续两次相关库存通过。原失败必须保留；如需根因关闭，应另行在隔离 fixture 中对照基线与当前源码、保持相同并发负载并记录连接/响应头/响应体阶段。本次任务未执行该实验，也未放宽超时或断言。
