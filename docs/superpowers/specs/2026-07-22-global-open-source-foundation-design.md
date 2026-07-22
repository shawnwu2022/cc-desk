# CC Desk 全球化开源基础设计

**日期**：2026-07-22
**状态**：已获口头批准，待书面审阅
**范围**：GitHub 社区治理、贡献入口、PR 质量门禁、依赖维护与发布验证；不改变产品功能或发行版本。

## 1. 目标

将 CC Desk 从可独立发布的个人维护仓库，升级为外部用户能安全安装、能清楚求助、能按一致规则贡献、且能验证发布来源的全球开源项目。

成功不以 Star 或翻译数量衡量，而以以下行为闭环衡量：陌生贡献者能从英文文档完成本地验证并提交 PR；安全研究者有私密披露渠道；`main` 不再可绕过检查直接破坏；每个发布资产和 updater URL 可以自动验证。

## 2. 治理模型

- 创始维护者保留产品方向、品牌和最终合并决定权。
- 日常维护由维护者执行：Issue 分流、PR 审查、发布、依赖维护和文档更新。
- 技术决策默认在 Issue 或 Discussion 中公开记录；影响兼容性、数据存储、权限或安全边界的变更必须先写 design/spec。
- 不引入 CLA、基金会或付费赞助承诺；MIT 的 inbound=outbound 贡献条款继续生效。

## 3. 首批交付

### 3.1 GitHub 协作入口

1. 开启仓库 Issues 与 Discussions。
2. 建立标签：`bug`、`feature`、`question`、`documentation`、`good first issue`、`help wanted`、`security`、`needs-triage`、`blocked`、`wontfix`。
3. 新增 Issue Forms：Bug、Feature request、Question；Issue 模板要求英文标题，允许正文使用任意语言。
4. 新增 PR 模板，要求变更说明、验证命令、破坏性变更声明和相关 Issue。

### 3.2 社区契约

根目录新增：

- `CODE_OF_CONDUCT.md`：采用 Contributor Covenant 2.1，指定维护者联系渠道和处理措施。
- `SECURITY.md`：私密漏洞报告路径、支持版本、确认/初步答复/修复目标时间，以及禁止公开未修复漏洞的要求。
- `SUPPORT.md`：Issues 用于可复现 bug，Discussions 用于用法问题与想法；声明不提供 Claude Code、第三方 Provider 或操作系统的通用支持。
- `GOVERNANCE.md`：维护者职责、决策规则、紧急安全例外、维护者加入与退出方式。

`CONTRIBUTING.md` 改为唯一贡献入口：以 MSVC 为 Windows 工具链，列出 Node、Rust、平台依赖和 `npm test`、`npm run typecheck`、`cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test` 的验证矩阵。

### 3.3 质量与供应链

新增 PR CI 工作流，在 Ubuntu 上执行前端安装、测试、类型检查；在 Rust 支持平台执行 `cargo fmt --check`、`cargo clippy -- -D warnings` 与 `cargo test`。发布工作流保留跨平台包构建，但应降低 job 权限：构建 job 只读，Release job 才有发布写入权限。

新增 `.github/dependabot.yml`，每周检查 npm、Cargo 与 GitHub Actions。新增发布验证脚本或测试，下载 Release 的 `latest.json`，确认 Windows、macOS、Linux 三个平台 URL 均返回非 404 响应；该验证应在草稿 Release 公开前完成。

### 3.4 GitHub 保护规则

对 `main` 设置规则：只能通过 PR 合并、要求上述 CI、禁止 force push 与删除分支。首批不强制双人审批或签名提交，以免单维护者无法发布；当至少有两位活跃维护者后再提高门槛。

## 4. 非目标与后续批次

首批不处理 Apple Developer ID/notarization、Windows Authenticode、SBOM、artifact attestation、代码扫描、翻译社区、赞助、组织迁移或基金会。它们属于第二批：先建立可持续的协作和验证基础，再投入账号、证书与费用。

## 5. 安全与隐私边界

本批不读取、上传或修改用户本地 Claude、Provider、MCP 或项目数据。CI Secret 继续只用于 Tauri updater 私钥；不得在日志、Issue 模板、测试夹具或文档中要求用户粘贴 Token、API Key、完整配置文件或私密项目路径。

`SECURITY.md` 只描述项目漏洞披露，不替第三方 Claude Code、Provider、MCP 服务承担漏洞响应义务。

## 6. 验收标准

1. GitHub 页面显示 Issues 已启用、Discussions 已启用，且三类 Issue Form 与 PR 模板可用。
2. 根目录存在四个社区契约文件，链接和联系路径不包含私密凭据。
3. 从干净 clone 按 CONTRIBUTING 的英文步骤可运行规定的验证命令。
4. 新 PR 必须通过 CI 才能进入 `main`，且 `main` 拒绝 force push。
5. Dependabot 覆盖 npm、Cargo 与 GitHub Actions。
6. 发布草稿在公开前会验证 `latest.json` 三个资产 URL；任一 404 则阻止公开。
7. 现有产品功能、版本号、Release 和用户数据格式不因本批改变。

## 7. 文件边界

| 文件 | 职责 |
|---|---|
| `CODE_OF_CONDUCT.md` | 社区行为与执行渠道 |
| `SECURITY.md` | 漏洞私密披露与响应承诺 |
| `SUPPORT.md` | 支持入口与范围 |
| `GOVERNANCE.md` | 维护与决策模型 |
| `CONTRIBUTING.md` | 可复现的贡献者环境与 PR 规则 |
| `.github/ISSUE_TEMPLATE/*.yml` | 结构化外部反馈 |
| `.github/pull_request_template.md` | PR 自检 |
| `.github/workflows/ci.yml` | PR 质量门禁 |
| `.github/dependabot.yml` | 依赖与 Action 更新 |
| `.github/workflows/release.yml` | 最小权限发布及草稿验证 |
| `docs/release-process.md` | 面向维护者的发布验收步骤 |

## 8. 风险与决策

- Issues/Discussions 带来噪声；用模板、标签和关闭理由处理，不以关闭入口替代分流。
- 单维护者下严格审批会阻塞安全修复；首批只要求 CI 与 PR，后续按维护者数量增强审批。
- macOS/Windows 信任提示无法仅靠仓库文件消除；代码签名是后续需要账户与成本的独立里程碑。
- 默认英文面向外部协作，但不拒绝中文或其他语言；维护者负责提供英文摘要以沉淀可检索决策。
