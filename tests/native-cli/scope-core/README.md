# D12 scope-core

此入口编译真实生产 capability、历史解析和更名模块，以及这些文件原有的测试；不替换文件系统、权限、锁、SDK 或写入实现。先从仓库根执行 `npm ci` 与 `npm run prepare:session-sdk`，然后执行 `cargo test --locked --manifest-path tests/native-cli/scope-core/Cargo.toml`。Windows 本地可从 `src-tauri` 调用同一 manifest 以复用镜像配置。

`lib.rs` 按路径引用生产模块。为避免把无关 Tauri/恢复协调器引入此小型入口，`build.rs` 使用 Rust AST 从生产源码选择以下原始项，保留属性、签名和函数体；名称缺失或重复会直接使构建失败：

- `cli/snapshot.rs`：`CallerIdentity`。
- `store.rs`：原子写入、原子替换、文件锁及被索引维护原有测试调用的历史枚举链路，共十三个函数/结构。
- `platform/legacy.rs`：`CREATE_NO_WINDOW` 和 `configure_command`。
- `version_history/windows.rs`：四个句柄/错误工具函数；Windows 文件和安全模块仍整文件按路径编译。

官方 SDK bundle 由同一准备脚本生成，构建脚本仅复制这些字节到本 harness 的生成目录，满足生产 `include_bytes!` 的 manifest 相对路径。其测试调用真实 Node 与官方 SDK，只使用临时夹具。生成目录不进入版本控制。

三平台矩阵仍分别执行完整测试及严格 all-target Clippy。本地 Windows 成功不能代替 Linux/macOS CI 或安装包 WebView/真实忙碌 CLI 验收。
