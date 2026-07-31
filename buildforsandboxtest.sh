#!/bin/bash
set -e

# -------------------------------------------------------
# 本地沙盒测试构建脚本
# 不需要 provisioning profile，使用 Apple Development 证书
# macOS 内核会根据 entitlements 强制执行沙盒，不依赖 profile
# -------------------------------------------------------

ARCH=$(uname -m)  # arm64 或 x86_64
if [ "$ARCH" = "arm64" ]; then
  TARGET="aarch64-apple-darwin"
else
  TARGET="x86_64-apple-darwin"
fi

APP="/Users/a1-6/PycharmProjects/remake_history/app/src-tauri/target/${TARGET}/release/bundle/macos/ChatFinder.app"
ENTITLEMENTS="/Users/a1-6/PycharmProjects/remake_history/app/src-tauri/entitlements.dev.plist"
SIGN_DEV="Apple Development: Tao Liu (9V77JTT89B)"

echo "==> 架构：$ARCH  目标：$TARGET"

echo "==> 构建（原生架构，速度更快）"
cd /Users/a1-6/PycharmProjects/remake_history/app
npm run tauri build -- --target "$TARGET" --bundles app

echo "==> 清除隔离标记"
xattr -cr "$APP"

echo "==> 签名（从内到外，使用 Apple Development 证书，无需 profile）"
codesign --force --verbose \
  --sign "$SIGN_DEV" \
  --entitlements "$ENTITLEMENTS" \
  --identifier "com.a1-6.chatvault" \
  "$APP/Contents/MacOS/app"

codesign --force --verbose \
  --sign "$SIGN_DEV" \
  --entitlements "$ENTITLEMENTS" \
  --identifier "com.a1-6.chatvault" \
  "$APP"

echo "==> 验证签名"
codesign --verify --strict --verbose=1 "$APP" 2>&1

echo "==> 确认沙盒 entitlements 已嵌入"
codesign -d --entitlements :- "$APP" 2>/dev/null | grep -E "app-sandbox|user-selected|network" \
  || echo "    警告：entitlements 未读取到，请检查签名"

echo ""
echo "==> 启动 App 进行测试"
echo "    数据目录（沙盒容器）："
echo "    ~/Library/Containers/com.a1-6.chatvault/Data/Library/Application Support/"
echo ""
open "$APP"

echo "==> 完成。App 已在沙盒模式下启动。"
