# OpenCode native archive helper 构建

`npm ci` 安装根项目工具后，`npm run dev` / `npm run build` 在 `prepare:session-sdk` 后自动运行 `prepare:opencode-archive`。直接调用 Cargo 前也须先运行 `npm run prepare:opencode-archive`，使 Rust 的 `include_bytes!` 输入存在。

主 crate 的 CI 编译、Windows 测试及独立分片执行 job 固定 Node 24.18.0，以便真实运行归档 helper 测试；纯前端及不运行 helper 的独立 core 路线保持原工具链。这是 helper 的 Node 运行时要求，不是用户 CLI 版本白名单。

普通 CI、D11/D12、ConPTY、历史诊断及粘贴验收的直接 Cargo 编译 job 会在当前 checkout 先准备两个 helper，并在准备失败时停止。分片只运行已编译产物，纯 observer/transport/input core 不需要这些资源。本地 `npm run test:rust:lib` 与 baseline 入口同样先准备；此依赖顺序由 `tests/scripts/nativeHelperPreparation.node.mjs` 静态策略检查守护，并纳入普通 CI。

D12 scope-core 直接接入真实 Claude recovery 模块，并由既有 AST 机制原样选取 OpenCode `PinnedSource` 结构体、依赖函数和唯一完整 impl。它验证 registry 的来源能力，不包含 helper 执行或进程 owner，不用替身替换校验。主 crate 继续负责完整执行与 helper 资源验收；D12 不因此声称产品全链通过。

## 可复现依赖与缓存

- `scripts/opencode-archive-runtime/package.json` 和独立 `package-lock.json` 固定完整依赖图。OpenCode 2.2.6 / Effect 4.1.0 是 helper 构建供应链版本，不是用户 CLI 版本白名单。
- 默认将这两个文件复制到 `.cache/opencode-archive-sdk/<hash>/`，执行 `npm ci --ignore-scripts --no-audit --no-fund --include=dev`。hash 覆盖两个文件、操作系统、架构和 Node 版本。npm 下载缓存也限定在 `.cache/opencode-archive-sdk/npm/`。
- 不执行依赖安装脚本，不改全局 npm、用户 CLI 或配置，不运行模型/host。安装会遵守已有 npm registry / 代理设置；网络、完整性或 registry 错误以非零退出，不回退到其他版本或已有全局 SDK。
- 完整安装后缓存可重复使用；每次仍重新 bundle 当前 helper 源码、校验保留代码的包版本并生成许可证。锁文件或平台变化会建立独立缓存，旧缓存不会自动删除。
- 同一安装目录有排他锁。若进程被硬终止，确认没有其他准备进程后可自行移除其 `install.lock` 空目录并重试，不应复制完成标记来绕过失败。
- 构建已在 Node 24.18.0 / npm 11.14.1 验证。间接依赖 `@opentui/core` 声明 Node >=26.4.0 会产生 npm engine warning；该 TUI 不进入归档 helper，未放宽使用者的 engine-strict 配置。运行 helper 所需 Node/SQLite 权限条件由原生调用方另行检查。

## 产物和许可证

默认只生成 `src-tauri/resources/opencode-native-archive/bridge.mjs`、`LICENSES.txt` 和 `NOTICE.txt`。Rust 嵌入 bridge；Tauri 资源映射应将两个文本文件放入 `licenses/opencode-native-archive/`。不将构建缓存、node_modules、CLI 二进制或 native addon 加入资源配置。

许可证依据 esbuild metafile 中实际保留代码的包收集，包含包内 LICENSE / COPYING / NOTICE / COPYRIGHT 文件。npm 包缺少独立许可证时，精确版本的补充文本保存在 `scripts/opencode-archive-runtime/licenses/`；未知缺失导致构建失败，不能只靠 SPDX 标识放行。完整上游源码注释由 esbuild `legalComments: 'eof'` 保留。

- OpenCode 的 npm 包声明 MIT 但不含 LICENSE，补充文本来自 [v2.2.6/LICENSE](https://github.com/anomalyco/opencode/blob/v2.2.6/LICENSE)，上游 Git blob 为 `6439474beed8e0271df9862eff97ffd70ec2464c`。
- AWS、Sigstore、Drizzle 的缺失包使用各自仓库 LICENSE，源 URL 与 Git blob 记录在 `licenses/sources.json`。
- `data-uri-to-buffer` 的完整 MIT 文本来自其发布包 README。SPDX 数据包同时保留其发布 README 的归属说明和完整 CC-BY-3.0 / CC0-1.0 条款。
- `@npmcli/agent` 发布包及官方仓库未附独立许可证文本；其 package.json 声明 ISC、author 为 GitHub Inc.，产物保留作者元数据，并附 SPDX 官方 ISC 条款原文，不自行杜撰版权年份。SPDX 许可证文本来源也记录在 `sources.json`。
- 本构建将依赖 bundle/minify 并替换 node:sqlite 接线，未修改 vendored 上游许可证。修改说明随 NOTICE 分发。

## 隔离实验与验证

旧的显式模块目录模式继续可用，不执行安装，默认输出到 `.audit/opencode-archive-bundle/bridge.mjs`。第二个参数可指定输出文件，旁边同时生成两个许可证文件。

```powershell
node scripts/prepare-opencode-archive-sdk.mjs <isolated-node_modules> [output.mjs]
node --check src-tauri/resources/opencode-native-archive/bridge.mjs
node tests/scripts/opencodeArchiveBridge.real.mjs <bridge.mjs> <isolated-node_modules>
```

该 real 脚本仅使用官方 SDK 新建的隔离 fixture 数据库。它不触碰用户数据库，也不替代 Windows 原生进程、权限与安装包验收。2026-11-12 在上述工具链、6d3f6cb5 helper 源上生成 bridge 4,657,940 字节，许可证覆盖 188 包；真实 fixture 的 10 项检查通过。后续源码改动必须重新生成、重新验收。
