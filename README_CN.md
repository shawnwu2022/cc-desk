# CC Desk

CC Desk 是面向 **Claude Code 与 Codex CLI** 的桌面工作台，技术栈为 Tauri 2、Vue 3、xterm.js 与 Rust。

它不替代 CLI 的单会话交互，而是让真实 CLI 继续负责命令、权限、认证、MCP、扩展与未来版本行为；CC Desk 负责多项目、多标签页、稳定运行身份、恢复、终端传输和只读资源投影。

> Native CLI v3 的代码侧实现已推进到 D27。D20 真实 Claude Code / Codex CLI Layer-C 认证仍需要一个明确授权的目标环境。CI 的宿主测试不能被表述为真实 CLI 认证。

## 核心能力

- 在受控 PTY 中直接运行真实 **Claude Code** 或 **Codex CLI**。
- 一个窗口同时管理多个项目和多个会话。
- 支持新建、CLI 原生恢复选择器、指定原生 session ID 恢复、精确 raw argv 启动。
- raw argv 使用 JSON `string[]`，不会按 shell 字符串重新拆分。
- 键盘、IME、粘贴、终端协议回复与图片粘贴意图共享有序输入通道。
- 输出使用有界背压和精确 ACK，绑定 owner/run/generation/stream。
- 原生 history / MCP / skills / agents / plugins 等资源通过后端鉴权范围只读投影。
- Claude observer 仅是可选元数据通道，不拥有进程生命周期。

## 明确不做

CC Desk **不负责** Provider/API Key 切换，也不把自己变成原生 CLI 配置写入器。

- Provider/配置切换由 Claude Code、Codex CLI 或 cc-switch 等外部工具负责。
- Native CLI 工作区中的资源面板为只读投影。
- observer 失败不会杀死或重启 CLI。
- 启动或输入结果不确定时，不会自动重放或偷偷再启动一次。

## Native CLI 工作区

标题栏和欢迎页都可进入 **Native CLI** 工作区。

| 能力 | Claude Code | Codex CLI |
|---|---:|---:|
| 新会话 | 支持 | 支持 |
| 原生恢复选择器 | 支持 | 支持 |
| 指定 session ID 恢复 | 支持 | 支持 |
| 精确 raw argv | 支持 | 支持 |
| 注册项目选择 | 支持 | 支持 |
| 独立 Profile | 支持 | 支持 |
| 原生资源只读投影 | 支持 | 支持 |
| 有序分段输入 | 支持 | 支持 |
| 有界输出 + ACK | 支持 | 支持 |
| Claude observer overlay | 可选 | 不注入 |

仓库仍保留旧 Claude 工作区用于兼容。新的双 CLI 功能必须走鉴权 Native CLI 路径，禁止回退到旧 `ptySpawn` / `ptyInput` / `ptyKill`。

完整架构见 [docs/native-cli-v3.md](docs/native-cli-v3.md)。

## 关键安全边界

- workspace/profile 写入使用 revision/CAS 与原子替换；
- `unset`、false、空值不会被旧版本状态重新“复活”；
- Codex 不继承旧 Claude secret/env；
- Native frontend 命令必须经过鉴权 document bridge；
- Native UI 禁止 `v-html` / `innerHTML` 可执行 HTML sink；
- 对外错误只能是固定安全码，不回显任意原生异常文本；
- partial/unknown 输入写入不自动重放；
- 错 owner、旧 generation、旧 stream 的 ACK 会被拒绝。

## 验证状态

D01–D19、D21–D27 已有对应代码侧/宿主侧验证。

D20 harness 与目标机命令已经实现，但真实 PASS 必须同时具备：

- 明确授权的测试账号；
- 真实 Claude Code binary；
- 真实 Codex CLI binary；
- 隔离的目标测试根；
- 四格矩阵的真实事件/host payload 证据。

合成 fixture、模型复述、屏幕文字或 writer success 都不能替代这层证据。

## 发布状态

当前仓库只构建 **signed candidate packages**。

发布被明确关闭：

- `scripts/release-policy.mjs` 永远返回 false；
- `.github/workflows/release.yml` 只构建并上传候选产物；
- 不创建 GitHub Release，也不发布 updater manifest。

代码 CI 全绿不等于获得发布授权。

## 从源码构建

```bash
npm ci
npm run typecheck
npm run test:ci
npm run build

cd src-tauri
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

启动桌面开发环境：

```bash
npm run tauri:dev
```

## 项目来源

CC Desk 最初基于 `orczh-hj/cc-box` fork，现独立维护。MIT 来源与版权信息保留在 [LICENSE](LICENSE) 和 [NOTICE.md](NOTICE.md)。

`~/.cc-box/`、部分 `CC_BOX_*` 名称仍作为兼容标识保留，避免破坏既有用户数据；它们不代表新的产品职责边界。

## 文档入口

- [Native CLI v3 架构](docs/native-cli-v3.md)
- [Native CLI 执行账本](docs/superpowers/execution/)
- [D20 真实 CLI 认证命令](docs/testing/d20-real-cli-command.md)
- [终端集成](docs/terminal-integration.md)
- [数据持久化](docs/data-persistence.md)
- [发布流程](docs/release-process.md)
- [路线图](docs/roadmap.md)

## License

MIT.
