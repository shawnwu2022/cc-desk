# 分组、排序、优化统一审修记录

审查日期：2026-10-17。共同基线为 `351f539`，共 15 个去重的本地提交。
`feat/cli-group-collapse` 的 `b8924d7` 已包含 `perf/runtime-batch1` 的 `3be33b1`；
另将 `fix/session-creation-order` 的 `12a7af1` 以不提交方式整合进当前工作区。
只有 `docs/components.md` 发生文本冲突，已同时保留创建时间排序与分组折叠契约。

| 线 | 提交数 | 审查范围 |
| --- | ---: | --- |
| 优化 | 6 | 十项依赖清理、store blocking pool 调度、Native resize 防抖、真实数据基准快照及文档 |
| 分组 | 7 | 状态归并、CLI 组头、折叠状态、搜索/改名强制展开、键盘导航、Vue 最低版本及视觉断言 |
| 排序 | 2 | CLI 创建时间提取/缓存/投影、首次冻结排序、CLI 与 Desk 双重归档恢复 |

## 确认并修复的问题

| 级别 | 问题及修复 | 回归证据 |
| --- | --- | --- |
| P1 | Rust 返回 `createdAt` 时被严格 IPC 白名单拒绝，导致整个来源历史不可用；补有界解码并保留字段 | 两种 CLI 的真实 decoder 用例先失败，修复后通过；缺失/null 兼容，非法类型、超长、NUL 和未知字段仍拒绝 |
| P2 | `v-show` 隐藏的会话仍被方向键选中；过滤不可见列表中的行并保留常驻实例 | DOM 双向用例先失败后通过；另加真实 Chromium 首组折叠双向导航用例 |
| P2 | 历史来源读取失败时，同一活动行的 `createdAt` 回退为 tab 时间而跳序；所有行首次出现时冻结键 | `Tree_CreationKeySurvivesHistoryGap_032` 先失败后通过，重启排序及既有首次近似用例保持通过 |
| P2 | 旧 rollout 后读到且不替换最新代表时丢失 thread 最早创建时间；无论是否替换代表均合并创建时间 | `HistoryCodex_LaterVisitedRolloutKeepsEarliestCreation_062` 先失败后通过，覆盖逆文件名顺序和跨归档容器 |
| P2 | retained-history 内存预算漏计新字符串堆容量；补 `created_at.capacity()` | `HistorySnapshot_CreationTimeCapacityIsBounded_001` 先失败后通过 |
| P2 | 两个组头及列表间距改变四张严格截图；逐组检查 actual/expected/diff 后同步原始 PNG 字节 | 两位审查者独立接受四组；其余九张原基线字节保留，原失败记录保留 |

四项 Native/排序修复经独立只读收口审查，未发现新增 P1/P2。
归档恢复的外部 CLI 操作仍先于 Desk 清标记；阻塞调度复用既有 helper；
resize 防抖仍检查 active/disposed 并在卸载时清理定时器。未改变输入、授权或进程所有权。

## 实际验证

| 命令 | 结果 |
| --- | --- |
| `npm test -- --maxWorkers=4` | 184 个文件通过；2760 项通过、2 项既有 Windows 跳过 |
| `npm run typecheck` | 通过 |
| `npm run test:visual:unit` | 66 项通过 |
| `npm run build` | 通过；保留既有超过 500 kB 的 chunk 警告 |
| `cargo test --offline --locked --lib cli::native_projection` | 原权限环境完整 167 项通过 |
| `cargo test --offline --locked --lib SpawnBlockingStore` / `IndexDispatch` | 分别 2 项和 4 项通过 |
| `cargo fmt --check` / `cargo clippy --offline --locked --all-targets -- -D warnings` | 通过 |
| `git diff --check 351f539` / `git diff --cached --check` | 通过，无残留冲突 |
| Linux Chromium 完整 no-update 视觉验证 | 237 项通过，覆盖全部 13 张原库存截图、几何矩阵及新增双向导航 |
| Linux `playwright test --config playwright.production.config.ts` | 使用干净 lockfile 依赖重建正常生产入口，中英文首屏与异步设置在真实生产 CSP 下共 2 项通过 |

两个既有 Windows 跳过为 `D20_Runner_ExecutionRejectsSymlinkEscape_11`
与 `D28_D20_RunRootSymlinkEscapeIsRejected_04`，未改变其平台条件。
Rust 专项第一次在沙箱内为 166 通过、1 失败：
`NativeRename_ClaudeValidPlainScopePinsExactTranscript_011` 无法创建 Windows 私有目录。
代码和断言未改，原权限单项及相同环境整域复跑均通过。第一次失败不计作通过。
另一次误从仓库根以 manifest-path 调用 Cargo 未加载 `src-tauri/.cargo/config.toml`，
发生 crates.io TLS 凭证失败；后续均从 `src-tauri` 使用已有离线依赖验证。

像素验收、四张哈希、字体参数和源码补丁哈希见
[视觉验收记录](visual-testing.md#cli-group-integration-baseline-acceptance)。
本地证据保留于 `test-results/release-integration-review/`，包括原 RED 比较、
四组 actual/expected/diff、字体配置及 14 个视觉相关变更文件与临时副本的逐字节一致性记录。

## 证据边界

本轮未执行全部 Rust library、真实 Claude/Codex CLI、安装包 Windows WebView2、
原生缩放/屏幕阅读器或远端 CI 验收。完整浏览器 fixture 不能替代这些要求。
本轮以本地合并提交收口整合；发布仍须由最终精确源码的既有 CI 与发布门禁准入。
