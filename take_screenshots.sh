#!/bin/bash
# App Store Screenshot Generator for ChatFinder
# On Retina (2x) display: 1440x900 → 2880x1800, 1280x800 → 2560x1600

set -e

APP_NAME="ChatFinder"
APP_PATH="$(dirname "$0")/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/ChatFinder.app"
OUTPUT_DIR="$HOME/Desktop/AppStore-Screenshots"

# Window position: start just below menu bar (~45pt)
WIN_X=0
WIN_Y=45

mkdir -p "$OUTPUT_DIR"

resize_window() {
    local w=$1 h=$2
    osascript << EOF
tell application "$APP_NAME" to activate
delay 0.3
tell application "System Events"
    tell process "$APP_NAME"
        set position of front window to {$WIN_X, $WIN_Y}
        set size of front window to {$w, $h}
    end tell
end tell
EOF
    sleep 0.8
}

capture() {
    local w=$1 h=$2 file=$3
    resize_window "$w" "$h"
    screencapture -R "${WIN_X},${WIN_Y},${w},${h}" "$OUTPUT_DIR/${file}.png"
    echo "  ✓ $file.png  (actual: $((w*2))x$((h*2))px on Retina)"
}

prompt_and_capture() {
    local num=$1 desc=$2
    echo ""
    echo "── Screenshot $num: $desc ──"
    echo "请在App中切换到对应界面，按 Enter 拍照..."
    read -r
    capture 1440 900 "screenshot_0${num}_1440x900"
}

# ── Open app ──
if ! pgrep -x "$APP_NAME" > /dev/null; then
    echo "启动 $APP_NAME..."
    open "$APP_PATH"
    sleep 4
else
    echo "$APP_NAME 已在运行，继续..."
    osascript -e "tell application \"System Events\" to tell process \"$APP_NAME\" to set frontmost to true"
    sleep 1
fi

echo ""
echo "======================================="
echo "  ChatFinder App Store 截图生成器"
echo "======================================="
echo "输出目录: $OUTPUT_DIR"
echo "格式: 1440×900pt → 2880×1800px (Retina 2x)"
echo ""
echo "共需拍摄 5 张截图，每张需要你先切换好界面。"

prompt_and_capture 1 "对话列表主界面"
prompt_and_capture 2 "对话详情 / 消息浏览"
prompt_and_capture 3 "全文搜索"
prompt_and_capture 4 "收藏夹"
prompt_and_capture 5 "导入对话界面"

echo ""
echo "======================================="
echo "✅ 全部完成！"
echo "文件位置: $OUTPUT_DIR"
echo ""
echo "Retina 实际像素: 2880×1800 px (App Store 接受 ✓)"
echo ""
ls -lh "$OUTPUT_DIR"/*.png 2>/dev/null
echo "======================================="
