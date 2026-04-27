# 打包与分发指南

## 本地打包

### Windows
```bash
npm run build:win
```
输出: `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/`
- NSIS 安装程序: `bundle/nsis/*.exe`
- MSI 安装程序: `bundle/msi/*.msi`
- 免安装版: `release/*.exe`

### macOS
```bash
npm run build:mac
```
输出: `src-tauri/target/universal-apple-darwin/release/bundle/`
- DMG 镜像: `bundle/dmg/*.dmg` (推荐分发)
- APP 包: `bundle/macos/*.app`

### Linux
```bash
npm run build:linux
```
输出: `src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/`
- Debian 包: `bundle/deb/*.deb`
- AppImage: `bundle/appimage/*.AppImage`

## 自动化发布（GitHub Actions）

### 创建 Git 标签触发发布
```bash
git tag v1.0.0
git push origin v1.0.0
```

### 发布流程
1. 推送标签 → GitHub Actions 自动构建
2. 所有平台的安装包自动上传
3. 创建 GitHub Release (草稿)
4. 审核后发布

### 支持的平台
- Windows (x64): NSIS + MSI
- macOS (Universal): DMG + APP
- Linux (x64): DEB + AppImage

## 签名与公证（可选）

### Windows 代码签名
在 `src-tauri/tauri.conf.json` 中配置:
```json
"bundle": {
  "windows": {
    "certificateThumbprint": "你的证书指纹",
    "timestampUrl": "http://timestamp.digicert.com"
  }
}
```

### macOS 代码签名
```bash
# 导入证书到钥匙串
security import certificate.p12 -k ~/Library/Keychains/login.keychain-db

# 在 tauri.conf.json 配置
"macOS": {
  "signingIdentity": "Developer ID Application: Your Name",
  "hardenedRuntime": true
}
```

## 发布到应用商店（可选）

### Windows
转换为 MSIX 包并发布到 Microsoft Store

### macOS
使用 Transporter 上传到 Mac App Store

### Linux
- Debian/Ubuntu: 上传到 PPA
- Fedora: 构建 RPM 包
- Flatpak: 发布到 Flathub

## 用户安装指南

### Windows
1. 下载 `.exe` 或 `.msi` 安装程序
2. 双击运行，按提示安装
3. 桌面和开始菜单自动创建快捷方式

### macOS
1. 下载 `.dmg` 镜像文件
2. 打开 DMG，拖拽到 Applications 文件夹
3. 首次打开需要在系统偏好设置中允许

### Linux
```bash
# Debian/Ubuntu
sudo dpkg -i claude-gui_1.0.0_amd64.deb

# AppImage (通用)
chmod +x claude-gui_1.0.0_amd64.AppImage
./claude-gui_1.0.0_amd64.AppImage
```

## 体积优化

### 当前配置（已启用）
- `strip = true` - 移除调试符号
- `lto = true` - 链接时优化

### 进一步优化
- 禁用不必要的 Tauri 功能
- 使用 Upx 压缩二进制文件
- 按需加载前端资源

## 更新机制（TODO）

Tauri 内置更新器可实现自动更新，配置步骤:
1. 设置更新服务器（可自建）
2. 在 `tauri.conf.json` 启用 updater
3. 前端实现更新检查 UI

详细文档: https://v2.tauri.app/distribute/update/
