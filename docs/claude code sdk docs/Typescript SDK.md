> ## Documentation Index
> Fetch the complete documentation index at: https://code.claude.com/docs/llms.txt
> Use this file to discover all available pages before exploring further.

# Agent SDK 参考 - TypeScript

> TypeScript Agent SDK 的完整 API 参考，包括所有函数、类型和接口。

<script src="/components/typescript-sdk-type-links.js" defer />

<Note>
  **尝试新的 V2 接口（预览版）：** 现已推出简化的接口，具有 `send()` 和 `stream()` 模式，使多轮对话更加容易。[了解有关 TypeScript V2 预览版的更多信息](/zh-CN/agent-sdk/typescript-v2-preview)
</Note>

## 安装

```bash theme={null}
npm install @anthropic-ai/claude-agent-sdk
```

<Note>
  SDK 为您的平台捆绑了一个本地 Claude Code 二进制文件，作为可选依赖项，例如 `@anthropic-ai/claude-agent-sdk-darwin-arm64`。您无需单独安装 Claude Code。如果您的包管理器跳过可选依赖项，SDK 会抛出 `Native CLI binary for <platform> not found`；改为将 [`pathToClaudeCodeExecutable`](#options) 设置为单独安装的 `claude` 二进制文件。
</Note>

## 函数

### `query()`

与 Claude Code 交互的主要函数。创建一个异步生成器，在消息到达时流式传输消息。

```typescript theme={null}
function query({
  prompt,
  options
}: {
  prompt: string | AsyncIterable<SDKUserMessage>;
  options?: Options;
}): Query;
```

#### 参数

| 参数        | 类型                                                                | 描述                          |
| :-------- | :---------------------------------------------------------------- | :-------------------------- |
| `prompt`  | `string \| AsyncIterable<`[`SDKUserMessage`](#sdkuser-message)`>` | 输入提示，可以是字符串或异步可迭代对象（用于流式模式） |
| `options` | [`Options`](#options)                                             | 可选配置对象（请参阅下面的 Options 类型）   |

#### 返回值

返回一个 [`Query`](#query-object) 对象，该对象扩展 `AsyncGenerator<`[`SDKMessage`](#sdk-message)`, void>`，并具有其他方法。

### `startup()`

通过生成 CLI 子进程并在提示可用之前完成初始化握手来预热 CLI 子进程。返回的 [`WarmQuery`](#warm-query) 句柄稍后接受提示并将其写入已准备好的进程，因此第一个 `query()` 调用解析时无需支付子进程生成和初始化成本。

```typescript theme={null}
function startup(params?: {
  options?: Options;
  initializeTimeoutMs?: number;
}): Promise<WarmQuery>;
```

#### 参数

| 参数                    | 类型                    | 描述                                                            |
| :-------------------- | :-------------------- | :------------------------------------------------------------ |
| `options`             | [`Options`](#options) | 可选配置对象。与 `query()` 的 `options` 参数相同                           |
| `initializeTimeoutMs` | `number`              | 等待子进程初始化的最长时间（毫秒）。默认为 `60000`。如果初始化未在规定时间内完成，promise 将以超时错误拒绝 |

#### 返回值

返回一个 `Promise<`[`WarmQuery`](#warm-query)`>`，在子进程生成并完成其初始化握手后解析。

#### 示例

早期调用 `startup()`，例如在应用程序启动时，然后在提示准备好后在返回的句柄上调用 `.query()`。这会将子进程生成和初始化移出关键路径。

```typescript theme={null}
import { startup } from "@anthropic-ai/claude-agent-sdk";

// 提前支付启动成本
const warm = await startup({ options: { maxTurns: 3 } });

// 稍后，当提示准备好时，这是立即的
for await (const message of warm.query("What files are here?")) {
  console.log(message);
}
```

### `tool()`

为与 SDK MCP 服务器一起使用创建类型安全的 MCP 工具定义。

```typescript theme={null}
function tool<Schema extends AnyZodRawShape>(
  name: string,
  description: string,
  inputSchema: Schema,
  handler: (args: InferShape<Schema>, extra: unknown) => Promise<CallToolResult>,
  extras?: { annotations?: ToolAnnotations }
): SdkMcpToolDefinition<Schema>;
```

#### 参数

| 参数            | 类型                                                                  | 描述                                 |
| :------------ | :------------------------------------------------------------------ | :--------------------------------- |
| `name`        | `string`                                                            | 工具的名称                              |
| `description` | `string`                                                            | 工具功能的描述                            |
| `inputSchema` | `Schema extends AnyZodRawShape`                                     | 定义工具输入参数的 Zod 架构（支持 Zod 3 和 Zod 4） |
| `handler`     | `(args, extra) => Promise<`[`CallToolResult`](#call-tool-result)`>` | 执行工具逻辑的异步函数                        |
| `extras`      | `{ annotations?: `[`ToolAnnotations`](#tool-annotations)` }`        | 可选的 MCP 工具注释，为客户端提供行为提示            |

#### `ToolAnnotations`

从 `@modelcontextprotocol/sdk/types.js` 重新导出。所有字段都是可选提示；客户端不应依赖它们做出安全决策。

| 字段                | 类型        | 默认值         | 描述                                                             |
| :---------------- | :-------- | :---------- | :------------------------------------------------------------- |
| `title`           | `string`  | `undefined` | 工具的人类可读标题                                                      |
| `readOnlyHint`    | `boolean` | `false`     | 如果为 `true`，工具不会修改其环境                                           |
| `destructiveHint` | `boolean` | `true`      | 如果为 `true`，工具可能执行破坏性更新（仅在 `readOnlyHint` 为 `false` 时有意义）       |
| `idempotentHint`  | `boolean` | `false`     | 如果为 `true`，使用相同参数的重复调用没有额外效果（仅在 `readOnlyHint` 为 `false` 时有意义） |
| `openWorldHint`   | `boolean` | `true`      | 如果为 `true`，工具与外部实体交互（例如，网络搜索）。如果为 `false`，工具的域是封闭的（例如，内存工具）    |

```typescript theme={null}
import { tool } from "@anthropic-ai/claude-agent-sdk";
import { z } from "zod";

const searchTool = tool(
  "search",
  "Search the web",
  { query: z.string() },
  async ({ query }) => {
    return { content: [{ type: "text", text: `Results for: ${query}` }] };
  },
  { annotations: { readOnlyHint: true, openWorldHint: true } }
);
```

### `createSdkMcpServer()`

创建在与应用程序相同的进程中运行的 MCP 服务器实例。

```typescript theme={null}
function createSdkMcpServer(options: {
  name: string;
  version?: string;
  tools?: Array<SdkMcpToolDefinition<any>>;
}): McpSdkServerConfigWithInstance;
```

#### 参数

| 参数                | 类型                            | 描述                             |
| :---------------- | :---------------------------- | :----------------------------- |
| `options.name`    | `string`                      | MCP 服务器的名称                     |
| `options.version` | `string`                      | 可选版本字符串                        |
| `options.tools`   | `Array<SdkMcpToolDefinition>` | 使用 [`tool()`](#tool) 创建的工具定义数组 |

### `listSessions()`

发现并列出具有轻量级元数据的过去会话。按项目目录筛选或列出所有项目中的会话。

```typescript theme={null}
function listSessions(options?: ListSessionsOptions): Promise<SDKSessionInfo[]>;
```

#### 参数

| 参数                         | 类型        | 默认值         | 描述                                        |
| :------------------------- | :-------- | :---------- | :---------------------------------------- |
| `options.dir`              | `string`  | `undefined` | 列出会话的目录。省略时，返回所有项目中的会话                    |
| `options.limit`            | `number`  | `undefined` | 要返回的最大会话数                                 |
| `options.includeWorktrees` | `boolean` | `true`      | 当 `dir` 在 git 存储库内时，包括来自所有 worktree 路径的会话 |

#### 返回类型：`SDKSessionInfo`

| 属性             | 类型                    | 描述                                            |
| :------------- | :-------------------- | :-------------------------------------------- |
| `sessionId`    | `string`              | 唯一会话标识符 (UUID)                                |
| `summary`      | `string`              | 显示标题：自定义标题、自动生成的摘要或第一个提示                      |
| `lastModified` | `number`              | 上次修改时间（自纪元以来的毫秒数）                             |
| `fileSize`     | `number \| undefined` | 会话文件大小（字节）。仅对本地 JSONL 存储进行填充                  |
| `customTitle`  | `string \| undefined` | 用户设置的会话标题（通过 `/rename`）                       |
| `firstPrompt`  | `string \| undefined` | 会话中的第一个有意义的用户提示                               |
| `gitBranch`    | `string \| undefined` | 会话结束时的 git 分支                                 |
| `cwd`          | `string \| undefined` | 会话的工作目录                                       |
| `tag`          | `string \| undefined` | 用户设置的会话标签（请参阅 [`tagSession()`](#tag-session)） |
| `createdAt`    | `number \| undefined` | 创建时间（自纪元以来的毫秒数），来自第一个条目的时间戳                   |

#### 示例

打印项目的 10 个最近会话。结果按 `lastModified` 降序排序，因此第一项是最新的。省略 `dir` 以搜索所有项目。

```typescript theme={null}
import { listSessions } from "@anthropic-ai/claude-agent-sdk";

const sessions = await listSessions({ dir: "/path/to/project", limit: 10 });

for (const session of sessions) {
  console.log(`${session.summary} (${session.sessionId})`);
}
```

### `getSessionMessages()`

从过去的会话记录中读取用户和助手消息。

```typescript theme={null}
function getSessionMessages(
  sessionId: string,
  options?: GetSessionMessagesOptions
): Promise<SessionMessage[]>;
```

#### 参数

| 参数               | 类型       | 默认值         | 描述                                |
| :--------------- | :------- | :---------- | :-------------------------------- |
| `sessionId`      | `string` | 必需          | 要读取的会话 UUID（请参阅 `listSessions()`） |
| `options.dir`    | `string` | `undefined` | 查找会话的项目目录。省略时，搜索所有项目              |
| `options.limit`  | `number` | `undefined` | 要返回的最大消息数                         |
| `options.offset` | `number` | `undefined` | 从开始跳过的消息数                         |

#### 返回类型：`SessionMessage`

| 属性                   | 类型                      | 描述            |
| :------------------- | :---------------------- | :------------ |
| `type`               | `"user" \| "assistant"` | 消息角色          |
| `uuid`               | `string`                | 唯一消息标识符       |
| `session_id`         | `string`                | 此消息所属的会话      |
| `message`            | `unknown`               | 来自记录的原始消息有效负载 |
| `parent_tool_use_id` | `null`                  | 保留            |

#### 示例

```typescript theme={null}
import { listSessions, getSessionMessages } from "@anthropic-ai/claude-agent-sdk";

const [latest] = await listSessions({ dir: "/path/to/project", limit: 1 });

if (latest) {
  const messages = await getSessionMessages(latest.sessionId, {
    dir: "/path/to/project",
    limit: 20
  });

  for (const msg of messages) {
    console.log(`[${msg.type}] ${msg.uuid}`);
  }
}
```

### `getSessionInfo()`

按 ID 读取单个会话的元数据，无需扫描完整项目目录。

```typescript theme={null}
function getSessionInfo(
  sessionId: string,
  options?: GetSessionInfoOptions
): Promise<SDKSessionInfo | undefined>;
```

#### 参数

| 参数            | 类型       | 默认值         | 描述                  |
| :------------ | :------- | :---------- | :------------------ |
| `sessionId`   | `string` | 必需          | 要查找的会话 UUID         |
| `options.dir` | `string` | `undefined` | 项目目录路径。省略时，搜索所有项目目录 |

返回 [`SDKSessionInfo`](#return-type-sdk-session-info)，如果找不到会话，则返回 `undefined`。

### `renameSession()`

通过附加自定义标题条目来重命名会话。重复调用是安全的；最新的标题获胜。

```typescript theme={null}
function renameSession(
  sessionId: string,
  title: string,
  options?: SessionMutationOptions
): Promise<void>;
```

#### 参数

| 参数            | 类型       | 默认值         | 描述                  |
| :------------ | :------- | :---------- | :------------------ |
| `sessionId`   | `string` | 必需          | 要重命名的会话 UUID        |
| `title`       | `string` | 必需          | 新标题。修剪空格后必须非空       |
| `options.dir` | `string` | `undefined` | 项目目录路径。省略时，搜索所有项目目录 |

### `tagSession()`

标记会话。传递 `null` 以清除标签。重复调用是安全的；最新的标签获胜。

```typescript theme={null}
function tagSession(
  sessionId: string,
  tag: string | null,
  options?: SessionMutationOptions
): Promise<void>;
```

#### 参数

| 参数            | 类型               | 默认值         | 描述                  |
| :------------ | :--------------- | :---------- | :------------------ |
| `sessionId`   | `string`         | 必需          | 要标记的会话 UUID         |
| `tag`         | `string \| null` | 必需          | 标签字符串，或 `null` 以清除  |
| `options.dir` | `string`         | `undefined` | 项目目录路径。省略时，搜索所有项目目录 |

## 类型

### `Options`

`query()` 函数的配置对象。

| 属性                                | 类型                                                                                                       | 默认值                           | 描述                                                                                                                                                                                                                                                                                            |
| :-------------------------------- | :------------------------------------------------------------------------------------------------------- | :---------------------------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `abortController`                 | `AbortController`                                                                                        | `new AbortController()`       | 用于取消操作的控制器                                                                                                                                                                                                                                                                                    |
| `additionalDirectories`           | `string[]`                                                                                               | `[]`                          | Claude 可以访问的其他目录                                                                                                                                                                                                                                                                              |
| `agent`                           | `string`                                                                                                 | `undefined`                   | 主线程的代理名称。代理必须在 `agents` 选项或设置中定义                                                                                                                                                                                                                                                              |
| `agents`                          | `Record<string, [`AgentDefinition`](#agent-definition)>`                                                 | `undefined`                   | 以编程方式定义子代理                                                                                                                                                                                                                                                                                    |
| `allowDangerouslySkipPermissions` | `boolean`                                                                                                | `false`                       | 启用绕过权限。使用 `permissionMode: 'bypassPermissions'` 时需要                                                                                                                                                                                                                                           |
| `allowedTools`                    | `string[]`                                                                                               | `[]`                          | 无需提示即可自动批准的工具。这不会将 Claude 限制为仅这些工具；未列出的工具会通过 `permissionMode` 和 `canUseTool` 进行处理。使用 `disallowedTools` 阻止工具。请参阅[权限](/zh-CN/agent-sdk/permissions#allow-and-deny-rules)                                                                                                                        |
| `betas`                           | [`SdkBeta`](#sdk-beta)`[]`                                                                               | `[]`                          | 启用测试功能                                                                                                                                                                                                                                                                                        |
| `canUseTool`                      | [`CanUseTool`](#can-use-tool)                                                                            | `undefined`                   | 工具使用的自定义权限函数                                                                                                                                                                                                                                                                                  |
| `continue`                        | `boolean`                                                                                                | `false`                       | 继续最近的对话                                                                                                                                                                                                                                                                                       |
| `cwd`                             | `string`                                                                                                 | `process.cwd()`               | 当前工作目录                                                                                                                                                                                                                                                                                        |
| `debug`                           | `boolean`                                                                                                | `false`                       | 为 Claude Code 进程启用调试模式                                                                                                                                                                                                                                                                        |
| `debugFile`                       | `string`                                                                                                 | `undefined`                   | 将调试日志写入特定文件路径。隐式启用调试模式                                                                                                                                                                                                                                                                        |
| `disallowedTools`                 | `string[]`                                                                                               | `[]`                          | 始终拒绝的工具。拒绝规则首先检查并覆盖 `allowedTools` 和 `permissionMode`（包括 `bypassPermissions`）                                                                                                                                                                                                                 |
| `effort`                          | `'low' \| 'medium' \| 'high' \| 'xhigh' \| 'max'`                                                        | `'high'`                      | 控制 Claude 在其响应中投入的努力程度。与自适应思考一起工作以指导思考深度                                                                                                                                                                                                                                                      |
| `enableFileCheckpointing`         | `boolean`                                                                                                | `false`                       | 启用文件更改跟踪以进行回滚。请参阅[文件检查点](/zh-CN/agent-sdk/file-checkpointing)                                                                                                                                                                                                                                 |
| `env`                             | `Record<string, string \| undefined>`                                                                    | `process.env`                 | 环境变量。设置 `CLAUDE_AGENT_SDK_CLIENT_APP` 以在 User-Agent 标头中标识您的应用                                                                                                                                                                                                                                 |
| `executable`                      | `'bun' \| 'deno' \| 'node'`                                                                              | 自动检测                          | 要使用的 JavaScript 运行时                                                                                                                                                                                                                                                                           |
| `executableArgs`                  | `string[]`                                                                                               | `[]`                          | 传递给可执行文件的参数                                                                                                                                                                                                                                                                                   |
| `extraArgs`                       | `Record<string, string \| null>`                                                                         | `{}`                          | 其他参数                                                                                                                                                                                                                                                                                          |
| `fallbackModel`                   | `string`                                                                                                 | `undefined`                   | 主模型失败时使用的模型                                                                                                                                                                                                                                                                                   |
| `forkSession`                     | `boolean`                                                                                                | `false`                       | 使用 `resume` 恢复时，分叉到新会话 ID 而不是继续原始会话                                                                                                                                                                                                                                                           |
| `hooks`                           | `Partial<Record<`[`HookEvent`](#hook-event)`, `[`HookCallbackMatcher`](#hook-callback-matcher)`[]>>`     | `{}`                          | 事件的 Hook 回调                                                                                                                                                                                                                                                                                   |
| `includePartialMessages`          | `boolean`                                                                                                | `false`                       | 包括部分消息事件                                                                                                                                                                                                                                                                                      |
| `maxBudgetUsd`                    | `number`                                                                                                 | `undefined`                   | 当客户端成本估计达到此 USD 值时停止查询。与 `total_cost_usd` 的相同估计进行比较；请参阅[跟踪成本和使用情况](/zh-CN/agent-sdk/cost-tracking)了解准确性注意事项                                                                                                                                                                                   |
| `maxThinkingTokens`               | `number`                                                                                                 | `undefined`                   | *已弃用：* 改用 `thinking`。思考过程的最大令牌数                                                                                                                                                                                                                                                               |
| `maxTurns`                        | `number`                                                                                                 | `undefined`                   | 最大代理轮次（工具使用往返）                                                                                                                                                                                                                                                                                |
| `mcpServers`                      | `Record<string, [`McpServerConfig`](#mcp-server-config)>`                                                | `{}`                          | MCP 服务器配置                                                                                                                                                                                                                                                                                     |
| `model`                           | `string`                                                                                                 | CLI 的默认值                      | 要使用的 Claude 模型                                                                                                                                                                                                                                                                                |
| `outputFormat`                    | `{ type: 'json_schema', schema: JSONSchema }`                                                            | `undefined`                   | 为代理结果定义输出格式。请参阅[结构化输出](/zh-CN/agent-sdk/structured-outputs)了解详情                                                                                                                                                                                                                               |
| `pathToClaudeCodeExecutable`      | `string`                                                                                                 | 从捆绑的本地二进制文件自动解析               | Claude Code 可执行文件的路径。仅在安装期间跳过可选依赖项或您的平台不在支持的集合中时需要                                                                                                                                                                                                                                            |
| `permissionMode`                  | [`PermissionMode`](#permission-mode)                                                                     | `'default'`                   | 会话的权限模式                                                                                                                                                                                                                                                                                       |
| `permissionPromptToolName`        | `string`                                                                                                 | `undefined`                   | 权限提示的 MCP 工具名称                                                                                                                                                                                                                                                                                |
| `persistSession`                  | `boolean`                                                                                                | `true`                        | 当为 `false` 时，禁用会话持久化到磁盘。会话之后无法恢复                                                                                                                                                                                                                                                              |
| `plugins`                         | [`SdkPluginConfig`](#sdk-plugin-config)`[]`                                                              | `[]`                          | 从本地路径加载自定义插件。请参阅[插件](/zh-CN/agent-sdk/plugins)了解详情                                                                                                                                                                                                                                            |
| `promptSuggestions`               | `boolean`                                                                                                | `false`                       | 启用提示建议。在每个轮次后发出 `prompt_suggestion` 消息，包含预测的下一个用户提示                                                                                                                                                                                                                                           |
| `resume`                          | `string`                                                                                                 | `undefined`                   | 要恢复的会话 ID                                                                                                                                                                                                                                                                                     |
| `resumeSessionAt`                 | `string`                                                                                                 | `undefined`                   | 在特定消息 UUID 处恢复会话                                                                                                                                                                                                                                                                              |
| `sandbox`                         | [`SandboxSettings`](#sandbox-settings)                                                                   | `undefined`                   | 以编程方式配置沙箱行为。请参阅[沙箱设置](#sandbox-settings)了解详情                                                                                                                                                                                                                                                  |
| `sessionId`                       | `string`                                                                                                 | 自动生成                          | 为会话使用特定的 UUID 而不是自动生成一个                                                                                                                                                                                                                                                                       |
| `settingSources`                  | [`SettingSource`](#setting-source)`[]`                                                                   | CLI 默认值（所有源）                  | 控制加载哪些文件系统设置。传递 `[]` 以禁用用户、项目和本地设置。无论如何都会加载托管策略设置。请参阅[使用 Claude Code 功能](/zh-CN/agent-sdk/claude-code-features#what-settingsources-does-not-control)                                                                                                                                          |
| `spawnClaudeCodeProcess`          | `(options: SpawnOptions) => SpawnedProcess`                                                              | `undefined`                   | 用于生成 Claude Code 进程的自定义函数。用于在 VM、容器或远程环境中运行 Claude Code                                                                                                                                                                                                                                       |
| `stderr`                          | `(data: string) => void`                                                                                 | `undefined`                   | stderr 输出的回调                                                                                                                                                                                                                                                                                  |
| `strictMcpConfig`                 | `boolean`                                                                                                | `false`                       | 强制执行严格的 MCP 验证                                                                                                                                                                                                                                                                                |
| `systemPrompt`                    | `string \| { type: 'preset'; preset: 'claude_code'; append?: string; excludeDynamicSections?: boolean }` | `undefined`（最小提示）             | 系统提示配置。传递字符串以获取自定义提示，或 `{ type: 'preset', preset: 'claude_code' }` 以使用 Claude Code 的系统提示。使用预设对象形式时，添加 `append` 以使用其他说明扩展它，并设置 `excludeDynamicSections: true` 以将每个会话上下文移到第一条用户消息中，以便[更好地跨机器重用提示缓存](/zh-CN/agent-sdk/modifying-system-prompts#improve-prompt-caching-across-users-and-machines) |
| `thinking`                        | [`ThinkingConfig`](#thinking-config)                                                                     | 支持的模型为 `{ type: 'adaptive' }` | 控制 Claude 的思考/推理行为。请参阅 [`ThinkingConfig`](#thinking-config) 了解选项                                                                                                                                                                                                                              |
| `toolConfig`                      | [`ToolConfig`](#tool-config)                                                                             | `undefined`                   | 内置工具行为的配置。请参阅 [`ToolConfig`](#tool-config) 了解详情                                                                                                                                                                                                                                               |
| `tools`                           | `string[] \| { type: 'preset'; preset: 'claude_code' }`                                                  | `undefined`                   | 工具配置。传递工具名称数组或使用预设获取 Claude Code 的默认工具                                                                                                                                                                                                                                                        |

### `Query` 对象

由 `query()` 函数返回的接口。

```typescript theme={null}
interface Query extends AsyncGenerator<SDKMessage, void> {
  interrupt(): Promise<void>;
  rewindFiles(
    userMessageId: string,
    options?: { dryRun?: boolean }
  ): Promise<RewindFilesResult>;
  setPermissionMode(mode: PermissionMode): Promise<void>;
  setModel(model?: string): Promise<void>;
  setMaxThinkingTokens(maxThinkingTokens: number | null): Promise<void>;
  initializationResult(): Promise<SDKControlInitializeResponse>;
  supportedCommands(): Promise<SlashCommand[]>;
  supportedModels(): Promise<ModelInfo[]>;
  supportedAgents(): Promise<AgentInfo[]>;
  mcpServerStatus(): Promise<McpServerStatus[]>;
  accountInfo(): Promise<AccountInfo>;
  reconnectMcpServer(serverName: string): Promise<void>;
  toggleMcpServer(serverName: string, enabled: boolean): Promise<void>;
  setMcpServers(servers: Record<string, McpServerConfig>): Promise<McpSetServersResult>;
  streamInput(stream: AsyncIterable<SDKUserMessage>): Promise<void>;
  stopTask(taskId: string): Promise<void>;
  close(): void;
}
```

#### 方法

| 方法                                     | 描述                                                                                                                              |
| :------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------ |
| `interrupt()`                          | 中断查询（仅在流式输入模式下可用）                                                                                                               |
| `rewindFiles(userMessageId, options?)` | 将文件恢复到指定用户消息时的状态。传递 `{ dryRun: true }` 以预览更改。需要 `enableFileCheckpointing: true`。请参阅[文件检查点](/zh-CN/agent-sdk/file-checkpointing) |
| `setPermissionMode()`                  | 更改权限模式（仅在流式输入模式下可用）                                                                                                             |
| `setModel()`                           | 更改模型（仅在流式输入模式下可用）                                                                                                               |
| `setMaxThinkingTokens()`               | *已弃用：* 改用 `thinking` 选项。更改最大思考令牌数                                                                                               |
| `initializationResult()`               | 返回完整的初始化结果，包括支持的命令、模型、帐户信息和输出样式配置                                                                                               |
| `supportedCommands()`                  | 返回可用的 slash commands                                                                                                            |
| `supportedModels()`                    | 返回具有显示信息的可用模型                                                                                                                   |
| `supportedAgents()`                    | 返回可用的子代理作为 [`AgentInfo`](#agent-info)`[]`                                                                                       |
| `mcpServerStatus()`                    | 返回连接的 MCP 服务器的状态                                                                                                                |
| `accountInfo()`                        | 返回帐户信息                                                                                                                          |
| `reconnectMcpServer(serverName)`       | 按名称重新连接 MCP 服务器                                                                                                                 |
| `toggleMcpServer(serverName, enabled)` | 按名称启用或禁用 MCP 服务器                                                                                                                |
| `setMcpServers(servers)`               | 动态替换此会话的 MCP 服务器集。返回有关添加、删除的服务器和任何错误的信息                                                                                         |
| `streamInput(stream)`                  | 将输入消息流式传输到查询以进行多轮对话                                                                                                             |
| `stopTask(taskId)`                     | 按 ID 停止运行的后台任务                                                                                                                  |
| `close()`                              | 关闭查询并终止底层进程。强制结束查询并清理所有资源                                                                                                       |

### `WarmQuery`

由 [`startup()`](#startup) 返回的句柄。子进程已生成并初始化，因此在此句柄上调用 `query()` 会直接将提示写入准备好的进程，无需启动延迟。

```typescript theme={null}
interface WarmQuery extends AsyncDisposable {
  query(prompt: string | AsyncIterable<SDKUserMessage>): Query;
  close(): void;
}
```

#### 方法

| 方法              | 描述                                                            |
| :-------------- | :------------------------------------------------------------ |
| `query(prompt)` | 向预热的子进程发送提示并返回 [`Query`](#query-object)。每个 `WarmQuery` 只能调用一次 |
| `close()`       | 关闭子进程而不发送提示。使用此方法丢弃不再需要的预热查询                                  |

`WarmQuery` 实现 `AsyncDisposable`，因此可以与 `await using` 一起使用以进行自动清理。

### `SDKControlInitializeResponse`

`initializationResult()` 的返回类型。包含会话初始化数据。

```typescript theme={null}
type SDKControlInitializeResponse = {
  commands: SlashCommand[];
  agents: AgentInfo[];
  output_style: string;
  available_output_styles: string[];
  models: ModelInfo[];
  account: AccountInfo;
  fast_mode_state?: "off" | "cooldown" | "on";
};
```

### `AgentDefinition`

以编程方式定义的子代理的配置。

```typescript theme={null}
type AgentDefinition = {
  description: string;
  tools?: string[];
  disallowedTools?: string[];
  prompt: string;
  model?: "sonnet" | "opus" | "haiku" | "inherit";
  mcpServers?: AgentMcpServerSpec[];
  skills?: string[];
  maxTurns?: number;
  criticalSystemReminder_EXPERIMENTAL?: string;
};
```

| 字段                                    | 必需 | 描述                               |
| :------------------------------------ | :- | :------------------------------- |
| `description`                         | 是  | 何时使用此代理的自然语言描述                   |
| `tools`                               | 否  | 允许的工具名称数组。如果省略，继承父级的所有工具         |
| `disallowedTools`                     | 否  | 要为此代理明确禁止的工具名称数组                 |
| `prompt`                              | 是  | 代理的系统提示                          |
| `model`                               | 否  | 此代理的模型覆盖。如果省略或 `'inherit'`，使用主模型 |
| `mcpServers`                          | 否  | 此代理的 MCP 服务器规范                   |
| `skills`                              | 否  | 要预加载到代理上下文中的技能名称数组               |
| `maxTurns`                            | 否  | 停止前的最大代理轮次数（API 往返）              |
| `criticalSystemReminder_EXPERIMENTAL` | 否  | 实验性：添加到系统提示的关键提醒                 |

### `AgentMcpServerSpec`

指定子代理可用的 MCP 服务器。可以是服务器名称（字符串，引用父级 `mcpServers` 配置中的服务器）或内联服务器配置记录，将服务器名称映射到配置。

```typescript theme={null}
type AgentMcpServerSpec = string | Record<string, McpServerConfigForProcessTransport>;
```

其中 `McpServerConfigForProcessTransport` 是 `McpStdioServerConfig | McpSSEServerConfig | McpHttpServerConfig | McpSdkServerConfig`。

### `SettingSource`

控制 SDK 从哪些基于文件系统的配置源加载设置。

```typescript theme={null}
type SettingSource = "user" | "project" | "local";
```

| 值           | 描述                 | 位置                            |
| :---------- | :----------------- | :---------------------------- |
| `'user'`    | 全局用户设置             | `~/.claude/settings.json`     |
| `'project'` | 共享项目设置（版本控制）       | `.claude/settings.json`       |
| `'local'`   | 本地项目设置（gitignored） | `.claude/settings.local.json` |

#### 默认行为

当 `settingSources` 被省略或 `undefined` 时，`query()` 加载与 Claude Code CLI 相同的文件系统设置：用户、项目和本地。在所有情况下都会加载托管策略设置。请参阅[settingSources 不控制的内容](/zh-CN/agent-sdk/claude-code-features#what-settingsources-does-not-control)了解无论此选项如何都会读取的输入，以及如何禁用它们。

#### 为什么使用 settingSources

**禁用文件系统设置：**

```typescript theme={null}
// 不从磁盘加载用户、项目或本地设置
const result = query({
  prompt: "Analyze this code",
  options: { settingSources: [] }
});
```

**显式加载所有文件系统设置：**

```typescript theme={null}
const result = query({
  prompt: "Analyze this code",
  options: {
    settingSources: ["user", "project", "local"] // 加载所有设置
  }
});
```

**仅加载特定设置源：**

```typescript theme={null}
// 仅加载项目设置，忽略用户和本地
const result = query({
  prompt: "Run CI checks",
  options: {
    settingSources: ["project"] // 仅 .claude/settings.json
  }
});
```

**测试和 CI 环境：**

```typescript theme={null}
// 通过排除本地设置确保 CI 中的一致行为
const result = query({
  prompt: "Run tests",
  options: {
    settingSources: ["project"], // 仅团队共享设置
    permissionMode: "bypassPermissions"
  }
});
```

**仅 SDK 应用程序：**

```typescript theme={null}
// 以编程方式定义所有内容。
// 传递 [] 以选择退出文件系统设置源。
const result = query({
  prompt: "Review this PR",
  options: {
    settingSources: [],
    agents: {
      /* ... */
    },
    mcpServers: {
      /* ... */
    },
    allowedTools: ["Read", "Grep", "Glob"]
  }
});
```

**加载 CLAUDE.md 项目说明：**

```typescript theme={null}
// 加载项目设置以包括 CLAUDE.md 文件
const result = query({
  prompt: "Add a new feature following project conventions",
  options: {
    systemPrompt: {
      type: "preset",
      preset: "claude_code" // 使用 Claude Code 的系统提示
    },
    settingSources: ["project"], // 从项目目录加载 CLAUDE.md
    allowedTools: ["Read", "Write", "Edit"]
  }
});
```

#### 设置优先级

加载多个源时，设置按此优先级合并（从高到低）：

1. 本地设置（`.claude/settings.local.json`）
2. 项目设置（`.claude/settings.json`）
3. 用户设置（`~/.claude/settings.json`）

编程选项（如 `agents` 和 `allowedTools`）覆盖用户、项目和本地文件系统设置。托管策略设置优先于编程选项。

### `PermissionMode`

```typescript theme={null}
type PermissionMode =
  | "default" // 标准权限行为
  | "acceptEdits" // 自动接受文件编辑
  | "bypassPermissions" // 绕过所有权限检查
  | "plan" // 规划模式 - 无执行
  | "dontAsk" // 不提示权限，如果未预先批准则拒绝
  | "auto"; // 使用模型分类器批准或拒绝每个工具调用
```

### `CanUseTool`

用于控制工具使用的自定义权限函数类型。

```typescript theme={null}
type CanUseTool = (
  toolName: string,
  input: Record<string, unknown>,
  options: {
    signal: AbortSignal;
    suggestions?: PermissionUpdate[];
    blockedPath?: string;
    decisionReason?: string;
    toolUseID: string;
    agentID?: string;
  }
) => Promise<PermissionResult>;
```

| 选项               | 类型                                           | 描述                     |
| :--------------- | :------------------------------------------- | :--------------------- |
| `signal`         | `AbortSignal`                                | 如果应中止操作，则发出信号          |
| `suggestions`    | [`PermissionUpdate`](#permission-update)`[]` | 建议的权限更新，以便用户不会再次被提示此工具 |
| `blockedPath`    | `string`                                     | 触发权限请求的文件路径（如果适用）      |
| `decisionReason` | `string`                                     | 解释为什么触发此权限请求           |
| `toolUseID`      | `string`                                     | 此特定工具调用在助手消息中的唯一标识符    |
| `agentID`        | `string`                                     | 如果在子代理中运行，子代理的 ID      |

### `PermissionResult`

权限检查的结果。

```typescript theme={null}
type PermissionResult =
  | {
      behavior: "allow";
      updatedInput?: Record<string, unknown>;
      updatedPermissions?: PermissionUpdate[];
      toolUseID?: string;
    }
  | {
      behavior: "deny";
      message: string;
      interrupt?: boolean;
      toolUseID?: string;
    };
```

### `ToolConfig`

内置工具行为的配置。

```typescript theme={null}
type ToolConfig = {
  askUserQuestion?: {
    previewFormat?: "markdown" | "html";
  };
};
```

| 字段                              | 类型                     | 描述                                                                                                                |
| :------------------------------ | :--------------------- | :---------------------------------------------------------------------------------------------------------------- |
| `askUserQuestion.previewFormat` | `'markdown' \| 'html'` | 选择加入 [`AskUserQuestion`](/zh-CN/agent-sdk/user-input#question-format) 选项上的 `preview` 字段并设置其内容格式。未设置时，Claude 不发出预览 |

### `McpServerConfig`

MCP 服务器的配置。

```typescript theme={null}
type McpServerConfig =
  | McpStdioServerConfig
  | McpSSEServerConfig
  | McpHttpServerConfig
  | McpSdkServerConfigWithInstance;
```

#### `McpStdioServerConfig`

```typescript theme={null}
type McpStdioServerConfig = {
  type?: "stdio";
  command: string;
  args?: string[];
  env?: Record<string, string>;
};
```

#### `McpSSEServerConfig`

```typescript theme={null}
type McpSSEServerConfig = {
  type: "sse";
  url: string;
  headers?: Record<string, string>;
};
```

#### `McpHttpServerConfig`

```typescript theme={null}
type McpHttpServerConfig = {
  type: "http";
  url: string;
  headers?: Record<string, string>;
};
```

#### `McpSdkServerConfigWithInstance`

```typescript theme={null}
type McpSdkServerConfigWithInstance = {
  type: "sdk";
  name: string;
  instance: McpServer;
};
```

#### `McpClaudeAIProxyServerConfig`

```typescript theme={null}
type McpClaudeAIProxyServerConfig = {
  type: "claudeai-proxy";
  url: string;
  id: string;
};
```

### `SdkPluginConfig`

SDK 中加载插件的配置。

```typescript theme={null}
type SdkPluginConfig = {
  type: "local";
  path: string;
};
```

| 字段     | 类型        | 描述                       |
| :----- | :-------- | :----------------------- |
| `type` | `'local'` | 必须为 `'local'`（目前仅支持本地插件） |
| `path` | `string`  | 插件目录的绝对或相对路径             |

**示例：**

```typescript theme={null}
plugins: [
  { type: "local", path: "./my-plugin" },
  { type: "local", path: "/absolute/path/to/plugin" }
];
```

有关创建和使用插件的完整信息，请参阅[插件](/zh-CN/agent-sdk/plugins)。

## 消息类型

### `SDKMessage`

查询返回的所有可能消息的联合类型。

```typescript theme={null}
type SDKMessage =
  | SDKAssistantMessage
  | SDKUserMessage
  | SDKUserMessageReplay
  | SDKResultMessage
  | SDKSystemMessage
  | SDKPartialAssistantMessage
  | SDKCompactBoundaryMessage
  | SDKStatusMessage
  | SDKLocalCommandOutputMessage
  | SDKHookStartedMessage
  | SDKHookProgressMessage
  | SDKHookResponseMessage
  | SDKPluginInstallMessage
  | SDKToolProgressMessage
  | SDKAuthStatusMessage
  | SDKTaskNotificationMessage
  | SDKTaskStartedMessage
  | SDKTaskProgressMessage
  | SDKTaskUpdatedMessage
  | SDKFilesPersistedEvent
  | SDKToolUseSummaryMessage
  | SDKRateLimitEvent
  | SDKPromptSuggestionMessage;
```

### `SDKAssistantMessage`

助手响应消息。

```typescript theme={null}
type SDKAssistantMessage = {
  type: "assistant";
  uuid: UUID;
  session_id: string;
  message: BetaMessage; // 来自 Anthropic SDK
  parent_tool_use_id: string | null;
  error?: SDKAssistantMessageError;
};
```

`message` 字段是来自 Anthropic SDK 的 [`BetaMessage`](https://platform.claude.com/docs/zh-CN/api/messages/create)。它包括 `id`、`content`、`model`、`stop_reason` 和 `usage` 等字段。

`SDKAssistantMessageError` 是以下之一：`'authentication_failed'`、`'billing_error'`、`'rate_limit'`、`'invalid_request'`、`'server_error'`、`'max_output_tokens'` 或 `'unknown'`。

### `SDKUserMessage`

用户输入消息。

```typescript theme={null}
type SDKUserMessage = {
  type: "user";
  uuid?: UUID;
  session_id: string;
  message: MessageParam; // 来自 Anthropic SDK
  parent_tool_use_id: string | null;
  isSynthetic?: boolean;
  shouldQuery?: boolean;
  tool_use_result?: unknown;
};
```

将 `shouldQuery` 设置为 `false` 以将消息附加到记录中而不触发助手轮次。消息被保留并合并到下一个触发轮次的用户消息中。使用此方法注入上下文，例如您在带外运行的命令的输出，而无需在其上花费模型调用。

### `SDKUserMessageReplay`

具有必需 UUID 的重放用户消息。

```typescript theme={null}
type SDKUserMessageReplay = {
  type: "user";
  uuid: UUID;
  session_id: string;
  message: MessageParam;
  parent_tool_use_id: string | null;
  isSynthetic?: boolean;
  tool_use_result?: unknown;
  isReplay: true;
};
```

### `SDKResultMessage`

最终结果消息。

```typescript theme={null}
type SDKResultMessage =
  | {
      type: "result";
      subtype: "success";
      uuid: UUID;
      session_id: string;
      duration_ms: number;
      duration_api_ms: number;
      is_error: boolean;
      num_turns: number;
      result: string;
      stop_reason: string | null;
      total_cost_usd: number;
      usage: NonNullableUsage;
      modelUsage: { [modelName: string]: ModelUsage };
      permission_denials: SDKPermissionDenial[];
      structured_output?: unknown;
    }
  | {
      type: "result";
      subtype:
        | "error_max_turns"
        | "error_during_execution"
        | "error_max_budget_usd"
        | "error_max_structured_output_retries";
      uuid: UUID;
      session_id: string;
      duration_ms: number;
      duration_api_ms: number;
      is_error: boolean;
      num_turns: number;
      stop_reason: string | null;
      total_cost_usd: number;
      usage: NonNullableUsage;
      modelUsage: { [modelName: string]: ModelUsage };
      permission_denials: SDKPermissionDenial[];
      errors: string[];
    };
```

### `SDKSystemMessage`

系统初始化消息。

```typescript theme={null}
type SDKSystemMessage = {
  type: "system";
  subtype: "init";
  uuid: UUID;
  session_id: string;
  agents?: string[];
  apiKeySource: ApiKeySource;
  betas?: string[];
  claude_code_version: string;
  cwd: string;
  tools: string[];
  mcp_servers: {
    name: string;
    status: string;
  }[];
  model: string;
  permissionMode: PermissionMode;
  slash_commands: string[];
  output_style: string;
  skills: string[];
  plugins: { name: string; path: string }[];
};
```

### `SDKPartialAssistantMessage`

流式部分消息（仅当 `includePartialMessages` 为 true 时）。

```typescript theme={null}
type SDKPartialAssistantMessage = {
  type: "stream_event";
  event: BetaRawMessageStreamEvent; // 来自 Anthropic SDK
  parent_tool_use_id: string | null;
  uuid: UUID;
  session_id: string;
};
```

### `SDKCompactBoundaryMessage`

指示对话压缩边界的消息。

```typescript theme={null}
type SDKCompactBoundaryMessage = {
  type: "system";
  subtype: "compact_boundary";
  uuid: UUID;
  session_id: string;
  compact_metadata: {
    trigger: "manual" | "auto";
    pre_tokens: number;
  };
};
```

### `SDKPluginInstallMessage`

插件安装进度事件。当设置 [`CLAUDE_CODE_SYNC_PLUGIN_INSTALL`](/zh-CN/env-vars) 时发出，以便您的 Agent SDK 应用程序可以在第一个轮次之前跟踪市场插件安装。`started` 和 `completed` 状态括起整体安装。`installed` 和 `failed` 状态报告单个市场并包括 `name`。

```typescript theme={null}
type SDKPluginInstallMessage = {
  type: "system";
  subtype: "plugin_install";
  status: "started" | "installed" | "failed" | "completed";
  name?: string;
  error?: string;
  uuid: UUID;
  session_id: string;
};
```

### `SDKPermissionDenial`

有关被拒绝的工具使用的信息。

```typescript theme={null}
type SDKPermissionDenial = {
  tool_name: string;
  tool_use_id: string;
  tool_input: Record<string, unknown>;
};
```

## Hook 类型

有关使用 hooks 的综合指南，包括示例和常见模式，请参阅 [Hooks 指南](/zh-CN/agent-sdk/hooks)。

### `HookEvent`

可用的 hook 事件。

```typescript theme={null}
type HookEvent =
  | "PreToolUse"
  | "PostToolUse"
  | "PostToolUseFailure"
  | "Notification"
  | "UserPromptSubmit"
  | "SessionStart"
  | "SessionEnd"
  | "Stop"
  | "SubagentStart"
  | "SubagentStop"
  | "PreCompact"
  | "PermissionRequest"
  | "Setup"
  | "TeammateIdle"
  | "TaskCompleted"
  | "ConfigChange"
  | "WorktreeCreate"
  | "WorktreeRemove";
```

### `HookCallback`

Hook 回调函数类型。

```typescript theme={null}
type HookCallback = (
  input: HookInput, // 所有 hook 输入类型的联合
  toolUseID: string | undefined,
  options: { signal: AbortSignal }
) => Promise<HookJSONOutput>;
```

### `HookCallbackMatcher`

带有可选匹配器的 Hook 配置。

```typescript theme={null}
interface HookCallbackMatcher {
  matcher?: string;
  hooks: HookCallback[];
  timeout?: number; // 此匹配器中所有 hooks 的超时时间（秒）
}
```

### `HookInput`

所有 hook 输入类型的联合类型。

```typescript theme={null}
type HookInput =
  | PreToolUseHookInput
  | PostToolUseHookInput
  | PostToolUseFailureHookInput
  | NotificationHookInput
  | UserPromptSubmitHookInput
  | SessionStartHookInput
  | SessionEndHookInput
  | StopHookInput
  | SubagentStartHookInput
  | SubagentStopHookInput
  | PreCompactHookInput
  | PermissionRequestHookInput
  | SetupHookInput
  | TeammateIdleHookInput
  | TaskCompletedHookInput
  | ConfigChangeHookInput
  | WorktreeCreateHookInput
  | WorktreeRemoveHookInput;
```

### `BaseHookInput`

所有 hook 输入类型扩展的基本接口。

```typescript theme={null}
type BaseHookInput = {
  session_id: string;
  transcript_path: string;
  cwd: string;
  permission_mode?: string;
  agent_id?: string;
  agent_type?: string;
};
```

#### `PreToolUseHookInput`

```typescript theme={null}
type PreToolUseHookInput = BaseHookInput & {
  hook_event_name: "PreToolUse";
  tool_name: string;
  tool_input: unknown;
  tool_use_id: string;
};
```

#### `PostToolUseHookInput`

```typescript theme={null}
type PostToolUseHookInput = BaseHookInput & {
  hook_event_name: "PostToolUse";
  tool_name: string;
  tool_input: unknown;
  tool_response: unknown;
  tool_use_id: string;
};
```

#### `PostToolUseFailureHookInput`

```typescript theme={null}
type PostToolUseFailureHookInput = BaseHookInput & {
  hook_event_name: "PostToolUseFailure";
  tool_name: string;
  tool_input: unknown;
  tool_use_id: string;
  error: string;
  is_interrupt?: boolean;
};
```

#### `NotificationHookInput`

```typescript theme={null}
type NotificationHookInput = BaseHookInput & {
  hook_event_name: "Notification";
  message: string;
  title?: string;
  notification_type: string;
};
```

#### `UserPromptSubmitHookInput`

```typescript theme={null}
type UserPromptSubmitHookInput = BaseHookInput & {
  hook_event_name: "UserPromptSubmit";
  prompt: string;
};
```

#### `SessionStartHookInput`

```typescript theme={null}
type SessionStartHookInput = BaseHookInput & {
  hook_event_name: "SessionStart";
  source: "startup" | "resume" | "clear" | "compact";
  agent_type?: string;
  model?: string;
};
```

#### `SessionEndHookInput`

```typescript theme={null}
type SessionEndHookInput = BaseHookInput & {
  hook_event_name: "SessionEnd";
  reason: ExitReason; // EXIT_REASONS 数组中的字符串
};
```

#### `StopHookInput`

```typescript theme={null}
type StopHookInput = BaseHookInput & {
  hook_event_name: "Stop";
  stop_hook_active: boolean;
  last_assistant_message?: string;
};
```

#### `SubagentStartHookInput`

```typescript theme={null}
type SubagentStartHookInput = BaseHookInput & {
  hook_event_name: "SubagentStart";
  agent_id: string;
  agent_type: string;
};
```

#### `SubagentStopHookInput`

```typescript theme={null}
type SubagentStopHookInput = BaseHookInput & {
  hook_event_name: "SubagentStop";
  stop_hook_active: boolean;
  agent_id: string;
  agent_transcript_path: string;
  agent_type: string;
  last_assistant_message?: string;
};
```

#### `PreCompactHookInput`

```typescript theme={null}
type PreCompactHookInput = BaseHookInput & {
  hook_event_name: "PreCompact";
  trigger: "manual" | "auto";
  custom_instructions: string | null;
};
```

#### `PermissionRequestHookInput`

```typescript theme={null}
type PermissionRequestHookInput = BaseHookInput & {
  hook_event_name: "PermissionRequest";
  tool_name: string;
  tool_input: unknown;
  permission_suggestions?: PermissionUpdate[];
};
```

#### `SetupHookInput`

```typescript theme={null}
type SetupHookInput = BaseHookInput & {
  hook_event_name: "Setup";
  trigger: "init" | "maintenance";
};
```

#### `TeammateIdleHookInput`

```typescript theme={null}
type TeammateIdleHookInput = BaseHookInput & {
  hook_event_name: "TeammateIdle";
  teammate_name: string;
  team_name: string;
};
```

#### `TaskCompletedHookInput`

```typescript theme={null}
type TaskCompletedHookInput = BaseHookInput & {
  hook_event_name: "TaskCompleted";
  task_id: string;
  task_subject: string;
  task_description?: string;
  teammate_name?: string;
  team_name?: string;
};
```

#### `ConfigChangeHookInput`

```typescript theme={null}
type ConfigChangeHookInput = BaseHookInput & {
  hook_event_name: "ConfigChange";
  source:
    | "user_settings"
    | "project_settings"
    | "local_settings"
    | "policy_settings"
    | "skills";
  file_path?: string;
};
```

#### `WorktreeCreateHookInput`

```typescript theme={null}
type WorktreeCreateHookInput = BaseHookInput & {
  hook_event_name: "WorktreeCreate";
  name: string;
};
```

#### `WorktreeRemoveHookInput`

```typescript theme={null}
type WorktreeRemoveHookInput = BaseHookInput & {
  hook_event_name: "WorktreeRemove";
  worktree_path: string;
};
```

### `HookJSONOutput`

Hook 返回值。

```typescript theme={null}
type HookJSONOutput = AsyncHookJSONOutput | SyncHookJSONOutput;
```

#### `AsyncHookJSONOutput`

```typescript theme={null}
type AsyncHookJSONOutput = {
  async: true;
  asyncTimeout?: number;
};
```

#### `SyncHookJSONOutput`

```typescript theme={null}
type SyncHookJSONOutput = {
  continue?: boolean;
  suppressOutput?: boolean;
  stopReason?: string;
  decision?: "approve" | "block";
  systemMessage?: string;
  reason?: string;
  hookSpecificOutput?:
    | {
        hookEventName: "PreToolUse";
        permissionDecision?: "allow" | "deny" | "ask";
        permissionDecisionReason?: string;
        updatedInput?: Record<string, unknown>;
        additionalContext?: string;
      }
    | {
        hookEventName: "UserPromptSubmit";
        additionalContext?: string;
      }
    | {
        hookEventName: "SessionStart";
        additionalContext?: string;
      }
    | {
        hookEventName: "Setup";
        additionalContext?: string;
      }
    | {
        hookEventName: "SubagentStart";
        additionalContext?: string;
      }
    | {
        hookEventName: "PostToolUse";
        additionalContext?: string;
        updatedMCPToolOutput?: unknown;
      }
    | {
        hookEventName: "PostToolUseFailure";
        additionalContext?: string;
      }
    | {
        hookEventName: "Notification";
        additionalContext?: string;
      }
    | {
        hookEventName: "PermissionRequest";
        decision:
          | {
              behavior: "allow";
              updatedInput?: Record<string, unknown>;
              updatedPermissions?: PermissionUpdate[];
            }
          | {
              behavior: "deny";
              message?: string;
              interrupt?: boolean;
            };
      };
};
```

## 工具输入类型

所有内置 Claude Code 工具的输入架构文档。这些类型从 `@anthropic-ai/claude-agent-sdk` 导出，可用于类型安全的工具交互。

### `ToolInputSchemas`

所有工具输入类型的联合，从 `@anthropic-ai/claude-agent-sdk` 导出。

```typescript theme={null}
type ToolInputSchemas =
  | AgentInput
  | AskUserQuestionInput
  | BashInput
  | TaskOutputInput
  | ConfigInput
  | EnterWorktreeInput
  | ExitPlanModeInput
  | FileEditInput
  | FileReadInput
  | FileWriteInput
  | GlobInput
  | GrepInput
  | ListMcpResourcesInput
  | McpInput
  | MonitorInput
  | NotebookEditInput
  | ReadMcpResourceInput
  | SubscribeMcpResourceInput
  | SubscribePollingInput
  | TaskStopInput
  | TodoWriteInput
  | UnsubscribeMcpResourceInput
  | UnsubscribePollingInput
  | WebFetchInput
  | WebSearchInput;
```

### Agent

**工具名称：** `Agent`（之前为 `Task`，仍然接受作为别名）

```typescript theme={null}
type AgentInput = {
  description: string;
  prompt: string;
  subagent_type: string;
  model?: "sonnet" | "opus" | "haiku";
  resume?: string;
  run_in_background?: boolean;
  max_turns?: number;
  name?: string;
  team_name?: string;
  mode?: "acceptEdits" | "bypassPermissions" | "default" | "dontAsk" | "plan";
  isolation?: "worktree";
};
```

启动新代理以自主处理复杂的多步骤任务。

### AskUserQuestion

**工具名称：** `AskUserQuestion`

```typescript theme={null}
type AskUserQuestionInput = {
  questions: Array<{
    question: string;
    header: string;
    options: Array<{ label: string; description: string; preview?: string }>;
    multiSelect: boolean;
  }>;
};
```

在执行期间向用户提出澄清问题。请参阅[处理批准和用户输入](/zh-CN/agent-sdk/user-input#handle-clarifying-questions)了解使用详情。

### Bash

**工具名称：** `Bash`

```typescript theme={null}
type BashInput = {
  command: string;
  timeout?: number;
  description?: string;
  run_in_background?: boolean;
  dangerouslyDisableSandbox?: boolean;
};
```

在持久 shell 会话中执行 bash 命令，支持可选超时和后台执行。

### Monitor

**工具名称：** `Monitor`

```typescript theme={null}
type MonitorInput = {
  command: string;
  description: string;
  timeout_ms?: number;
  persistent?: boolean;
};
```

运行后台脚本并将每个 stdout 行作为事件传递给 Claude，以便它可以做出反应而无需轮询。为会话长度的监视（如日志尾部）设置 `persistent: true`。Monitor 遵循与 Bash 相同的权限规则。请参阅 [Monitor 工具参考](/zh-CN/tools-reference#monitor-tool)了解行为和提供商可用性。

### TaskOutput

**工具名称：** `TaskOutput`

```typescript theme={null}
type TaskOutputInput = {
  task_id: string;
  block: boolean;
  timeout: number;
};
```

从运行中或已完成的后台任务检索输出。

### Edit

**工具名称：** `Edit`

```typescript theme={null}
type FileEditInput = {
  file_path: string;
  old_string: string;
  new_string: string;
  replace_all?: boolean;
};
```

在文件中执行精确字符串替换。

### Read

**工具名称：** `Read`

```typescript theme={null}
type FileReadInput = {
  file_path: string;
  offset?: number;
  limit?: number;
  pages?: string;
};
```

从本地文件系统读取文件，包括文本、图像、PDF 和 Jupyter 笔记本。对 PDF 页面范围使用 `pages`（例如，`"1-5"`）。

### Write

**工具名称：** `Write`

```typescript theme={null}
type FileWriteInput = {
  file_path: string;
  content: string;
};
```

将文件写入本地文件系统，如果存在则覆盖。

### Glob

**工具名称：** `Glob`

```typescript theme={null}
type GlobInput = {
  pattern: string;
  path?: string;
};
```

快速文件模式匹配，适用于任何代码库大小。

### Grep

**工具名称：** `Grep`

```typescript theme={null}
type GrepInput = {
  pattern: string;
  path?: string;
  glob?: string;
  type?: string;
  output_mode?: "content" | "files_with_matches" | "count";
  "-i"?: boolean;
  "-n"?: boolean;
  "-B"?: number;
  "-A"?: number;
  "-C"?: number;
  context?: number;
  head_limit?: number;
  offset?: number;
  multiline?: boolean;
};
```

基于 ripgrep 的强大搜索工具，支持正则表达式。

### TaskStop

**工具名称：** `TaskStop`

```typescript theme={null}
type TaskStopInput = {
  task_id?: string;
  shell_id?: string; // 已弃用：使用 task_id
};
```

按 ID 停止运行的后台任务或 shell。

### NotebookEdit

**工具名称：** `NotebookEdit`

```typescript theme={null}
type NotebookEditInput = {
  notebook_path: string;
  cell_id?: string;
  new_source: string;
  cell_type?: "code" | "markdown";
  edit_mode?: "replace" | "insert" | "delete";
};
```

编辑 Jupyter 笔记本文件中的单元格。

### WebFetch

**工具名称：** `WebFetch`

```typescript theme={null}
type WebFetchInput = {
  url: string;
  prompt: string;
};
```

从 URL 获取内容并使用 AI 模型处理它。

### WebSearch

**工具名称：** `WebSearch`

```typescript theme={null}
type WebSearchInput = {
  query: string;
  allowed_domains?: string[];
  blocked_domains?: string[];
};
```

搜索网络并返回格式化的结果。

### TodoWrite

**工具名称：** `TodoWrite`

```typescript theme={null}
type TodoWriteInput = {
  todos: Array<{
    content: string;
    status: "pending" | "in_progress" | "completed";
    activeForm: string;
  }>;
};
```

创建和管理结构化任务列表以跟踪进度。

### ExitPlanMode

**工具名称：** `ExitPlanMode`

```typescript theme={null}
type ExitPlanModeInput = {
  allowedPrompts?: Array<{
    tool: "Bash";
    prompt: string;
  }>;
};
```

退出规划模式。可选地指定实现计划所需的基于提示的权限。

### ListMcpResources

**工具名称：** `ListMcpResources`

```typescript theme={null}
type ListMcpResourcesInput = {
  server?: string;
};
```

列出来自连接服务器的可用 MCP 资源。

### ReadMcpResource

**工具名称：** `ReadMcpResource`

```typescript theme={null}
type ReadMcpResourceInput = {
  server: string;
  uri: string;
};
```

从服务器读取特定的 MCP 资源。

### Config

**工具名称：** `Config`

```typescript theme={null}
type ConfigInput = {
  setting: string;
  value?: string | boolean | number;
};
```

获取或设置配置值。

### EnterWorktree

**工具名称：** `EnterWorktree`

```typescript theme={null}
type EnterWorktreeInput = {
  name?: string;
  path?: string;
};
```

创建并进入临时 git worktree 以进行隔离工作。传递 `path` 以切换到当前存储库的现有 worktree 而不是创建新的。`name` 和 `path` 互斥。

## 工具输出类型

所有内置 Claude Code 工具的输出架构文档。这些类型从 `@anthropic-ai/claude-agent-sdk` 导出，代表每个工具返回的实际响应数据。

### `ToolOutputSchemas`

所有工具输出类型的联合。

```typescript theme={null}
type ToolOutputSchemas =
  | AgentOutput
  | AskUserQuestionOutput
  | BashOutput
  | ConfigOutput
  | EnterWorktreeOutput
  | ExitPlanModeOutput
  | FileEditOutput
  | FileReadOutput
  | FileWriteOutput
  | GlobOutput
  | GrepOutput
  | ListMcpResourcesOutput
  | MonitorOutput
  | NotebookEditOutput
  | ReadMcpResourceOutput
  | TaskStopOutput
  | TodoWriteOutput
  | WebFetchOutput
  | WebSearchOutput;
```

### Agent

**工具名称：** `Agent`（之前为 `Task`，仍然接受作为别名）

```typescript theme={null}
type AgentOutput =
  | {
      status: "completed";
      agentId: string;
      content: Array<{ type: "text"; text: string }>;
      totalToolUseCount: number;
      totalDurationMs: number;
      totalTokens: number;
      usage: {
        input_tokens: number;
        output_tokens: number;
        cache_creation_input_tokens: number | null;
        cache_read_input_tokens: number | null;
        server_tool_use: {
          web_search_requests: number;
          web_fetch_requests: number;
        } | null;
        service_tier: ("standard" | "priority" | "batch") | null;
        cache_creation: {
          ephemeral_1h_input_tokens: number;
          ephemeral_5m_input_tokens: number;
        } | null;
      };
      prompt: string;
    }
  | {
      status: "async_launched";
      agentId: string;
      description: string;
      prompt: string;
      outputFile: string;
      canReadOutputFile?: boolean;
    }
  | {
      status: "sub_agent_entered";
      description: string;
      message: string;
    };
```

返回来自子代理的结果。在 `status` 字段上进行区分：`"completed"` 表示已完成的任务，`"async_launched"` 表示后台任务，`"sub_agent_entered"` 表示交互式子代理。

### AskUserQuestion

**工具名称：** `AskUserQuestion`

```typescript theme={null}
type AskUserQuestionOutput = {
  questions: Array<{
    question: string;
    header: string;
    options: Array<{ label: string; description: string; preview?: string }>;
    multiSelect: boolean;
  }>;
  answers: Record<string, string>;
};
```

返回提出的问题和用户的答案。

### Bash

**工具名称：** `Bash`

```typescript theme={null}
type BashOutput = {
  stdout: string;
  stderr: string;
  rawOutputPath?: string;
  interrupted: boolean;
  isImage?: boolean;
  backgroundTaskId?: string;
  backgroundedByUser?: boolean;
  dangerouslyDisableSandbox?: boolean;
  returnCodeInterpretation?: string;
  structuredContent?: unknown[];
  persistedOutputPath?: string;
  persistedOutputSize?: number;
};
```

返回命令输出，stdout/stderr 分开。后台命令包括 `backgroundTaskId`。

### Monitor

**工具名称：** `Monitor`

```typescript theme={null}
type MonitorOutput = {
  taskId: string;
  timeoutMs: number;
  persistent?: boolean;
};
```

返回运行监视器的后台任务 ID。使用此 ID 与 `TaskStop` 一起提前取消监视。

### Edit

**工具名称：** `Edit`

```typescript theme={null}
type FileEditOutput = {
  filePath: string;
  oldString: string;
  newString: string;
  originalFile: string;
  structuredPatch: Array<{
    oldStart: number;
    oldLines: number;
    newStart: number;
    newLines: number;
    lines: string[];
  }>;
  userModified: boolean;
  replaceAll: boolean;
  gitDiff?: {
    filename: string;
    status: "modified" | "added";
    additions: number;
    deletions: number;
    changes: number;
    patch: string;
  };
};
```

返回编辑操作的结构化差异。

### Read

**工具名称：** `Read`

```typescript theme={null}
type FileReadOutput =
  | {
      type: "text";
      file: {
        filePath: string;
        content: string;
        numLines: number;
        startLine: number;
        totalLines: number;
      };
    }
  | {
      type: "image";
      file: {
        base64: string;
        type: "image/jpeg" | "image/png" | "image/gif" | "image/webp";
        originalSize: number;
        dimensions?: {
          originalWidth?: number;
          originalHeight?: number;
          displayWidth?: number;
          displayHeight?: number;
        };
      };
    }
  | {
      type: "notebook";
      file: {
        filePath: string;
        cells: unknown[];
      };
    }
  | {
      type: "pdf";
      file: {
        filePath: string;
        base64: string;
        originalSize: number;
      };
    }
  | {
      type: "parts";
      file: {
        filePath: string;
        originalSize: number;
        count: number;
        outputDir: string;
      };
    };
```

返回适合文件类型的格式的文件内容。在 `type` 字段上进行区分。

### Write

**工具名称：** `Write`

```typescript theme={null}
type FileWriteOutput = {
  type: "create" | "update";
  filePath: string;
  content: string;
  structuredPatch: Array<{
    oldStart: number;
    oldLines: number;
    newStart: number;
    newLines: number;
    lines: string[];
  }>;
  originalFile: string | null;
  gitDiff?: {
    filename: string;
    status: "modified" | "added";
    additions: number;
    deletions: number;
    changes: number;
    patch: string;
  };
};
```

返回写入结果，包含结构化差异信息。

### Glob

**工具名称：** `Glob`

```typescript theme={null}
type GlobOutput = {
  durationMs: number;
  numFiles: number;
  filenames: string[];
  truncated: boolean;
};
```

返回与 glob 模式匹配的文件路径，按修改时间排序。

### Grep

**工具名称：** `Grep`

```typescript theme={null}
type GrepOutput = {
  mode?: "content" | "files_with_matches" | "count";
  numFiles: number;
  filenames: string[];
  content?: string;
  numLines?: number;
  numMatches?: number;
  appliedLimit?: number;
  appliedOffset?: number;
};
```

返回搜索结果。形状因 `mode` 而异：文件列表、带匹配的内容或匹配计数。

### TaskStop

**工具名称：** `TaskStop`

```typescript theme={null}
type TaskStopOutput = {
  message: string;
  task_id: string;
  task_type: string;
  command?: string;
};
```

停止后台任务后返回确认。

### NotebookEdit

**工具名称：** `NotebookEdit`

```typescript theme={null}
type NotebookEditOutput = {
  new_source: string;
  cell_id?: string;
  cell_type: "code" | "markdown";
  language: string;
  edit_mode: string;
  error?: string;
  notebook_path: string;
  original_file: string;
  updated_file: string;
};
```

返回笔记本编辑的结果，包含原始和更新的文件内容。

### WebFetch

**工具名称：** `WebFetch`

```typescript theme={null}
type WebFetchOutput = {
  bytes: number;
  code: number;
  codeText: string;
  result: string;
  durationMs: number;
  url: string;
};
```

返回获取的内容，包含 HTTP 状态和元数据。

### WebSearch

**工具名称：** `WebSearch`

```typescript theme={null}
type WebSearchOutput = {
  query: string;
  results: Array<
    | {
        tool_use_id: string;
        content: Array<{ title: string; url: string }>;
      }
    | string
  >;
  durationSeconds: number;
};
```

返回来自网络的搜索结果。

### TodoWrite

**工具名称：** `TodoWrite`

```typescript theme={null}
type TodoWriteOutput = {
  oldTodos: Array<{
    content: string;
    status: "pending" | "in_progress" | "completed";
    activeForm: string;
  }>;
  newTodos: Array<{
    content: string;
    status: "pending" | "in_progress" | "completed";
    activeForm: string;
  }>;
};
```

返回之前和更新的任务列表。

### ExitPlanMode

**工具名称：** `ExitPlanMode`

```typescript theme={null}
type ExitPlanModeOutput = {
  plan: string | null;
  isAgent: boolean;
  filePath?: string;
  hasTaskTool?: boolean;
  awaitingLeaderApproval?: boolean;
  requestId?: string;
};
```

返回退出规划模式后的计划状态。

### ListMcpResources

**工具名称：** `ListMcpResources`

```typescript theme={null}
type ListMcpResourcesOutput = Array<{
  uri: string;
  name: string;
  mimeType?: string;
  description?: string;
  server: string;
}>;
```

返回可用 MCP 资源的数组。

### ReadMcpResource

**工具名称：** `ReadMcpResource`

```typescript theme={null}
type ReadMcpResourceOutput = {
  contents: Array<{
    uri: string;
    mimeType?: string;
    text?: string;
  }>;
};
```

返回请求的 MCP 资源的内容。

### Config

**工具名称：** `Config`

```typescript theme={null}
type ConfigOutput = {
  success: boolean;
  operation?: "get" | "set";
  setting?: string;
  value?: unknown;
  previousValue?: unknown;
  newValue?: unknown;
  error?: string;
};
```

返回配置获取或设置操作的结果。

### EnterWorktree

**工具名称：** `EnterWorktree`

```typescript theme={null}
type EnterWorktreeOutput = {
  worktreePath: string;
  worktreeBranch?: string;
  message: string;
};
```

返回有关 git worktree 的信息。

## 权限类型

### `PermissionUpdate`

用于更新权限的操作。

```typescript theme={null}
type PermissionUpdate =
  | {
      type: "addRules";
      rules: PermissionRuleValue[];
      behavior: PermissionBehavior;
      destination: PermissionUpdateDestination;
    }
  | {
      type: "replaceRules";
      rules: PermissionRuleValue[];
      behavior: PermissionBehavior;
      destination: PermissionUpdateDestination;
    }
  | {
      type: "removeRules";
      rules: PermissionRuleValue[];
      behavior: PermissionBehavior;
      destination: PermissionUpdateDestination;
    }
  | {
      type: "setMode";
      mode: PermissionMode;
      destination: PermissionUpdateDestination;
    }
  | {
      type: "addDirectories";
      directories: string[];
      destination: PermissionUpdateDestination;
    }
  | {
      type: "removeDirectories";
      directories: string[];
      destination: PermissionUpdateDestination;
    };
```

### `PermissionBehavior`

```typescript theme={null}
type PermissionBehavior = "allow" | "deny" | "ask";
```

### `PermissionUpdateDestination`

```typescript theme={null}
type PermissionUpdateDestination =
  | "userSettings" // 全局用户设置
  | "projectSettings" // 每个目录的项目设置
  | "localSettings" // Gitignored 本地设置
  | "session" // 仅当前会话
  | "cliArg"; // CLI 参数
```

### `PermissionRuleValue`

```typescript theme={null}
type PermissionRuleValue = {
  toolName: string;
  ruleContent?: string;
};
```

## 其他类型

### `ApiKeySource`

```typescript theme={null}
type ApiKeySource = "user" | "project" | "org" | "temporary" | "oauth";
```

### `SdkBeta`

可通过 `betas` 选项启用的可用测试功能。请参阅 [Beta 标头](https://platform.claude.com/docs/zh-CN/api/beta-headers)了解更多信息。

```typescript theme={null}
type SdkBeta = "context-1m-2025-08-07";
```

<Warning>
  `context-1m-2025-08-07` beta 自 2026 年 4 月 30 日起已停用。使用 Claude Sonnet 4.5 或 Sonnet 4 传递此值无效，超过标准 200k 令牌上下文窗口的请求返回错误。要使用 1M 令牌上下文窗口，请迁移到 [Claude Sonnet 4.6、Claude Opus 4.6 或 Claude Opus 4.7](https://platform.claude.com/docs/zh-CN/about-claude/models/overview)，它们以标准定价包括 1M 上下文，无需 beta 标头。
</Warning>

### `SlashCommand`

有关可用 slash command 的信息。

```typescript theme={null}
type SlashCommand = {
  name: string;
  description: string;
  argumentHint: string;
};
```

### `ModelInfo`

有关可用模型的信息。

```typescript theme={null}
type ModelInfo = {
  value: string;
  displayName: string;
  description: string;
  supportsEffort?: boolean;
  supportedEffortLevels?: ("low" | "medium" | "high" | "xhigh" | "max")[];
  supportsAdaptiveThinking?: boolean;
  supportsFastMode?: boolean;
};
```

### `AgentInfo`

有关可通过 Agent 工具调用的可用子代理的信息。

```typescript theme={null}
type AgentInfo = {
  name: string;
  description: string;
  model?: string;
};
```

| 字段            | 类型                    | 描述                                          |
| :------------ | :-------------------- | :------------------------------------------ |
| `name`        | `string`              | 代理类型标识符（例如，`"Explore"`、`"general-purpose"`） |
| `description` | `string`              | 何时使用此代理的描述                                  |
| `model`       | `string \| undefined` | 此代理使用的模型别名。如果省略，继承父级的模型                     |

### `McpServerStatus`

连接的 MCP 服务器的状态。

```typescript theme={null}
type McpServerStatus = {
  name: string;
  status: "connected" | "failed" | "needs-auth" | "pending" | "disabled";
  serverInfo?: {
    name: string;
    version: string;
  };
  error?: string;
  config?: McpServerStatusConfig;
  scope?: string;
  tools?: {
    name: string;
    description?: string;
    annotations?: {
      readOnly?: boolean;
      destructive?: boolean;
      openWorld?: boolean;
    };
  }[];
};
```

### `McpServerStatusConfig`

由 `mcpServerStatus()` 报告的 MCP 服务器的配置。这是所有 MCP 服务器传输类型的联合。

```typescript theme={null}
type McpServerStatusConfig =
  | McpStdioServerConfig
  | McpSSEServerConfig
  | McpHttpServerConfig
  | McpSdkServerConfig
  | McpClaudeAIProxyServerConfig;
```

请参阅 [`McpServerConfig`](#mcp-server-config)了解每种传输类型的详情。

### `AccountInfo`

经过身份验证的用户的帐户信息。

```typescript theme={null}
type AccountInfo = {
  email?: string;
  organization?: string;
  subscriptionType?: string;
  tokenSource?: string;
  apiKeySource?: string;
};
```

### `ModelUsage`

结果消息中返回的每个模型使用统计。`costUSD` 值是客户端估计。请参阅[跟踪成本和使用情况](/zh-CN/agent-sdk/cost-tracking)了解计费注意事项。

```typescript theme={null}
type ModelUsage = {
  inputTokens: number;
  outputTokens: number;
  cacheReadInputTokens: number;
  cacheCreationInputTokens: number;
  webSearchRequests: number;
  costUSD: number;
  contextWindow: number;
  maxOutputTokens: number;
};
```

### `ConfigScope`

```typescript theme={null}
type ConfigScope = "local" | "user" | "project";
```

### `NonNullableUsage`

[`Usage`](#usage) 的版本，所有可空字段都变为非可空。

```typescript theme={null}
type NonNullableUsage = {
  [K in keyof Usage]: NonNullable<Usage[K]>;
};
```

### `Usage`

令牌使用统计（来自 `@anthropic-ai/sdk`）。

```typescript theme={null}
type Usage = {
  input_tokens: number | null;
  output_tokens: number | null;
  cache_creation_input_tokens?: number | null;
  cache_read_input_tokens?: number | null;
};
```

### `CallToolResult`

MCP 工具结果类型（来自 `@modelcontextprotocol/sdk/types.js`）。

```typescript theme={null}
type CallToolResult = {
  content: Array<{
    type: "text" | "image" | "resource";
    // 其他字段因类型而异
  }>;
  isError?: boolean;
};
```

### `ThinkingConfig`

控制 Claude 的思考/推理行为。优先于已弃用的 `maxThinkingTokens`。

```typescript theme={null}
type ThinkingConfig =
  | { type: "adaptive" } // 模型确定何时以及多少推理（Opus 4.6+）
  | { type: "enabled"; budgetTokens?: number } // 固定思考令牌预算
  | { type: "disabled" }; // 无扩展思考
```

### `SpawnedProcess`

自定义进程生成的接口（与 `spawnClaudeCodeProcess` 选项一起使用）。`ChildProcess` 已满足此接口。

```typescript theme={null}
interface SpawnedProcess {
  stdin: Writable;
  stdout: Readable;
  readonly killed: boolean;
  readonly exitCode: number | null;
  kill(signal: NodeJS.Signals): boolean;
  on(
    event: "exit",
    listener: (code: number | null, signal: NodeJS.Signals | null) => void
  ): void;
  on(event: "error", listener: (error: Error) => void): void;
  once(
    event: "exit",
    listener: (code: number | null, signal: NodeJS.Signals | null) => void
  ): void;
  once(event: "error", listener: (error: Error) => void): void;
  off(
    event: "exit",
    listener: (code: number | null, signal: NodeJS.Signals | null) => void
  ): void;
  off(event: "error", listener: (error: Error) => void): void;
}
```

### `SpawnOptions`

传递给自定义生成函数的选项。

```typescript theme={null}
interface SpawnOptions {
  command: string;
  args: string[];
  cwd?: string;
  env: Record<string, string | undefined>;
  signal: AbortSignal;
}
```

### `McpSetServersResult`

`setMcpServers()` 操作的结果。

```typescript theme={null}
type McpSetServersResult = {
  added: string[];
  removed: string[];
  errors: Record<string, string>;
};
```

### `RewindFilesResult`

`rewindFiles()` 操作的结果。

```typescript theme={null}
type RewindFilesResult = {
  canRewind: boolean;
  error?: string;
  filesChanged?: string[];
  insertions?: number;
  deletions?: number;
};
```

### `SDKStatusMessage`

状态更新消息（例如，压缩）。

```typescript theme={null}
type SDKStatusMessage = {
  type: "system";
  subtype: "status";
  status: "compacting" | null;
  permissionMode?: PermissionMode;
  uuid: UUID;
  session_id: string;
};
```

### `SDKTaskNotificationMessage`

后台任务完成、失败或停止时的通知。后台任务包括 `run_in_background` Bash 命令、[Monitor](#monitor) 监视和后台子代理。

```typescript theme={null}
type SDKTaskNotificationMessage = {
  type: "system";
  subtype: "task_notification";
  task_id: string;
  tool_use_id?: string;
  status: "completed" | "failed" | "stopped";
  output_file: string;
  summary: string;
  usage?: {
    total_tokens: number;
    tool_uses: number;
    duration_ms: number;
  };
  uuid: UUID;
  session_id: string;
};
```

### `SDKToolUseSummaryMessage`

对话中工具使用的摘要。

```typescript theme={null}
type SDKToolUseSummaryMessage = {
  type: "tool_use_summary";
  summary: string;
  preceding_tool_use_ids: string[];
  uuid: UUID;
  session_id: string;
};
```

### `SDKHookStartedMessage`

当 hook 开始执行时发出。

```typescript theme={null}
type SDKHookStartedMessage = {
  type: "system";
  subtype: "hook_started";
  hook_id: string;
  hook_name: string;
  hook_event: string;
  uuid: UUID;
  session_id: string;
};
```

### `SDKHookProgressMessage`

在 hook 运行时发出，包含 stdout/stderr 输出。

```typescript theme={null}
type SDKHookProgressMessage = {
  type: "system";
  subtype: "hook_progress";
  hook_id: string;
  hook_name: string;
  hook_event: string;
  stdout: string;
  stderr: string;
  output: string;
  uuid: UUID;
  session_id: string;
};
```

### `SDKHookResponseMessage`

当 hook 完成执行时发出。

```typescript theme={null}
type SDKHookResponseMessage = {
  type: "system";
  subtype: "hook_response";
  hook_id: string;
  hook_name: string;
  hook_event: string;
  output: string;
  stdout: string;
  stderr: string;
  exit_code?: number;
  outcome: "success" | "error" | "cancelled";
  uuid: UUID;
  session_id: string;
};
```

### `SDKToolProgressMessage`

在工具执行时定期发出，以指示进度。

```typescript theme={null}
type SDKToolProgressMessage = {
  type: "tool_progress";
  tool_use_id: string;
  tool_name: string;
  parent_tool_use_id: string | null;
  elapsed_time_seconds: number;
  task_id?: string;
  uuid: UUID;
  session_id: string;
};
```

### `SDKAuthStatusMessage`

在身份验证流程中发出。

```typescript theme={null}
type SDKAuthStatusMessage = {
  type: "auth_status";
  isAuthenticating: boolean;
  output: string[];
  error?: string;
  uuid: UUID;
  session_id: string;
};
```

### `SDKTaskStartedMessage`

当后台任务开始时发出。`task_type` 字段对于后台 Bash 命令和 [Monitor](#monitor) 监视为 `"local_bash"`，对于子代理为 `"local_agent"`，或 `"remote_agent"`。

```typescript theme={null}
type SDKTaskStartedMessage = {
  type: "system";
  subtype: "task_started";
  task_id: string;
  tool_use_id?: string;
  description: string;
  task_type?: string;
  uuid: UUID;
  session_id: string;
};
```

### `SDKTaskProgressMessage`

在后台任务运行时定期发出。

```typescript theme={null}
type SDKTaskProgressMessage = {
  type: "system";
  subtype: "task_progress";
  task_id: string;
  tool_use_id?: string;
  description: string;
  usage: {
    total_tokens: number;
    tool_uses: number;
    duration_ms: number;
  };
  last_tool_name?: string;
  uuid: UUID;
  session_id: string;
};
```

### `SDKTaskUpdatedMessage`

当后台任务的状态发生变化时发出，例如当它从 `running` 转换为 `completed` 时。将 `patch` 合并到按 `task_id` 键入的本地任务映射中。`end_time` 字段是 Unix 纪元时间戳（以毫秒为单位），可与 `Date.now()` 比较。

```typescript theme={null}
type SDKTaskUpdatedMessage = {
  type: "system";
  subtype: "task_updated";
  task_id: string;
  patch: {
    status?: "pending" | "running" | "completed" | "failed" | "killed";
    description?: string;
    end_time?: number;
    total_paused_ms?: number;
    error?: string;
    is_backgrounded?: boolean;
  };
  uuid: UUID;
  session_id: string;
};
```

### `SDKFilesPersistedEvent`

当文件检查点持久化到磁盘时发出。

```typescript theme={null}
type SDKFilesPersistedEvent = {
  type: "system";
  subtype: "files_persisted";
  files: { filename: string; file_id: string }[];
  failed: { filename: string; error: string }[];
  processed_at: string;
  uuid: UUID;
  session_id: string;
};
```

### `SDKRateLimitEvent`

当会话遇到速率限制时发出。

```typescript theme={null}
type SDKRateLimitEvent = {
  type: "rate_limit_event";
  rate_limit_info: {
    status: "allowed" | "allowed_warning" | "rejected";
    resetsAt?: number;
    utilization?: number;
  };
  uuid: UUID;
  session_id: string;
};
```

### `SDKLocalCommandOutputMessage`

来自本地 slash command 的输出（例如，`/voice` 或 `/cost`）。在记录中显示为助手样式的文本。

```typescript theme={null}
type SDKLocalCommandOutputMessage = {
  type: "system";
  subtype: "local_command_output";
  content: string;
  uuid: UUID;
  session_id: string;
};
```

### `SDKPromptSuggestionMessage`

当启用 `promptSuggestions` 时在每个轮次后发出。包含预测的下一个用户提示。

```typescript theme={null}
type SDKPromptSuggestionMessage = {
  type: "prompt_suggestion";
  suggestion: string;
  uuid: UUID;
  session_id: string;
};
```

### `AbortError`

用于中止操作的自定义错误类。

```typescript theme={null}
class AbortError extends Error {}
```

## 沙箱配置

### `SandboxSettings`

沙箱行为的配置。使用此选项以编程方式启用命令沙箱和配置网络限制。

```typescript theme={null}
type SandboxSettings = {
  enabled?: boolean;
  autoAllowBashIfSandboxed?: boolean;
  excludedCommands?: string[];
  allowUnsandboxedCommands?: boolean;
  network?: SandboxNetworkConfig;
  filesystem?: SandboxFilesystemConfig;
  ignoreViolations?: Record<string, string[]>;
  enableWeakerNestedSandbox?: boolean;
  ripgrep?: { command: string; args?: string[] };
};
```

| 属性                          | 类型                                                      | 默认值         | 描述                                                                                                                              |
| :-------------------------- | :------------------------------------------------------ | :---------- | :------------------------------------------------------------------------------------------------------------------------------ |
| `enabled`                   | `boolean`                                               | `false`     | 为命令执行启用沙箱模式                                                                                                                     |
| `autoAllowBashIfSandboxed`  | `boolean`                                               | `true`      | 启用沙箱时自动批准 bash 命令                                                                                                               |
| `excludedCommands`          | `string[]`                                              | `[]`        | 始终绕过沙箱限制的命令（例如，`['docker']`）。这些自动运行在沙箱外，无需模型参与                                                                                  |
| `allowUnsandboxedCommands`  | `boolean`                                               | `true`      | 允许模型请求在沙箱外运行命令。当为 `true` 时，模型可以在工具输入中设置 `dangerouslyDisableSandbox`，这会回退到[权限系统](#permissions-fallback-for-unsandboxed-commands) |
| `network`                   | [`SandboxNetworkConfig`](#sandbox-network-config)       | `undefined` | 网络特定的沙箱配置                                                                                                                       |
| `filesystem`                | [`SandboxFilesystemConfig`](#sandbox-filesystem-config) | `undefined` | 用于读/写限制的文件系统特定沙箱配置                                                                                                              |
| `ignoreViolations`          | `Record<string, string[]>`                              | `undefined` | 违规类别到要忽略的模式的映射（例如，`{ file: ['/tmp/*'], network: ['localhost'] }`）                                                               |
| `enableWeakerNestedSandbox` | `boolean`                                               | `false`     | 为兼容性启用较弱的嵌套沙箱                                                                                                                   |
| `ripgrep`                   | `{ command: string; args?: string[] }`                  | `undefined` | 沙箱环境中的自定义 ripgrep 二进制配置                                                                                                         |

#### 示例用法

```typescript theme={null}
import { query } from "@anthropic-ai/claude-agent-sdk";

for await (const message of query({
  prompt: "Build and test my project",
  options: {
    sandbox: {
      enabled: true,
      autoAllowBashIfSandboxed: true,
      network: {
        allowLocalBinding: true
      }
    }
  }
})) {
  if ("result" in message) console.log(message.result);
}
```

<Warning>
  **Unix socket 安全性：** `allowUnixSockets` 选项可以授予对强大系统服务的访问权限。例如，允许 `/var/run/docker.sock` 实际上通过 Docker API 授予对主机系统的完全访问权限，绕过沙箱隔离。仅允许严格必要的 Unix sockets 并了解每个的安全含义。
</Warning>

### `SandboxNetworkConfig`

沙箱模式的网络特定配置。

```typescript theme={null}
type SandboxNetworkConfig = {
  allowedDomains?: string[];
  deniedDomains?: string[];
  allowManagedDomainsOnly?: boolean;
  allowLocalBinding?: boolean;
  allowUnixSockets?: string[];
  allowAllUnixSockets?: boolean;
  httpProxyPort?: number;
  socksProxyPort?: number;
};
```

| 属性                        | 类型         | 默认值         | 描述                                       |
| :------------------------ | :--------- | :---------- | :--------------------------------------- |
| `allowedDomains`          | `string[]` | `[]`        | 沙箱进程可以访问的域名                              |
| `deniedDomains`           | `string[]` | `[]`        | 沙箱进程无法访问的域名。优先于 `allowedDomains`         |
| `allowManagedDomainsOnly` | `boolean`  | `false`     | 将网络访问限制为仅 `allowedDomains` 中的域           |
| `allowLocalBinding`       | `boolean`  | `false`     | 允许进程绑定到本地端口（例如，用于开发服务器）                  |
| `allowUnixSockets`        | `string[]` | `[]`        | 进程可以访问的 Unix socket 路径（例如，Docker socket） |
| `allowAllUnixSockets`     | `boolean`  | `false`     | 允许访问所有 Unix sockets                      |
| `httpProxyPort`           | `number`   | `undefined` | 网络请求的 HTTP 代理端口                          |
| `socksProxyPort`          | `number`   | `undefined` | 网络请求的 SOCKS 代理端口                         |

### `SandboxFilesystemConfig`

沙箱模式的文件系统特定配置。

```typescript theme={null}
type SandboxFilesystemConfig = {
  allowWrite?: string[];
  denyWrite?: string[];
  denyRead?: string[];
};
```

| 属性           | 类型         | 默认值  | 描述            |
| :----------- | :--------- | :--- | :------------ |
| `allowWrite` | `string[]` | `[]` | 允许写入访问的文件路径模式 |
| `denyWrite`  | `string[]` | `[]` | 拒绝写入访问的文件路径模式 |
| `denyRead`   | `string[]` | `[]` | 拒绝读取访问的文件路径模式 |

### 沙箱外命令的权限回退

启用 `allowUnsandboxedCommands` 时，模型可以通过在工具输入中设置 `dangerouslyDisableSandbox: true` 来请求在沙箱外运行命令。这些请求回退到现有权限系统，意味着您的 `canUseTool` 处理程序被调用，允许您实现自定义授权逻辑。

<Note>
  **`excludedCommands` vs `allowUnsandboxedCommands`：**

  * `excludedCommands`：始终自动绕过沙箱的命令的静态列表（例如，`['docker']`）。模型对此无法控制。
  * `allowUnsandboxedCommands`：让模型在运行时通过在工具输入中设置 `dangerouslyDisableSandbox: true` 来决定是否请求沙箱外执行。
</Note>

```typescript theme={null}
import { query } from "@anthropic-ai/claude-agent-sdk";

for await (const message of query({
  prompt: "Deploy my application",
  options: {
    sandbox: {
      enabled: true,
      allowUnsandboxedCommands: true // 模型可以请求沙箱外执行
    },
    permissionMode: "default",
    canUseTool: async (tool, input) => {
      // 检查模型是否请求绕过沙箱
      if (tool === "Bash" && input.dangerouslyDisableSandbox) {
        // 模型请求在沙箱外运行此命令
        console.log(`Unsandboxed command requested: ${input.command}`);

        if (isCommandAuthorized(input.command)) {
          return { behavior: "allow" as const, updatedInput: input };
        }
        return {
          behavior: "deny" as const,
          message: "Command not authorized for unsandboxed execution"
        };
      }
      return { behavior: "allow" as const, updatedInput: input };
    }
  }
})) {
  if ("result" in message) console.log(message.result);
}
```

此模式使您能够：

* **审计模型请求：** 记录模型何时请求沙箱外执行
* **实现允许列表：** 仅允许特定命令在沙箱外运行
* **添加批准工作流：** 需要对特权操作进行明确授权

<Warning>
  使用 `dangerouslyDisableSandbox: true` 运行的命令具有完整的系统访问权限。确保您的 `canUseTool` 处理程序仔细验证这些请求。

  如果 `permissionMode` 设置为 `bypassPermissions` 且 `allowUnsandboxedCommands` 启用，模型可以自主执行沙箱外的命令，无需任何批准提示。此组合实际上允许模型以静默方式逃离沙箱隔离。
</Warning>

## 另请参阅

* [SDK 概述](/zh-CN/agent-sdk/overview) - 常规 SDK 概念
* [Python SDK 参考](/zh-CN/agent-sdk/python) - Python SDK 文档
* [CLI 参考](/zh-CN/cli-reference) - 命令行界面
* [常见工作流](/zh-CN/common-workflows) - 分步指南
