# 更新代理

更新设置中的 HTTP/HTTPS 地址和端口明文显示。可选用户名、密码使用独立密码字段；粘贴带认证的 URL 时立即分离认证信息。留空表示沿用更新客户端原有的进程环境及系统代理，不改变系统代理或 CLI 的配置。

“测试代理”检测当前输入的草稿，无需先保存。检测通过真实更新来源验证使用的 `validated_proxy` / `http_client`，只读取固定官方地址 `https://github.com/shawnwu2022/cc-desk/releases/latest/download/latest.json`，15 秒超时、最多 1 MiB。HTTP 重定向仅接受官方 GitHub 与发布资产域名上的无认证 HTTPS/443 URL。成功表示该清单可达且具备更新清单结构；不证明更新包已通过来源或签名验证，也不创建安装准入，不访问清单中的安装包 URL。

页面显示检测中、成功或失败，以及请求耗时。错误只使用现有固定安全代码和本地化说明，原始网络异常和敏感 URL 不进入日志或反馈。重复点击只发起一条请求；修改任一地址或认证字段、离开页面或卸载页面后，旧完成不再发布。配置修改后需重新检测。保存行为与正式更新的来源、签名、会话门禁保持原有边界。

自动验证：`tests/components/remainingSettings.test.ts` 覆盖可见地址、认证分离、草稿检测、重复提交、配置变化、失活、超时与错误脱敏；`tests/api/updaterBackend.test.ts` 覆盖独立只读命令。`cargo test --manifest-path tests/updater-core/Cargo.toml --locked` 导入实际生产 Rust 源码，覆盖固定官方 CONNECT 目标、清单结构、响应体超时和非官方重定向拒绝，以及原有继承/显式代理、来源和签名测试。这些测试使用合成回环服务，不能替代目标机器的实际代理连接或安装验收。
