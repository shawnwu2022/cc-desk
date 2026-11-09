# 0.27 候选材料及只读预览复核

开始 HEAD `3f60853a70ccf279c500c99d3cebb424e282f06a`，分支 `feat/v0.27.0`；只改文档，已测实现仍为 `68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3`。原 todo、全部 raw log、两轮编译包与隔离 CLI 数据保持不动。未同步 main，未运行真实 CLI/server、归档/删除、用户安装、WSL 重试或发布操作。

## 实际工作

- 阅读当前 acceptance/development/blocker 文档并核对历史面板、store/API、policy/catalog、ordinary admission、payload admission、manager 说明及既有测试。纠正“九包”可能覆盖所有历史安装的含混表述：普通签名历史安装与九包严格返回是两条能力；指南无安装或快照副作用，不代替恢复实现。对应 Rust `HistoryCatalog_DynamicSignedNsisMetadata_016` 和 `HistoryOrdinary_SignedIdentityWithoutVersionTable_001` 的已保存执行行均为 `ok`，未重跑。
- 新增 `docs/candidate-review-0.27.md`：准确变更、完整 P0/P1、提交索引、固定工具链/skip/host/原失败、候选审阅条件与不得发布条件。
- 根与独立只读审查者窄读固定 Codex `6ff1e1ca3b30fc8b914720681fcf5fe62e95f856` 的 thread list 协议、DB 分页/关系查询、ThreadManager 集合，以及 OpenCode `9bbb8ce18c80f099e0e448775f59d965ead40a21` 的 API/list/store/active/server-info/DB bootstrap。全部通过公开 GitHub 读取，未 clone/安装环境或读取真实会话。链接与行号保留在 `docs/native-cleanup-readonly-preview-0.27.md`。
- 结论分别记录：Codex 持久集合缺原生 live 并集；OpenCode 单次有界查询存在可证明只读方向，但当前缺确切 DB/授权域、无启动副作用的读取绑定。活动结果只代表该服务，不能推定全部实例 idle。读取契约与执行 expected 集合/活动拒绝原子条件分开，当前完整预览和执行均不交付。
- 纠正旧文档对 OpenCode v2 的含混判断：当前已支持准确 DB 探测、channel 来源验证、`session_v2` 优先；真实剩余是根历史过滤/关系 DTO 和完整同源读取通道。

## 对应验证

只做 Markdown 本地引用存在性、关键源码/测试路径存在性、事实一致性与 `git diff --check`，核对 `git diff 68fa6b3… -- src src-tauri scripts tests package.json package-lock.json .github` 为空。没有新增实现或测试；不重跑已通过的 Rust/Vitest/Node/构建/CSP/界面套件。最终文档复核与提交 SHA 以本回合交付结果为准。

独立只读审查继续由原 `cleanup_safety_design_review` 执行，实际配置沿用 `gpt-6.1-sol/high`。未派新实现 worker，未安装技能。此次继续采用已有 pstack 的边界/证据组织、superpowers 的审查与完成前验证原则，纯文档不制造镜像测试或虚假 TDD。

独立复核确认 ordinary/九包范围和只读能力分解没有新的确定性事实问题；发现旧 blocker 中四处固定 commit 行号过时。根按 superpowers `receiving-code-review` 逐项回读原源码后修正为 Codex prepare removal `#L1165`、OpenCode delete protocol `#L245`、handler `#L186`、remove `#L391`；原结论不变。同时明确旧固定边界的“活动/未知时拒绝”只针对清理预约/执行，不禁止已授权的只读观察。

文档一致性检查退出 0，日志 `.audit/v027-stage4-doc-check.log`：8 份文档、27 个本地 Markdown 链接、13 个代码/测试证据路径均存在，候选材料九包列表与生产 `REVIEWED` 表一致。`git diff --check` 通过；`git diff 68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3 -- src src-tauri scripts tests package.json package-lock.json .github` 为空。检查由单次 Node stdin 脚本执行，读取文档/源码并调用只读 Git 校验，不导入或执行应用。应用套件未重跑。后续只更新此检查记录及 ledger，并保存文档提交。
