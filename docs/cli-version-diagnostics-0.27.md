# 0.27 本地 CLI 版本诊断

关于页面新增显式采集，只使用已启用 CLI 当前所选 Desk profile。挂载、复制摘要及刷新界面不启动 CLI。每次主动采集先只读刷新 profile；刷新失败会撤销旧版本，显示 `unknown / unavailable`。切换 profile、revision、停用 CLI、离开页面均撤销旧结果并取消准确请求。没有按版本允许或禁止原生功能的逻辑。

## 可追溯依据

| 来源 | 观察到的语义 | 实现边界 |
| --- | --- | --- |
| [Claude CLI reference](https://code.claude.com/docs/en/cli-reference)（2026-11-06 查阅） | `--version` / `-v` 打印版本 | 文档未承诺精确括号格式；不是源码开放或所有版本格式的证明。 |
| Codex `rust-v0.166.0`，commit `6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`：[CLI 入口](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/cli/src/main.rs)、[包名](https://github.com/openai/codex/blob/6ff1e1ca3b30fc8b914720681fcf5fe62e95f856/codex-rs/cli/Cargo.toml) | clap 版本参数及 `codex-cli` 包名 | 固定参数，不解析交互输出或执行登录。 |
| OpenCode commit `286beb081a16d59d269b328329921fef3b15eda9`：[2.2.5 包版本](https://github.com/anomalyco/opencode/blob/286beb081a16d59d269b328329921fef3b15eda9/packages/cli/package.json)、[入口](https://github.com/anomalyco/opencode/blob/286beb081a16d59d269b328329921fef3b15eda9/packages/cli/src/index.ts)、[Runtime](https://github.com/anomalyco/opencode/blob/286beb081a16d59d269b328329921fef3b15eda9/packages/cli/src/framework/runtime.ts)、[官方文档](https://opencode.ai/docs/cli/) | `OPENCODE_VERSION` 交给 Effect Command.run；文档提供版本参数 | 此 commit 与清理审查采用的 v2.2.5 tag commit 不同，不能混称同一源码快照。 |

严格输出向量是本地版本/合成兼容性观察，不是上述文档保证的完整集合：Claude `X.Y.Z (Claude Code)`；Codex `codex-cli X.Y.Z`；OpenCode `opencode vX.Y.Z`、`opencode X.Y.Z` 或裸版本。仅接收单行规范数字版本及有界 `alpha.N / beta.N / rc.N / preview.N`；其他输出为 `unknown / invalid-output`。这只影响诊断能否采集，不影响使用 CLI。

## 探测、取消与来源

`native_probe_cli_version` 使用认证 document bridge，输入仅 `{ requestId, profileId, expectedProfileRevision }`。后端将所选配置冻结为原有 snapshot / recipe / process spec，在任务私有中性 cwd 执行固定 Raw `--version`。不采用 defaultArgs、extraArgs、prompt、observer 或 Desk hooks；不接收前端程序路径、环境、argv 或脚本文本，不扫描认证目录。Native 与已存在的安全 Cmd Shim 路线可用，Shell 及无法证明参数安全的 launcher 明确不支持，不影响正常启动。

运行阶段限时 5 秒，stdout/stderr 合计最多 8 KiB。stderr 非空、额外正文、ANSI、多行、未知格式都只返回固定原因，原始内容不会进入日志、UI 或诊断摘要。准备配置和文件系统解析不声称具有独立的五秒总墙钟上限。

请求绑定 document 身份、profile/revision 和 UUID。取消先到建立 fence，防止迟到开始。每个 profile 最多一个诊断 owner，跨 request 和文档生效；不同 profile 可独立运行。进程通过现有 admission、command owner 和 Windows Job 管理，只有准确 child 真实 reap 才释放。停止两秒仍无法确认时可以先返回 `termination-unknown`，后台保留 owner；新 ID 返回 `probe-in-progress`，不会再次 spawn。前端此时保留原 ID 与 busy，通过取消回执协调；`requested` 不等于已终止，`settled` 才解除。

复制摘要白名单仅有 CLI、临时匿名来源编号、版本、状态及固定原因。配置名称、ID、revision、路径、环境、认证内容、requestId、终端输出和原始错误不复制，不上传。匿名编号只在当前页面实例区分已选来源，不是持久身份。

## 证据限制

Rust 合成测试覆盖输出限制、超时、准确配置与不可用路径不 fallback、恶意默认参数不执行、取消先到/运行中、迟到结果、revision、文档撤销、跨 profile 与独占 owner。未知 reap 使用受控 owner fixture，尚未注入真实操作系统终止/等待失败。Windows 真实 Cmd Shim 使用合成脚本，不是用户 CLI 验收。

完整命令、通过/失败/未运行分类和本地日志见 [本回合验证](../.audit/v027-p0-validation.md)。未验证 Unix 进程组、真实用户 CLI、安装版 WebView 或用户现场身份与自动命名，不以合成测试替代。
