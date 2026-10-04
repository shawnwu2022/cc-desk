# 云端接续验证（2026-10-04）

## 源码与环境

从本机交接提交 `277d2bedb02da9f047a707a7ca8b0abb7afc907b` fast-forward
接续 `feat/unified-workspace-ux`；同步后工作树干净。没有使用、切换或重置本机
`main`，也没有替换本机安装。原始交接与历史证据见
[本机到云端交接](local-to-cloud-handoff-2026-10-04.md)。

云端 Linux 提供 Node 24.19.0/npm 11.9.0；没有 Rust 或 PowerShell。
以下结果不代表 Windows 原生执行、真实 CLI 或历史版本往返验收。

## 交接提交的本地检查

- `npm test`：147 个文件，1881 项通过
- `npm run typecheck`：通过
- `npm run build`：通过，包含普通应用与独立 manager 入口
- `npm run test:visual:unit`：2 个文件，37 项通过
- 历史签名、payload fixture、回转构建/driver、发布策略与 ConPTY 的 Node
  契约：30 项通过、2 项跳过。跳过的是 PowerShell wrapper 和可执行 dry-run
  契约；不能将源码检查记为已执行 PowerShell
- 完整 `node --test tests/scripts/*.node.mjs tests/scripts/*.node.cjs` 初次
  检查：42 项通过、15 项失败、2 项跳过。失败来自旧粘贴验收/trace harness
  不能加载生产代码新增的 `@/terminal/inputPolicy` 依赖，未进入完整语义断言

## 窄范围测试工具修复

修复仅涉及独立 Node 源码加载和测试 fixture：加载真实剪贴板策略及 API
依赖，保留 Tauri IPC 测试边界和全部既有字节/顺序/错误断言，不改变生产
粘贴处理或 Windows 进程保护。普通前端 CI 的 Node 步骤增加这三组已有
回归，避免只有旧专用工作流才执行它们。

修复后重新检查：完整 Node 套件60项，58通过、0失败、2跳过；跳过项
仍是上述两项 PowerShell 契约。三组粘贴套件独立运行28项全部通过。
完整前端1881项、typecheck、build、独立视觉 fixture 37项、四个修改的
CommonJS文件语法检查及 `git diff --check` 均通过。所有既有粘贴断言
保留；新增用例检查实际生产策略对“无图片证据”和“有图片证据”的分流。

这些是 Linux 上的源码/测试工具结果，不代表真实CLI编辑器输入、
Windows Node进程执行、PowerShell或Rust。最终推送后的远端检查须按
准确提交另外核对。

## 交接提交的远端终态

以下绑定源码 `277d2bedb02da9f047a707a7ca8b0abb7afc907b`，PR测试合并
提交 `97bd89c18dd99a47ab72bbd74eb707b0c1e5dc1a`，不代表后续修复提交的CI。

- [D12](https://github.com/shawnwu2022/cc-desk/actions/runs/37193612635)、
  [D13](https://github.com/shawnwu2022/cc-desk/actions/runs/37193612620)、
  [历史恢复专项](https://github.com/shawnwu2022/cc-desk/actions/runs/37193612631)、
  [历史包证据](https://github.com/shawnwu2022/cc-desk/actions/runs/37193612656)通过
- [视觉工作流](https://github.com/shawnwu2022/cc-desk/actions/runs/37193612606)
  通过，202个渲染/交互用例；13张已批准workspace基线没有更新，新历史/manager
  截图仍未批准。Linux Chromium不是原生Windows WebView2验收
- [Windows测试包](https://github.com/shawnwu2022/cc-desk/actions/runs/37193612668)
  的installer与formatting任务通过，包含安装、重装、迁移位置及拒绝损坏运行时
  的检查。这不是历史版本往返
- [普通CI](https://github.com/shawnwu2022/cc-desk/actions/runs/37193612651)失败。
  前端与无原生验收的编译策略任务通过，后者实际执行了PowerShell解析与
  无副作用driver契约。Rust独立门禁0通过/1失败；其余测试1173通过/11失败/
  31忽略/1过滤；Channel单项复核1通过，不重复计入总数。格式和严格lint通过，
  该工作流的应用loader检查跳过

12个不同失败名称与 `aa6c170` 完全一致，均为
`PermissionDenied: source host is job-contained`；独立门禁报告
`source_in_job=true`、`source_job_limits=8192`。未出现新增失败测试，也没有
将这些拒绝改成成功或绕过外层Job。新测试工具提交等本批全部终态后才推送，
不主动取消这批证据。

## 需求核对边界

[历史版本计划](superpowers/plans/2026-10-02-historical-versions.md) 已增加
源码实现与实际验收的分层状态。目录支持九个已观察版本，但当前仅0.17.7
有测量后的 payload policy；生产 `SUPPORTED_ROUNDTRIP_ENABLED=false`。

独立 Windows 正常/注入失败回转、最终原生重开、其他历史版本矩阵、
真实 CLI Layer-C 和 Windows 缩放/可访问性仍未完成。封存且未领取的
检查点返回不等于通用崩溃恢复。新历史/manager PNG 是未批准的合成
状态截图；现有13张workspace基线与绿色几何/交互检查不能替代它们的
像素验收。没有创建 Release/tag、合并 PR 或开启更新发布。

## 后续源码：九版本包证据与不确定回执提示

基于 `9b94b8d2425d81fa35debadc0e6fc5a6f05b1e94` 的后续修订只扩展
专用测试证据能力，并修正一处不确定回执提示：

- 实际检查该提交视觉产物中的94张新截图（26历史、68manager）后，发现
  unknown回执的泛化横幅仍建议“刷新后重试”。现在未知结果使用现有
  “先刷新状态”提示，已交接请求继续说明核查所有权；保留已知诊断、目录
  读取重试和所有禁止重放的控件/状态约束。新增8项双语组合回归
- manager增加8处实际关闭窗口/所有权警告的完整可见性断言与滚动截图点。
  Linux本地Chromium在页面打开前受socket权限限制，未生成这些新增截图；
  必须等待CI渲染和实际像素复核，仍未批准基线
- 历史payload证据fixture改为编译期固定9版本目录。发布元组、签名、
  不可变tag源码、Tauri配置/模板与fixture/run/case身份分别绑定；旧8版本
  的源码期望不含0.17.7新增ConPTY文件，不能把这种源码声明当成已测量产物
- 专用工作流准备9版本×clean/seeded-existing共18个分别分配的Windows
  job。保留真实受限令牌、进程/Job终态、签名与完整文件/已列举副作用检查；
  不启动历史应用、不操作用户电脑、不自动导入测量结果到生产策略

合并工作树的本地门禁：前端1889项通过；Node66项中64通过、0失败、
2项仍因没有PowerShell而跳过；typecheck、build、37项视觉fixture和差异
检查通过。两个独立范围审查未发现阻塞问题。Rust编译/格式/测试及18个
实际Windows观测此时尚未执行；新增测试源码不能替代它们。

生产历史版本代码、`payload_policy.rs`已审核表、
`SUPPORTED_ROUNDTRIP_ENABLED=false`、`acceptance.rs`及受限令牌实现
不变。即使18个payload观测通过，也仍须逐对审查实际导出的字节与安装
效果，且不能据此宣称独立管理器往返、崩溃恢复或九版本真实E2E通过。

## 9b94b8d远端终态及额外诊断

源码 `9b94b8d2425d81fa35debadc0e6fc5a6f05b1e94`，测试合并提交
`af300b7aee4cefc2b155c41d9eb9426242f676a0` 的七个工作流全部结束：
D12、D13、视觉、恢复专项、payload和Windows测试包通过，
[普通CI](https://github.com/shawnwu2022/cc-desk/actions/runs/37196430278)失败。
其前端1881项、接入后的48项Node契约（0跳过）、类型/构建、无原生验收的
编译策略、格式与严格lint通过；Channel单项复核通过，普通CI的loader跳过。

Rust聚合为1172通过/12失败/31忽略/1过滤，另有独立Job门禁1失败。
12个历史Job拒绝仍与277d2be一致，但本批**另外**出现
`D11_Launch_Native_001` 的90秒worker超时，不能将整个失败集合记为不变。
原测试未在超时时打印worker阶段，现有日志不能证明阻塞在哪一层。
后续CI增加相同精确测试的独立诊断步骤；不修改超时、断言、生产逻辑或
聚合失败结果，独立通过也不能擦除此前失败。

Windows包产物11301397977已经下载核验：归档SHA256
`ec8a7be977ce668ac862aa051a61025e2b546e6d6f7849cd2f14242b882fcb70`；
其中 `CC-Desk-0.18.0-test-af300b7aee4c-x64-setup.exe` 为7741803字节，
SHA256 `a00498c8c4204c99852477981b9c59b62bb70d624df5a198e5854ef7fd6a0e7b`，
与包内manifest和工作流日志一致。仅为test-only证据，没有替换本机安装。
