#!/bin/bash
# CC-Box 自动化发布脚本
# 执行完整的发布流程：提交、推送、打标签、等待 CI、发布

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 获取版本号
get_version() {
  grep '"version"' package.json | head -1 | sed 's/.*"version": *"([^"]+)".*/\1/'
}

VERSION=$(get_version)
TAG="v${VERSION}"

echo -e "${BLUE}=== CC-Box Release Script ===${NC}"
echo -e "Version: ${GREEN}${VERSION}${NC}"
echo -e "Tag:     ${GREEN}${TAG}${NC}"
echo ""

# 检查 git 状态
if ! git diff-index --quiet HEAD --; then
  echo -e "${YELLOW}Warning: There are uncommitted changes${NC}"
  echo -e "Changes:"
  git status --short
  echo ""
fi

# 检查版本号一致性
CARGO_VER=$(grep '^version' src-tauri/Cargo.toml | head -1 | sed 's/version = "([^"]+)"/\1/')
TAURI_VER=$(grep '"version"' src-tauri/tauri.conf.json | sed 's/.*"version": *"([^"]+)".*/\1/')

if [ "$VERSION" != "$CARGO_VER" ] || [ "$VERSION" != "$TAURI_VER" ]; then
  echo -e "${RED}Error: Version numbers are not consistent!${NC}"
  echo -e "  package.json:     ${VERSION}"
  echo -e "  Cargo.toml:       ${CARGO_VER}"
  echo -e "  tauri.conf.json:  ${TAURI_VER}"
  exit 1
fi

echo -e "${GREEN}✓ Version numbers are consistent${NC}"
echo ""

# 提示用户确认
echo -e "${YELLOW}This will:${NC}"
echo "  1. Commit all changes"
echo "  2. Push to GitHub main"
echo "  3. Create tag ${TAG}"
echo "  4. Push tag (trigger CI)"
echo "  5. Wait for CI build"
echo "  6. Publish Release"
echo ""
read -p "Continue? (y/n) " -n 1 -r
echo ""
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
  echo "Aborted."
  exit 1
fi

# 步骤 1: 提交更改
echo -e "${BLUE}Step 1: Committing changes...${NC}"
git add -A

# 从 CHANGELOG 提取当前版本的变更内容
CHANGELOG_SECTION=$(sed -n "/## \[${VERSION}\]/,/^## /p" CHANGELOG.md | sed '1d;$d')

# 构建 commit message
COMMIT_MSG="Release ${TAG}

${CHANGELOG_SECTION}

Author <Chen Zihan>: orczh_hj@163.com"

git commit -m "$COMMIT_MSG"
echo -e "${GREEN}✓ Committed${NC}"

# 步骤 2: 推送到 main
echo -e "${BLUE}Step 2: Pushing to main...${NC}"
git push origin main
echo -e "${GREEN}✓ Pushed${NC}"

# 步骤 3: 创建标签
echo -e "${BLUE}Step 3: Creating tag ${TAG}...${NC}"
git tag -a "$TAG" -m "Release ${TAG}"
echo -e "${GREEN}✓ Tag created${NC}"

# 步骤 4: 推送标签
echo -e "${BLUE}Step 4: Pushing tag (triggering CI)...${NC}"
git push origin "$TAG"
echo -e "${GREEN}✓ Tag pushed${NC}"

# 步骤 5: 等待 CI
echo -e "${BLUE}Step 5: Waiting for CI build...${NC}"
echo -e "${YELLOW}This may take 10-15 minutes...${NC}"

# 获取最新的 workflow run
sleep 5  # 等待 workflow 创建
RUN_ID=$(gh run list --workflow="release.yml" --limit=1 --json databaseId --jq '.[0].databaseId')

if [ -z "$RUN_ID" ]; then
  echo -e "${RED}Error: Could not find workflow run${NC}"
  echo "Please check https://github.com/orczh-hj/cc-box/actions"
  exit 1
fi

echo -e "Watching run: ${RUN_ID}"
gh run watch "$RUN_ID" --exit-status

echo -e "${GREEN}✓ CI build completed${NC}"

# 步骤 6: 发布 Release
echo -e "${BLUE}Step 6: Publishing Release...${NC}"

# 构建 release notes
RELEASE_notes=$(sed -n "/## \[${VERSION}\]/,/^## /p" CHANGELOG.md | sed '1d;$d' | sed 's/^### /## /')

gh release edit "$TAG" --notes "$RELEASE_notes" --draft=false

echo -e "${GREEN}✓ Release published!${NC}"

echo ""
echo -e "${GREEN}=== Release Complete ===${NC}"
echo -e "Download: https://github.com/orczh-hj/cc-box/releases/tag/${TAG}"