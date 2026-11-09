# Codex 真实隔离永久删除验收

基线：`25c37ac1b7ea2b6f63884a063da324e5e4f1562f`，`feat/v0.27.0`。只用现有安装 CLI 和任务新建数据，不使用真实会话/配置/凭据、模型或账号；不发布/推送/main/安装。

- [x] 核对 HEAD、跟踪区和旧材料；Claude/OpenCode 保持禁用，不重做阶段9实验。
- [x] 初始0.166源码后按实际version0.167固定086ccc来源做有界差异核查；exe固定SHA和369062704精确长度，系统配置不受ProgramData env重定向。
- [x] 元数据证明系统Codex namespace和HKLM policy key不存在，未读配置内容或registry values。
- [x] 独立审查A隔离合同：全新根/独立sqlite_home/空认证与插件/项目发现边界、Job和容器保护；pending等待保持宿主，只有settled释放，B另审。
- [x] 真实version0.167.0和剩余3项help/strict配置probe完成，前后metadata/Job/reap通过；旧大小与版本预期失败保留。配置probe不当作物理DB或服务删除验收。
- [x] 仅合成metadata创建原生归档记录；101真实initialize/read、B002真实Service→CLI delete均已通过，原件单链接/FileID/字节和删后明确NotFound，三个控制保留。
- [x] 真实B002验证Service双ack、换源/配置/修订、重复及只读结果；新fixture运行，未重放B001 unknown。
- [x] 007合成锁RED→GREEN，102在新任务源取得严格未成功确认/原件与controls保留观察；dup/inspect不重放，未解锁重发，不冒称真实模型writer或具体RPC错误。
- [ ] 真实Wry鉴权document、可见Vue双确认和安装版WebView本阶段未跑；已跑的3文件24项前端为合成桥，不能替代。
- [ ] child/reference/部分rollout失败的真实CLI矩阵未跑；当前只有单根无子树的原生验收，不声称完整范围或全关联数据清理。
- [x] 已发现删除helper初始化可允许自动登录：003行为RED→显式capability修复→7域GREEN；其余实测问题继续逐项闭环。
- [x] 记录Claude/OpenCode具体维护设计：`docs/provider-delete-maintenance-design-0.27.md`，源/活动准入、正常生命周期、崩溃/部分失败、新协议或用户动作及验证入口；不写成实现。
- [x] 新只读101定位精确官方状态通知；通知与阶段分类最小修复有RED→10项Rust/24项前端GREEN、独审/Clippy/typecheck，已本地提交515b75bb0488d69e04ba90f6f4051de4668863e8。
- [x] 保存最终文件摘要和完整结果索引，验收运行器/维护设计/文档独立本地提交收口；不将未跑矩阵改成通过。
- [x] Fast今日限定要求已核时钟与工具：无当前线程service tier/到期控制，未启用，未改模型/全局或被测CLI。

吞吐检查点：先完成隔离与最小probe，再一次编译受影响测试，显式运行live入口；若必须改源才重编/重测，不以16片替代真实CLI验收。根负责官方证据与审计，唯一Rust worker负责新live fixture/test，唯一只读审查者核查边界。所有新增fixture保留供核实，不操作旧材料。
