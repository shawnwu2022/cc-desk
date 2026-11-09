# 固定工具链执行前核对

执行前已读取 `v027-stage3-pinned-rust-preflight.json`：

- source `68fa6b3df0f34534bffd493173bdf8cfcc5b3ef3`，Rust `1.98.1 (48a229cea 2026-09-01)`，dev/test debug 都为 0；宿主 inJob=true、elevated=false。
- plan SHA256 `9bf9c2e8b8ad5e945f9a7de91d5d84abc0e943c4fe4f528068e2a9380ac0349f`。
- 四 harness bytes / SHA256：
  - lib：61718016 / `8c5c3d7d34e96db1e0da658e43314916fa24307478043bffea4de81c55fcc8a5`。
  - bin：3733504 / `edda1431a4c57b782cf814aa673d639d1bd5b44117ea26c156d3b3788d75ca10`。
  - paste_transport：1427456 / `6dc3b748425958cdb46d884e8ded2a3e0c0ddbfd7e5bae844dde7e53bd233ad3`。
  - paste_claude_e2e：1439232 / `caa61f90de1851eb1688fea6d9870b410563334ab37deeb676fb3870e3d53eed`。
- 原 runner 的 verifyBundle 在开始分片前及每片前实际重算 source / 编译包 / 四 harness 摘要；0–15 分片，harness 1200000ms、分片 1500000ms，原 ignore 和18项宿主排除保持不变。

额外 Node 确认源 SHA、Cargo 库存哈希和四文件字节/hash 全部匹配。第一次额外确认试图在默认执行沙箱重定向到 `v027-stage3-pinned-preflight-check.log`，Out-File 报 Access denied，文件未产生；这不是自动审批拒绝。未重试该写入，改用无重定向的只读确认，工具返回匹配。这个额外日志写入失败不影响此前已有的 preflight JSON 和原 runner 已执行的校验，也未中断/重跑健康分片。

所有绑定均属本地合成 run 命名空间 `2026110602/1`，不是 hosted CI 或发布证据。
