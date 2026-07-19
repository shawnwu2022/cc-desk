# CC-Box 项目愿景（fork 后）

> 本文件是 fork 后的真实定位锚，是所有后续功能决策的 source of truth。
> `CLAUDE.md` / `docs/roadmap.md` 为原作者（orczh-hj）版，降级为参考。

## 一句话定位

接在 cc-switch 下游，让重度多项目玩家同时驾驭多个原生 Claude Code 会话的指挥台。

## 生态分工

| 工具 | 职责 | 层次 |
|------|------|------|
| **cc-switch** | 管理 cc：provider / API Key / 配置切换 | 配置层 |
| **cc-box** | 增强 cc 的管理和能力：多会话 / 终端 / hook 监控 / 信息可视化 | 运行时层 |

cc-box 不管 provider，只消费 cc-switch 设好的配置启动原生 Claude CLI。

## 目标用户

重度、多项目并行、高强度使用、且要求保持原生 Claude Code 完整能力的玩家。

## 核心价值

让 N 个原生 Claude Code 会话同时跑，提供 CLI 天生做不了的跨会话 / 全局视野 / 可视化增强。

## 两条硬约束

1. **保持原生 CC** —— CLI 全部能力（对话、slash 命令、快捷键、模型切换）原样可用，GUI 只增强不替代；Claude Code 升级后 cc-box 不能坏。
2. **不做 switch 能力** —— provider / API Key / 配置切换交给 cc-switch。

## 设计原则

- **开箱即用、零配置优先** —— 任何需要用户手动配置才能用的功能（如模型定价表），除非有自动获取来源，否则降级或放弃。
- **性能与可用性优先于功能堆砌** —— 启动快、多会话切换流畅、解析不卡；少配置、好上手。
- **功能边界测试** —— 任何新功能必须通过「CLI 在单会话里做不好或做不到吗？」才做。做得到 → 不做（CLI 已经够）；做不到（跨会话 / 全局 / 可视化）→ 做。

## 退役 / 搁置

- **退役**：原作者 roadmap 的配置管理 6 阶段（MCP/Skills/Agents/Plugins CRUD）——与 cc-switch 重叠，非核心价值。
- **搁置**：现有 provider 管理模块（`providers.rs` / `providerPresets.ts` / 设置 UI）——先不动，将来瘦身成「model→单价」定价元数据表或整体退役。

## 当前主线

B 方向：多会话指挥中心。

- **第一刀：多项目焦点队列**（B1 收敛形态，经 codex 对抗审查修正）——常驻区域只列**未确认**的关注项「错误 > 等权限 > 新完成」（严重度排序，一键跳转），工作中/空闲只显总数。不复用 `pending`（那是注意力确认位非业务状态），引入独立 `AttentionItem{kind,createdAt,acknowledgedAt}`，`working/pending` 退为兼容派生值。「完成」只由 idle_prompt 判定（Stop 不直接生成 completed——stop hook 可能阻止停止让 Claude 继续，reporter 不知晓最终决策；SessionStart 也归 idle，用 state 会误报）。范围限定单进程（多窗口聚合需 broker，延后）。**不走系统 toast 通知**（重度用户事件量大，toast 刷屏打扰）。
- 待办：成本 / token 看板（token 计数版，零配置）。
