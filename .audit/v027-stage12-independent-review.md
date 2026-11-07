# 0.27 独立工作盘点

2026-11-07从 `feat/v0.27.0` 的 `218979d05b499cb874d1aa4e7796e67806a12486` 继续。起点受跟踪区干净，原有未跟踪材料保留。本轮只读源码、仓库状态和已有证据；没有查询进程、打开运行句柄、读取活动日志或原测试目录，没有执行观察脚本、窗口、CLI、Cargo或安装流程。以下生命周期事实来自已有工具回执，不是本轮实时观察。

## 已有事实

- f8d8dd16刷新修复已有前端5文件61项、typecheck和Vite构建证据；31cbc156目录修复已有固定1.98.1的lib-only check、Clippy、fmt及独审，没有真实UDF绿色回归。
- 阶段11唯一NoNative失败，确认、checkpoint、inspect和capture均为0。原 [审计](v027-stage11-webview.md)、[冻结来源](v027-stage11-source.json)、[49项归档](v027-stage11-raw-evidence.zip) 不修改。
- session71914未获Child OS reap、Job empty、浏览器归属及完整退出回执。上次已知父宿主PID52908、创建时间 `2026-11-06T16:41:05.3893493Z`。本轮未重新确认其是否存在，不能据日期或空列表推断收口。
- 旧session11956因日志写入权限拒绝而未得到原生阶段回执，原生查询是否执行仍不确定。
- 后续另行授权的一次stdout-only观察session11079明确exit1，阶段字段为 `OPEN_EXACT_PARENT:5`。拒绝发生在OpenProcess，没有取得父进程句柄，没有执行创建时间复核、PSS或Child/Job查询。它不证明PID不存在，不等于API不支持，也没有消除11956的不确定性。此处归档已有工具stdout的事实，不冒充新取得的原始文件日志。
- 阶段10真实Codex 0.167.0单根CLI后端结果与B001 unknown保持原样，未重放。

## 独立工作与阻塞

核对更新store、UpdateSection、updatePreferences、updateDiagnostics、useCliVersionDiagnostics、AboutSection及其已有测试和P0/P1清单。三通道、单候选忽略、更新记录、重新检查后重试、所选profile版本探测和脱敏复制已有实现。历史目录、签名准备、普通历史安装与严格九包返回各有既定边界。

本次有界盘点未发现当前可独立完成的编码缺项。这不是全仓审计或整版P0通过证明。本轮实际修改是四份主文档同步阶段11状态，修正旧“当前无运行中验证”表述，并保存精确解阻清单。

| 剩余工作 | 缺少的条件与最小输入 |
| --- | --- |
| document IPC与删除UI | 原实例生命周期未证，当前token打开父进程被拒，setter后真实UDF未验收。需要原执行控制面已有的Child/Job/browser收口回执，或协调方另行限定并获准的处置方式；不重试旧观察、不换接口获取受拒数据、不扩大权限。收口、目录链证明和审查完成前不新建窗口。 |
| 真实签名通道与stable→RC→stable | 需要单独授权的正式0.27候选及精确main CI/coverage、官方签名/来源证据、可销毁Windows环境和审核过的上一stable包。当前不发布、索取签名秘密或改用户安装。 |
| Claude永久删除 | 现有SDK映射与文件锁不能封闭动态namespace、旧DELETE句柄等竞态。需要可验证的来源、活动维护及失败恢复合同，不能用生产ACL、硬链接副本或隐藏替代原生删除。 |
| OpenCode永久删除 | durable claim不是互斥租约，缺可靠持久服务/DB来源及执行期间拒忙保证。需要既有授权服务绑定、活动和并发启动/恢复控制合同，不能用loopback或自报路径替代。 |
| Unix、安装版、D20与现场 | WSL枚举曾被拒，平台、测试账号或现场输入未具备。需要获准且可证明隔离的平台和测试输入；不换新环境绕过拒绝，不借用户凭据或真实历史做试验。 |

## 本轮校验

只校验本地链接、提及Git对象存在性、whitespace和冻结审计材料完整性。内联Node检查5份文档的51个本地链接、使用 `git cat-file -e` 核对25个Git对象，均通过；`git diff --check`退出0。SHA256核对阶段11来源索引、归档、源码、原todo、旧补丁及阶段10来源记录，均与冻结值一致。源码未变，不重跑Vitest、Rust16片、WebView或CLI，旧测试不记为本轮通过。

本轮使用已有pstack的poteto-mode、unslop、technical-writing和deslop指导事实区分与最小文档改动。technical-writing的包入口不可用时，只读找到已安装SKILL.md并读取，没有安装或修改技能。没有新子任务、模型切换或全局配置变更；没有当前线程Standard设置回执，不宣称切换成功。
