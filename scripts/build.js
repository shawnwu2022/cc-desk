#!/usr/bin/env node

/**
 * 跨平台打包脚本
 * 用法: node scripts/build.js [platform]
 * 平台: windows | macos | linux | all (默认: 当前平台)
 */

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const platforms = {
  windows: { args: '', check: () => process.platform === 'win32' },
  macos: { args: '--target universal-apple-darwin', check: () => process.platform === 'darwin' },
  linux: { args: '', check: () => process.platform === 'linux' },
  all: { args: '', check: () => true }
};

const platform = process.argv[2] || process.platform;
const config = platforms[platform] || platforms[process.platform];

console.log(`🚀 开始打包 ${platform}...`);

try {
  // 清理旧的构建
  if (fs.existsSync('dist')) {
    fs.rmSync('dist', { recursive: true });
  }

  // 执行打包
  const buildArgs = config.args ? `-- ${config.args}` : '';
  execSync(`npm run tauri build ${buildArgs}`, { stdio: 'inherit' });

  console.log(`\n✅ ${platform} 打包完成！`);

  // 显示输出位置
  showOutputPath(platform);

} catch (error) {
  console.error(`❌ 打包失败:`, error.message);
  process.exit(1);
}

function showOutputPath(platform) {
  const paths = {
    windows: 'src-tauri/target/release/bundle/nsis/ 或 msi/',
    macos: 'src-tauri/target/universal-apple-darwin/release/bundle/dmg/',
    linux: 'src-tauri/target/release/bundle/deb/ 或 appimage/'
  };
  console.log(`📦 安装包位置: ${paths[platform] || 'src-tauri/target/*/release/bundle/'}`);
}
