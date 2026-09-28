---
name: cc-desk-release
description: CC Desk 候选构建、Native CLI 验收与受保护 promotion。当用户说“发布”、“release”、“版本更新”、“准备发布”或“上传 OSS”时使用。
---

# CC Desk Release

## 稳定版发布原则

禁止直接执行旧的：

```bash
npm run release -- --bump ...
npm run release -- --exact ...
```

这些入口会 fail-closed。稳定版只能走：

1. 冻结版本与源码；
2. 审阅并将 `docs/testing/native-cli-release-targets.json` 设置为 `READY`；
3. 手工运行 **Signed candidate packages**；
4. 保存 candidate run ID 和 `candidate-manifest.json`；
5. 安装同一批候选文件并采集真实 acceptance evidence；
6. 运行 **Native CLI acceptance gate**，显式指定 candidate run 和 evidence run；
7. 审查完整 target/case/subcase 覆盖；
8. 运行 **Promote accepted native CLI candidate**；
9. 通过受保护的 `release-promotion` Environment 审批；
10. promotion 只发布已验收的候选文件，不重新构建；
11. 发布后验证 updater manifest，再将 Release 标记为 Latest。

## Fail-closed 条件

以下任一情况都不得发布：

- D20 或目标 target 的必需 C/D 证据仍为 BLOCKED/NOT_RUN/FAIL；
- target plan 不是 `READY`；
- 缺失 case/subcase；
- evidence layer 比 target plan 要求更弱；
- candidateId/commit/hash 不匹配；
- evidence 文件不存在、被篡改或哈希不匹配；
- promotion 不是来自显式 workflow_dispatch；
- 未通过 `release-promotion` Environment 审批；
- promotion 过程中发生重新 build。

## Canary

**Native CLI pinned/stable canary** 只记录 Claude Code / Codex CLI 的版本和二进制 SHA-256。

Canary 状态必须保持：

- `NOT_CERTIFIED`，或
- `BLOCKED`

Canary 不能直接变成 acceptance PASS。

## OSS 镜像

GitHub Release 已存在后，仍可使用：

```bash
npm run release -- --oss-only v<x.y.z>
```

这只是镜像已有 Release，不是稳定版发布入口。

## Release notes

英文，动词开头，只描述用户可观察变化。

## 发布后核对

- GitHub Release tag 与候选版本一致；
- Release target commit 与 candidate commit 一致；
- `latest.json` 版本正确；
- Windows/macOS/Linux updater 资产 URL 可访问；
- 发布资产来自 candidate manifest 中的同一批 SHA-256；
- GitHub Latest 只在 manifest 校验通过后指向该版本。

任一失败：停止 promotion，不以“重新构建同版本”修补原 candidate；需要修改构建输入时必须生成新 candidate。
