# Native CLI 终端协议来源与输入调度决策

状态：**BLOCKED（真实 CLI / WebView 协议认证尚未执行）**  
适用依赖基线：`@xterm/xterm 5.5.x`（仓库当前声明 `^5.5.0`）  
关联计划：W0/D04、W4、W5、W6；用例 NATIVE-29、30、41、44、47。

## 1. 决策

CC Desk 必须在事件产生处区分两类输入：

1. **用户意图**：键盘、IME、paste、系统菜单、附件动作；
2. **protocol reply**：由受信任终端 parser / xterm API 因 DSR、DA 或已协商协议查询产生的回复。

**禁止按字节正则猜测来源。** 普通用户文本完全可以包含看似 DSR/DA、CSI、OSC 或其他控制序列的字节；根据字节内容将其提升为 protocol reply 会破坏用户输入顺序和信任边界。

在 `pending clipboard` 屏障存在时：

- 已经能由受信任 parser 回调明确标记来源的 protocol reply，可以在 paste frame 尚未开始写入时使用独立的协议回复通道；
- 一旦一个 paste frame 开始进入 writer，任何用户输入或 protocol reply 都不得插入该 frame；
- 若当前锁定版本无法可靠证明回复来源，就保持该组合 **BLOCKED**，先验证或升级通用终端宿主，不用字节模式匹配规避问题；
- pending clipboard 失败不得自动释放后续 Enter，也不得把回复或正文改投新 run。

## 2. 为什么不能统一排队

将所有 protocol reply 都放在异步 clipboard 屏障之后，可能令 CLI 的终端查询超时或卡住。反过来，让任意看起来像控制序列的字节越过屏障，会允许用户输入乱序。

因此调度条件同时要求：

```text
trusted source identity
+ current run / generation
+ current terminal mode epoch
+ paste frame 尚未开始
```

缺少任一条件时，不作推断。

## 3. xterm 版本边界

当前仓库依赖 `@xterm/xterm 5.5`。在线最新文档不能证明锁定版本的具体回调、协议或 onBinary 行为；D04 只记录需要验证的来源路径，不把它标为已经通过。

W6 必须用实际锁文件版本核验：

- DSR / DA 请求和回复；
- focus in/out；
- mouse reporting；
- negotiated keyboard protocol；
- alternate screen；
- synchronized output；
- `onData` 与 `onBinary` 的来源及字节语义；
- CLI → editor → CLI 的模式切换。

## 4. 实现约束

- 用户输入序号在动作发生时分配；
- protocol reply 使用独立、内部来源标记，前端不得用任意字符串伪造；
- 后端核验 owner window、runId、generation 和 mode epoch；
- protocol reply 不计作用户 paste，也不触发剪贴板读取；
- 终端屏幕文本、标题或 ANSI 内容不得用于推断模型状态或授权；
- 无法证明来源时，不默认关闭 alternate screen、鼠标或原生按键来换取测试通过。

## 5. 证据状态

| 证据 | 状态 | 说明 |
|---|---|---|
| 设计决策与失败条件 | PASS | 本文锁定来源与调度边界 |
| `@xterm/xterm 5.5` 实际 parser 回调验证 | NOT_RUN | W6 实机执行 |
| Claude Code 实际终端查询 | NOT_RUN | 固定版本 + 系统终端对照 |
| Codex CLI 实际终端查询 | NOT_RUN | 固定版本 + 系统终端对照 |
| pending clipboard 与 protocol reply 并发 | NOT_RUN | W5/W6 故障注入 |

资料：

- xterm Terminal API：`https://xtermjs.org/docs/api/terminal/classes/terminal/`
- xterm 协议支持表：`https://xtermjs.org/docs/api/vtfeatures/`
- xterm 流控说明：`https://xtermjs.org/docs/guides/flowcontrol/`
