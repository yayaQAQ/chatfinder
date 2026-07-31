#!/bin/bash
set -e  # 任何步骤失败立即退出

APP="/Users/a1-6/PycharmProjects/remake_history/app/src-tauri/target/universal-apple-darwin/release/bundle/macos/ChatFinder.app"
PROFILE="$HOME/Library/MobileDevice/Provisioning Profiles/chatfinder.provisionprofile"
ENTITLEMENTS="/Users/a1-6/PycharmProjects/remake_history/app/src-tauri/entitlements.plist"
SIGN_APP="Apple Distribution: Tao Liu (8T9X7PLQ68)"
SIGN_PKG="3rd Party Mac Developer Installer: Tao Liu (8T9X7PLQ68)"

echo "==> 构建 Universal Binary"
cd /Users/a1-6/PycharmProjects/remake_history/app
npm run tauri build -- --target universal-apple-darwin

echo "==> 清除隔离标记"
xattr -d com.apple.quarantine "$PROFILE" 2>/dev/null || true
xattr -cr "$APP"

echo "==> 嵌入 Provisioning Profile"
cp "$PROFILE" "$APP/Contents/embedded.provisionprofile"
echo "    已嵌入：$(ls -lh "$APP/Contents/embedded.provisionprofile")"

echo "==> 签名（从内到外）"
# 先签主二进制，再签 app bundle（不用 --deep，避免 Transporter 拒绝）
codesign --force --verbose \
  --sign "$SIGN_APP" \
  --entitlements "$ENTITLEMENTS" \
  --identifier "com.a1-6.chatvault" \
  "$APP/Contents/MacOS/app"

codesign --force --verbose \
  --sign "$SIGN_APP" \
  --entitlements "$ENTITLEMENTS" \
  --identifier "com.a1-6.chatvault" \
  "$APP"

echo "    验证签名："
codesign --verify --strict --verbose=1 "$APP" 2>&1
codesign -d --entitlements :- "$APP" 2>/dev/null | grep -E "application-identifier|team-identifier" || echo "    警告：entitlements 未找到"

echo "==> 打包 .pkg"
productbuild \
  --component "$APP" /Applications \
  --sign "$SIGN_PKG" \
  ~/Desktop/ChatFinder.pkg

echo "==> 完成：~/Desktop/ChatFinder.pkg"
