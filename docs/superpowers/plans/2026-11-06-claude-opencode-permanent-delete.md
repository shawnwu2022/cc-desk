# Claude / OpenCode 永久删除后续切片 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不改动真实数据的前提下，证明并实现能安全开放的提供方窄删除；不能证明时给可重复反例和最小缺口，不用隐藏替代。

**Architecture:** 保留3bdc9674基线的Codex预览/双确认/账本。Claude先做真实Windows对象和固定SDK实验，只有来源和writer保护均成立才扩共同协议。OpenCode先核实现有服务生命周期和持久活动claim，不为删除新建服务。

**Tech Stack:** Rust1.98.1、Windows文件/目录API、固定Claude SDK0.3.301、Node24内置SQLite和受控原生函数实验。

**Spec:** `docs/permanent-delete-0.27.md` 及最新用户“真正永久删除、二次确认”约束。

## Global Constraints

- 只用任务合成隔离源，禁止删除/扫描真实用户会话、访问真实服务或凭据。
- 不push/main/Release，不改Codex已测合同，不猜目录、purge或直接删除真实DB。
- 一名Rust实现worker，一名独立只读审查者；根负责共同协议及OpenCode研究/实验。
- 二次确认不是来源/活动保护的替代。未知先只读核查，不自动重放。
- 固定1.98.1、CI dev/test debug=0；按实际影响测试，不重复旧全套。

## Review Focus

- 目录sharing是否实际阻止新子项，而非只约束目录对象本身。
- SDK unlink后同名重建、旧DELETE/目录handle是否绕过保护。
- 硬链接删除是否只删除副本；junction/权限是否确实映射原始名字。
- OpenCode进程内active与持久claim的区别，以及删除中断/动态递归副作用。
- 任何DACL/服务保留方案是否扩大产品权限或需要崩溃恢复，不能悄悄纳入生产。

## Task 1: Claude Windows 来源与保护实验

Files: worker专属 `src-tauri/src/tests/claude_delete_guards.rs`，按现行test模块注册；不改共同生产删除协议。

- [x] 写并执行已有writer、新writer、新child、同名替换、旧directory/DELETE handle、reparse/源替换、部分删除实验。
- [x] 调用固定SDK在真实合成原件上删除，验证private junction和Node权限；保留失败与观测日志。
- [x] 保护假设已被反例推翻，保留局部可行来源映射与创建保护事实；当前不接生产，不以fixture DACL承诺真实会话静止或通用恢复。
- [x] 独立审查与固定工具链受影响验证：最终10项合成观察、all-target Clippy/fmt通过；根只提交测试与证据，生产能力保持关闭。

## Task 2: OpenCode 现有服务与活动契约

Files: 根负责 `.audit/v027-stage9-opencode-*.mjs` 和证据；只有真实可用合同成立才修改共同服务/API。

- [x] 检查Desk实际owned server保存/销毁点及官方CLI删除命令是否复用既有服务。
- [x] 核实SessionExecution.claim/release、scope和delete实际调用，不把持久claim当互斥租约。
- [x] 以固定MIT许可的实际remove函数体和隔离SQLite/注入服务做原生算法实验，明确不等于完整OpenCode HTTP/CLI验收。
- [x] 当前没有可直接复用的可靠现有服务链；记录5项源/活动算法观察及具体维护准入条件，不构造伪issuer、不为删除新建服务。

## Task 3: 整合与交付

- [x] 审查共同双确认/账本仍保留；只开放已证明能力。
- [x] 测试、格式、Clippy和文档一致性；保留原始失败，不以GREEN覆盖。完整结果见 `.audit/v027-stage9-provider-delete.md`。
- [x] 自动本地commit；报告每个提供方的实际能力、未交付和解阻条件。提交限定本轮实验/测试与材料，不等同两家产品删除交付。
