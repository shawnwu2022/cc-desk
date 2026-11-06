# 0.26 正式 main 与 0.27 分支的整合风险

前一分析阶段的比较目标固定为开发 HEAD `1c018f24bf52e6de421a84a397e28d052e1c5b8e` 和 main `0aaaf2a171ced55ff2b4d7a3e1731027c45809df`；该阶段仅更新远端跟踪信息、没有整合。**后续明确授权已取代“仅分析”限制：现在 main 已实际合入开发分支，见文末整合结果。** 原静态分析和证据仍按各自采样源保留，不能反向写成当时已做 merge。

## 引用和差异实测

并行只读分析在 `2026-11-06T09:48:26.9464051Z` 执行 `git ls-remote origin refs/heads/main refs/tags/v0.26.0 refs/tags/v0.26.0^{}`：main 与 tag 都为 **`0aaaf2a171ced55ff2b4d7a3e1731027c45809df`**；无 peel 记录，该对象 `git cat-file -t` 为 commit，故是轻量 tag。

执行 `git fetch --no-tags origin main` 后，`2026-11-06T09:48:57.7460060Z` 确认 HEAD 仍为 `1c018f24…`，只更新本 clone 的远端跟踪信息。根随后独立复核 merge-base、计数与 main 文件清单。

```text
git merge-base HEAD origin/main
4d4674480849f1d4d8fb4edab095b388e769e189

git rev-list --left-right --count HEAD...origin/main
9  2

git diff --name-only 4d4674480849f1d4d8fb4edab095b388e769e189 HEAD
81 files

git diff --name-only 4d4674480849f1d4d8fb4edab095b388e769e189 origin/main
10 files
```

文件交集实测为 **0**，因此当前两组 diff 没有同文件重叠 hunk。这里的 9 个独有提交以开工 `1c018f24` 为准，后续分析文档提交另计。开发侧七个实现及两个文档提交的完整索引在 `candidate-review-0.27.md`；main 独有：

- `1e25b98021a582edadfd7bee5af6ca0ab174b5e4`，准备 0.26.0 stable。
- `0aaaf2a171ced55ff2b4d7a3e1731027c45809df`，合并 PR #80。

main 全部变化为 57 additions / 13 deletions：

| 文件 | 具体影响 |
| --- | --- |
| `package.json` L3 | 0.26.0-rc.3 → 0.26.0 |
| `package-lock.json` L3/L9 | 两处根版本同步，无依赖变化 |
| `src-tauri/Cargo.toml` L3 | 应用版本同步，无依赖变化 |
| `src-tauri/Cargo.lock` L442 | 仅 cc-desk 自身版本同步，无依赖解析变化 |
| `src-tauri/tauri.conf.json` L5 | 应用版本同步 |
| `tests/config/packageVersion.test.ts` L34–39/L47 | 六项元数据断言与最新 changelog heading 同步；日期仍 2026-11-06 |
| `CHANGELOG.md` | 增加正式版条目与既有验收边界 |
| `RELEASE_NOTES.md`、`RELEASE_NOTES_CN.md` | 增加 0.26 正式版说明，明确不包含 0.27/远程功能 |
| `docs/release-notes.json` | 增加中英摘要 |

结论为 **有依据预期文本冲突风险低**；没有实际执行 merge，所以没有“已验证无冲突”或“整合后测试通过”的结果。无文件交集也不能证明行为兼容。

## 正式 Release 的已核实与未核实

`2026-11-06T09:48:59.036Z` 读取 [v0.26.0 Release](https://github.com/shawnwu2022/cc-desk/releases/tag/v0.26.0)：ID `436190361`，draft=false、prerelease=false、immutable=false，target_commitish 为上述 main，published_at=`2026-11-06T06:52:10Z`。这是一份带采样时间的观察，不保证可变 Release/资产以后不变。

随后独立 API 查询 source/run/attempt/jobs/artifacts，不仅根据 tag 名和文案推定来源：

- [普通 CI 42175722832](https://github.com/shawnwu2022/cc-desk/actions/runs/42175722832)，attempt 1，push/main、精确 SHA 一致，全部 23 jobs 成功，含原 producer/static/16 shards、Frontend/Rust 和 roundtrip policy。
- [Release 42175723054](https://github.com/shawnwu2022/cc-desk/actions/runs/42175723054)，attempt 1，同 SHA，全部 6 jobs 成功，含三平台 build、admission、preflight、release。
- 三个未过期 candidate artifacts 的 workflow_run head_sha/branch/run 一致：Windows `13483920178`、macOS `13483453656`、Linux `13482878040`。
- CI coverage artifact `13483437918`，名称 `windows-native-coverage-0aaaf2a171ced55ff2b4d7a3e1731027c45809df-42175722832-1`，同 source/run，采样时未过期。
- `2026-11-06T09:50:06.097Z` 实际读取公开 `windows-native-coverage.json`：794944 bytes，SHA256 **`8eb17a2094cb00e232c1f9704c788bd71e0246a5b22aad34fd606e5f3d6d3bbe`**，与 Release asset `716218108` 的 API digest 一致；内部 sourceSha/runId/attempt/policy 也匹配。

没有下载三平台 candidate ZIP、全部安装包或 coverage 原始 ZIP/logs，没有重做完整平台 payload 比对、三平台 Minisign 验签或发布 preflight。以上只能证明此次读到的远端记录与公开 coverage 字节绑定，不能声称重新验证了完整构建 provenance、安装或返回。没有读取或申请凭据。

## 整合后的语义风险

1. **当前版本身份。** 应用会从 0.26.0-rc.3 变成 0.26.0，不会自动成为 0.27。updater 从实际 package_info 读取当前版本：RC3 下正式 0.26.0 可被视为更高，整合后同版本应被拒绝；历史 scope 的注册版本也必须继续精确匹配 CARGO_PKG_VERSION。不能放松 fixture 或比较规则适应 stable。
2. **发布和通道。** main 不改 workflow/发布脚本；开发侧 beta/RC/preview parser、三平台签名、candidate/test-only 与不可发布属性拒绝、旧 stable latest 和切回 stable 不降级均须保留。已存在 v0.26.0 tag/Release 时晋升同版本应拒绝。将来 bump 需单独授权并同步六项版本身份、测试和说明。
3. **消费/发布版本范围差异。** 静态复核发现现有 Rust `updater_policy.rs::release_channel` 允许裸 beta/rc/preview 以及数字序号 0，而发布侧 `scripts/release-version.mjs` 要求正整数 `.N`。这是开发分支现有兼容范围差异，非 main 引入；本轮没有改变或单独行为复现。未来应明确是否有意兼容，不能把它写成 main 合入造成的回归或本轮已修问题。
4. **Cargo 与测试来源。** main 只更新应用自身锁版本。开发侧 `tests/updater-core` 仍是独立 0.1.0/publish=false 辅助 crate，其生产模块测试不能覆盖 Tauri 实际 package version 或历史安装 scope；不要机械 bump 辅助 crate。固定 0.18.0 的 acceptance roundtrip binding 不随应用版本修改。
5. **证据失效。** 双方 `.github/workflows/ci.yml` 的 Rust 1.98.1 没有变化。开发源 `68fa6b3…` 的本地结果和 main `0aaaf2a…` 的 CI 都属于各自精确来源，不能直接当作未来整合 SHA 的通过。

## 后续安全整合顺序

以下是后续授权后的计划，本轮未执行：

1. 重新采样 feature/main，记录精确双方 SHA 和工作区状态；保持现有独有分支、原 todo、未跟踪证据与其他树不动，在独立整合环境操作。
2. 将正式 main 整合到候选侧，实际检查冲突和完整结果 diff；确认上面十文件和开发侧安全策略均保留。实际 merge 成败与静态预测分开记录，不用 merge-tree 写树假装整合已完成。
3. 先验证六项版本身份、packageVersion/changelog/说明一致性，再验证 updater RC→stable、同版本拒绝、不降级与通道矩阵；历史 scope 保持实际版本匹配，固定 acceptance binding 保留。
4. 按真实改动跑受影响 frontend updater/history、Rust updater policy/HTTP/history scope、manifest/release admission/recovery/coverage/draft fixtures及必要 typecheck/build；全部绑定新整合源。
5. 最终完整普通 CI 使用 Rust 1.98.1/CI profile、原四 harness/16片、原 ignore 和精确宿主限制披露，三个 required checks 同源闭环。不要把旧本地通过迁移成新 CI 通过。
6. 将来发布须另有授权，先合受保护 main，再按实际 merged-main 重验 CI/coverage、同 release run 的三平台字节与签名。0.26 的签名与 CI 不给 0.27 晋升权。本轮不 bump、不发布、不发 PR。

## 后续已授权的实际整合

再次 fetch 后目标仍为 `0aaaf2a171ced55ff2b4d7a3e1731027c45809df`，十文件没有变化。合并前 HEAD `aaad3d2bf1c05c40c30ab03d9d46c38d05f7b4cc` 已保存在 `refs/codex/v0.27.0/pre-main-aaad3d2b`。实际 `--no-ff --no-commit` 合并无冲突，检查后 merge commit 为 **`89e30ff37ca51b29239c17abbc52f0dc85106390`**，两 parent 即上述开发 HEAD 和正式 main；没有改写历史或操作其他树。

新增一个版本转换回归后，已验证的测试提交 **`68436f31dc8a6313e9d87ca9ab35717671edfcfd`** 对应 tree `acc7ec63f914b23f4388586e8d6f254d02ebbfd9`：固定1.98.1/CI profile的Rust受影响域91通过，前端14文件169、Node310/6原跳过、all-target Clippy/fmt、类型/构建和CSP2/2通过。实际应用版本0.26.0；未新增0.27版本号。详细命令、scope差异和未跑完整套件范围见 [整合验证](../.audit/v027-stage6-main-integration.md)。独立复核无新确定性回归，无依据要求重跑16片；新源完整CI和真实安装/往返仍未做。
