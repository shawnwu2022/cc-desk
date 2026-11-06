# 0.27 服务同源与 main 整合风险记录

开工 HEAD `1c018f24bf52e6de421a84a397e28d052e1c5b8e`，feat/v0.27.0，受跟踪工作区干净。原九项未跟踪 audit 材料保留，包括 todo、隔离 CLI 根、两轮编译包/分片、pinned coverage 和旧 Cargo 库存。此阶段没有应用代码或测试改动。

## A：实际完成与拒绝边界

固定 OpenCode 9bbb8ce18c80f099e0e448775f59d965ead40a21，通过官方 GitHub 读取 server-process、service-registration、service-config、server auth/info/schema/process、client service/probe、database-path；本地只读核对 Root/scoped_fs、私有 SQLite 快照、ScopeRegistry、with_server 与 v2 watcher 文档。字段与链接在 `docs/opencode-service-source-proof-0.27.md`。

发现并准确保留 instanceID/注册/认证，不将其误写为没有服务身份；缺少 OS 对端/进程生命周期→实际 DB 对象→held DB 的可信连接。没有可从当前既有授权入口取得的 issuer，故没有创建不可兑现的纯模型或预览 API。提供具体缺失契约、误绑/替换/跨 profile/取消/截断的合成 fixture 验证方案，尚未执行这些未来测试，不计 GREEN。

原 cleanup_safety_design_review（实际既有 gpt-6.1-sol/high 配置）独立只读复核无新的确定性问题；建议补充 server 启动调度/恢复副作用。根从已读取的 process.ts 精确核对 #L109–115/#L245–246 后补入，并限制 managed lifecycle 的条件。没有把既有创建/重命名的不同授权范围定性为漏洞。没有启动 CLI/server、初始化库、读真实注册/凭据/正文、写 nonce 或任何 mutation；不是工具拒绝、pending 批准或不确定执行。

## B：实际完成的 Git/API 读取

独立只读代理 main026_integration_risk，实际 gpt-6.1-sol/high；根持有 A，不并发改文件。代理只执行授权的 ls-remote/fetch/read/diff/API，没有文件写入或提交。准确命令及结果：

```text
git ls-remote origin refs/heads/main refs/tags/v0.26.0 refs/tags/v0.26.0^{}
main/tag = 0aaaf2a171ced55ff2b4d7a3e1731027c45809df；无 peel
git fetch --no-tags origin main
仅本 clone 的 origin/main 更新；HEAD 保持 1c018f24...
git cat-file -t 0aaaf2a171ced55ff2b4d7a3e1731027c45809df
commit
git merge-base HEAD origin/main
4d4674480849f1d4d8fb4edab095b388e769e189
git rev-list --left-right --count HEAD...origin/main
9 2
git diff --name-only <merge-base> HEAD
81 files
git diff --name-only <merge-base> origin/main
10 files；交集0
```

根随后独立复核 refs/base/count/main 十文件及关键版本 hunk。实际没有 checkout/merge/merge-tree/rebase/cherry-pick，没有修改本地 main（本 clone 无此分支）或其他树；不能声称整合无冲突或整合验证通过。

代理独立读取 Release436190361、CI42175722832/attempt1的23成功jobs、release42175723054/attempt1的6成功jobs、精确3个候选artifact和coverage artifact元数据，并读取公开coverage资产794944 bytes，SHA256 8eb17a2094cb00e232c1f9704c788bd71e0246a5b22aad34fd606e5f3d6d3bbe，与API digest一致。采样时间、来源边界和全部对象ID在 `docs/main-0.26-integration-risk-0.27.md`。没有重做平台payload签名/覆盖ZIP原日志完整验证，不声称完整provenance重验。

## 验证和技能范围

新增文档仅做本地链接、源码路径、固定源码摘录/锚点、Git图/文件集、原始source/hash一致性与 diff whitespace 检查。不重跑已过 Rust/Vitest/Node/构建/CSP/界面套件。所用 superpowers dispatching-parallel-agents 按两个独立领域分工，既有 requesting/receiving review、完成前验证及 pstack foundational-thinking 用于不伪造能力与信任边；未安装技能或环境。

检查实际退出 0，日志 `.audit/v027-stage5-doc-check.log`：7 份文档、31 个本地链接存在；固定分析 HEAD/main/base 与 9/2 独有提交、81/10 文件、交集 0 均一致；`git diff --check` 通过；实现路径相对 `68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3` 无变化。由一次 Node stdin 文档/Git 检查执行，不导入应用；日志保留 Git 既有 LF/CRLF 提示。之后仅追加此结果和 ledger，提交文档；没有挂起的验证或未知服务操作。
