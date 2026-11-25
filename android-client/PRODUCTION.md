# Android arm64 生产候选包

这是 **构建与签名候选**，不部署、不创建 Release、不修改更新通道。先合并整批移动端实现，确认精确受保护 main SHA 的普通 CI 通过，再在同一批验收中运行一次 Android production candidate。签名候选方可进入真机验收，并不等于真机已通过或公开发布。

## 启动前

1. 合并 Java 桌面后端和 Rust JNI：Android NativeDesktopBackend.java 必须加载 cc_desk_android_native；Observer Java 声明的 JNI 名称必须与 android-client/native Rust 导出一一相符。Cargo.lock 必须已提交。20aa6441 的 Java 发展包与 7638096c 的基础 Rust 导出并未构成可发布实现；不可用上传 APK 代替协议、批准及撤销验收。
2. 已有、经授权的 **Android** 正式签名身份，不能使用桌面 Tauri updater 密钥、调试证书或本地临时密钥。GitHub Actions 环境 android-production 必须由仓库负责人配置，仅允许受保护分支并设 Required reviewers。将现有正式签名配置仅放该环境：Secrets ANDROID_RELEASE_KEYSTORE_B64、ANDROID_RELEASE_KEYSTORE_PASSWORD、ANDROID_RELEASE_KEY_PASSWORD；Variables ANDROID_RELEASE_KEY_ALIAS、ANDROID_RELEASE_CERT_SHA256（64 位十六进制，不带冒号，独立从受信的正式证书或以前的可信发行包确定）。环境/真实密钥尚未证实配置。缺任一项、证书与固定指纹不一致，或证书为 debug，均不保留候选。不要在聊天、日志、构建产物或仓库提交密钥。
3. 记录该渠道历史最高 versionCode，工作流手动输入严格递增的下一值，版本名固定取本次 main 的 package.json。首包的 versionCode 也需要由该渠道负责人登记。脚本仅校验范围，无法推断历史分发渠道的最大值；不要复用或降低。
4. Runner 固定 windows-2025；Android SDK 35、Build-tools 35.0.0、JDK 17 和官方 runner 预装 NDK r27.3.13750724。Rust 固定 1.100.0、aarch64-linux-android 和 cargo build --release --locked；缺工具即退出，不回落旧 r22 或随意更换 NDK。DEX 主产物的 ZXing Core 3.5.3 有固定 SHA-256。

## 验收证据

- 唯一 ARM64 AArch64 ELF64 shared object，通过各 LOAD segment 16 KiB 检查；Java JNI 名称在实际 ELF 动态符号中逐项存在。
- JNI 库不压缩，APK 先 zipalign -P 16 再正式签名，签名后重新验 16 KiB 对齐、包名与版号、权限、APK v2/v3 签名、证书 SHA-256，以及 APK 内 JNI 库与已验证 ELF 的 SHA-256。不可在签名后编辑 APK。
- Actions 仅保留 SHA、run ID 和 attempt 绑定的不可覆盖候选，包含 .apk 与 android-release-build-proof.json，无 keystore、无未签包、无环境变量副本。候选的 proof 默认明确标记设备未验、未发布。
- 同一批集中验收采用 artifact 内精确 APK 文件：核对 proof 的签名者、文件 SHA 和 source SHA，安装在受控 ARM64 Android 31+ 设备，另在 Android 15+ 16 KiB 页面设备/模拟器上确认可启动、首次批准后只读数据流、撤销后同设备不能重连。补测已有正式签名包升级（如有）。未跑这些不能宣称生产就绪。后续公开发布只能由既有受授权流程使用同字节候选。

依据：[Android 16 KB](https://developer.android.com/guide/practices/page-sizes)、[apksigner](https://developer.android.com/tools/apksigner)、[Windows 2025 映像](https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md)。


## 合并发布：一个受保护主干测试版本

0.28 移动端按编号的 RC 流程处理。桌面和带 JNI3 控制接口的 Android 同时进入同一个测试版本，不支持自动推送触发移动发布、0.28 稳定发布、或者桌面缺 APK 时先发布。现有普通非移动发布门禁不变。当前公开版以及此次工作基线均是 0.28.0-rc.3；未修改版本。以后在推进确切版本之前，重新检查公开标签和草稿，选择尚未占用的编号 RC，完成全套版本文件修改并合并受保护 main。

手动从 main 启动 Release，输入与目标签名渠道相适配的 android_version_code；它在同一运行中构建既有桌面平台资产，再调用 Android 生产候选。保护审阅、精确 main、精确普通 CI run/attempt、现有桌面证书、Android 固定签名证书和版本是硬门禁。对 APK 所在同运行不可覆盖 artifact 用 ID 下载，重新核验真实 v2/v3 签名、16 KiB ZIP 对齐和本地包 SHA；APK 证明中还必须有 Android 原生安全测试已运行并通过。运行中重试所需平台时，应从同一新 attempt 重跑完整套件，不能混用旧 attempt 的 Android APK。

发布先创建不可公开的草稿，然后以 GitHub 官方资产的 SHA-256 与大小核查全部 13 个文件，再重新校验当前 main 与精确 CI，最后只切换该草稿为 prerelease 且 make_latest=false。错误停在未发布草稿，不能发桌面替代、不就地复用未知旧草稿。实际签名密钥、GitHub reviewer、托管构建与最终发布均没有在本次开发环节操作。隔离安全测试不是手机/远程生产链路验收。
