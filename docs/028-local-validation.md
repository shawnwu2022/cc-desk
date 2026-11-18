# 0.28 开发集成本地验证边界

本记录不是 RC 发布批准。源码基线为 bb197bec8c96c9faa5c9977a1db56613bf636054；版本仍继承主干 0.27.0，未 bump、推送、安装或发布。

## 本轮集成源码实测

- 离线重编 remote-host 原测试 harness，44.34 秒，exit 0。不是复用旧远程分支二进制。
- 原 product 取消/foreign advance 004、真实 Panel 双代 008、Panel 空闲 16 秒跨 lease 后操作 009 各 1 项通过；内外 harness 同例只计一次。
- Antigravity_ 过滤器 4 项通过：参数、能力关闭、身份/observer 隔离及偏好持久化。不运行真实 agy，不证明真实认证/恢复。
- 真实 Panel 使用本树生产 WASM 清单（逐 source/input/file 校验）、真实模块 Worker、隔离 Chromium 与公开虚拟 WebAuthn。合成 HTTPS origin 和 Node fixture-CA WSS 代理不证明浏览器可信 TLS、真实手机或普通 Tauri 桌面交互。Relay 复用独立 clean 6382ffe1fbde46ff9bb5586fb25d7c92e5617f2d，未改或部署。
- 默认/取消/Waiting UI 契约三文件 76/76 通过（cliSupport、remoteHostPanel、remoteTerminalPanel），为组件与 mock IPC 层，不代替上面真实 Panel 链。本轮共 native 7 + UI 76 = 83 项；没有全库回归或新增 Rust 代码。
- 日志：本树 .audit/integration-rc-native-build.log 与 integration-rc-UnattendedProduct_*、integration-rc-Antigravity_.log；integration-rc-defaults-ui.log 及 integration-rc-source.json 保存本轮源码库存；旧分支 108 项记录不是本轮同源码验收。

## 可交付范围与 RC 缺口

当前仅默认关闭、显式授权的最多 60 秒远程试用；等待/浏览器替代各自有界，不承诺长期无人值守、跨 App 恢复或随时回来连接。原授权截止、撤销、未结算输入不重放与 Unknown 隔离不因集成放宽。

专用 remote-trial 档已验证离线 no-bundle EXE/dist、双 runtime 来源及 clean HEAD 标识；ConPTY 诊断只证明原 loader/PTY 生命周期，不是普通桌面验收。updater artifacts 不生成，不代表运行时 updater 插件停用。

RC 尚缺：真实手机人工登记与经授权配置、可信浏览器 TLS 正链、独立 OS 环境下普通桌面/真实 AGY 认证及恢复、长期授权与安全日志维护产品接线、Job-free 平台发布门及安装/签名/升级验证。不得用短试用、合成浏览器或参数测试替代这些门。真实桌面隔离不能仅改 cwd/HOME：启动涉及安装控制、用户配置/日志、WebView profile、插件与后台更新，当前不启动用户 App。

AGY keyring 的新边界见 [AGY 文档](antigravity-cli.md) 与 [L07](lessons-learned.md#l07-真实-cli-要分层验证)。本轮未读写真实凭据或用户会话。
