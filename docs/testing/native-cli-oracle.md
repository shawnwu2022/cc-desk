# Native CLI UserPromptSubmit Oracle

状态：**合成 collector 契约已实现；真实 Claude Code / Codex CLI 证据仍为 NOT_RUN。**

本文件定义 W0/D04 的输入证据边界。它不证明最终网络请求、模型接收、模型理解或响应完整性。

## 1. 要证明什么

针对一条合成 prompt，分别记录：

1. 原始 fixture；
2. CC Desk 宿主实际构造并写入 PTY 的 payload；
3. CLI 公开 `UserPromptSubmit` 事件的原始 envelope；
4. 固定版本下明确声明的预期转换。

一条有效结论必须关联：

```text
cli kind
+ exact binary version/hash
+ Desk run nonce
+ native session id
+ native turn id（仅实际事件提供时）
+ cwd
+ transform id
+ raw envelope
```

屏幕上的 `[Pasted text …]`、writer success、外部编辑器内容或模型复述都不能单独替代该证据。

## 2. 合成 collector

`scripts/native-cli/collect-prompt.mjs` 只允许在以下环境中执行：

```text
CC_DESK_SYNTHETIC_TEST_MODE=1
CC_DESK_TEST_ROOT=<隔离测试目录>
```

它具有以下约束：

- 只接收 `claude` 或 `codex`；
- fixture 和报告路径都必须位于测试根目录；
- stdin 最多 8 MiB；
- stdout 始终为空；
- 事件内容不匹配时写入失败结论但退出 0，避免改变 CLI Hook 流程；
- 环境、argv、token 和用户剪贴板不进入报告；
- prompt 严格逐字符比较，不 `trim`、不规范化换行、不删除看似粘贴包装的正文；
- 报告保留原始 envelope，便于固定版本转换规则复核；
- 不输出 `additionalContext`、权限决定、继续/停止命令或任何会改变模型上下文的内容。

该 collector 不进入产品运行时，不作为用户监控功能。

## 3. Codex 候选事件契约

当前官方 Codex Hooks 文档给出的 `UserPromptSubmit` 候选字段包括：

- 通用字段：`session_id`、`cwd`、`hook_event_name`；
- 事件字段：`turn_id`、`prompt`。

合成测试据此验证 session、turn、cwd、event name 和 prompt 的精确关联。但在线文档不是安装版本证明，真实测试必须记录实际 binary 版本、schema 和原始 envelope。

官方入口：`https://developers.openai.com/codex/hooks`

## 4. Claude Code 证据边界

在实际固定版本事件被采集前，不假设 Claude Code 一定提供 `turn_id`，也不通过最近修改的 transcript 文件猜测会话或轮次。

Claude Code 文档指出粘贴内容可能在 Hook prompt 中展开或带包装信息，因此真实验证必须：

- 保留原始 envelope；
- 为精确 CLI 版本定义 `transformId`；
- 加入用户正文自身含包装样式文本的反例；
- 禁止使用宽泛正则剥离所有相似标签；
- observer on/off 配对运行，确认 collector 不改变行为。

官方入口：`https://code.claude.com/docs/en/hooks`

## 5. 当前证据状态

| 项目 | 状态 | 后续责任 |
|---|---|---|
| collector 安全边界与精确比较 | 自动测试中验证 | D04 |
| 合成 Codex envelope | 自动测试中验证 | D04 |
| 合成 Claude envelope（不虚构 turn） | 自动测试中验证 | D04 |
| 实际 Codex CLI schema 与事件点 | NOT_RUN | W0 实机 / W6 |
| 实际 Claude Code schema 与合法转换 | NOT_RUN | W0 实机 / W6 |
| 宿主 payload 与 Hook envelope 对照 | NOT_RUN | W5 / W6 |
| observer 开启/关闭无行为差异 | NOT_RUN | W6 |
| 最终安装包证据 | NOT_RUN | W9 |

如果实际版本没有可靠公开事件、无法绑定该次 run，或 collector 改变了上下文/权限行为，对应组合状态必须为 **BLOCKED**，不能降级为模型复述或屏幕解析。
