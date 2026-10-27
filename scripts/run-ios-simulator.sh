#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
export DEVELOPER_DIR="/Applications/Xcode-beta.app/Contents/Developer"
TARGET="aarch64-apple-ios-sim"
APP_DIR="$ROOT_DIR/vector-ui/target/$TARGET/release/Forma.app"
DEVICE="${FORMA_IOS_DEVICE:-iPhone 17 Pro}"

BACKEND_ARGS=("--platform" "ios")
if [ "${1:-}" = "--egui" ]; then
  BACKEND_ARGS+=("--backend" "egui")
elif [ "${1:-}" = "--svet" ]; then
  BACKEND_ARGS+=("--backend" "svet")
fi

echo "[1/3] Сборка Forma для iOS Simulator ($TARGET)..."
cargo build --release --offline --manifest-path "$ROOT_DIR/vector-ui/Cargo.toml" --features native --target "$TARGET"

echo "[2/3] Упаковка и подпись Forma.app..."
mkdir -p "$APP_DIR"
cp "$ROOT_DIR/vector-ui/target/$TARGET/release/forma" "$APP_DIR/forma"
cat > "$APP_DIR/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>ru</string>
    <key>CFBundleDisplayName</key>
    <string>Forma</string>
    <key>CFBundleExecutable</key>
    <string>forma</string>
    <key>CFBundleIdentifier</key>
    <string>dev.forma.app</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Forma</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSRequiresIPhoneOS</key>
    <true/>
    <key>UIDeviceFamily</key>
    <array>
        <integer>1</integer>
        <integer>2</integer>
    </array>
    <key>UIRequiredDeviceCapabilities</key>
    <array>
        <string>arm64</string>
        <string>metal</string>
    </array>
    <key>UISupportedInterfaceOrientations</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>CADisableMinimumFrameDurationOnPhone</key>
    <true/>
    <key>UILaunchScreen</key>
    <dict/>
</dict>
</plist>
EOF
codesign --force --sign - "$APP_DIR"

echo "[3/3] Запуск Simulator.app ($DEVICE) и установка Forma.app..."
open -a "$DEVELOPER_DIR/Applications/Simulator.app"
xcrun simctl boot "$DEVICE" 2>/dev/null || true
xcrun simctl bootstatus "$DEVICE" -b
BOOTED_UDIDS="$(xcrun simctl list devices booted | grep -Eo '[0-9A-F]{8}-([0-9A-F]{4}-){3}[0-9A-F]{12}' || true)"
if [ -n "$BOOTED_UDIDS" ]; then
  for UDID in $BOOTED_UDIDS; do
    xcrun simctl terminate "$UDID" dev.forma.app 2>/dev/null || true
    xcrun simctl uninstall "$UDID" dev.forma.app 2>/dev/null || true
    xcrun simctl install "$UDID" "$APP_DIR"
    xcrun simctl launch "$UDID" dev.forma.app "${BACKEND_ARGS[@]}"
  done
else
  xcrun simctl terminate "$DEVICE" dev.forma.app 2>/dev/null || true
  xcrun simctl uninstall "$DEVICE" dev.forma.app 2>/dev/null || true
  xcrun simctl install "$DEVICE" "$APP_DIR"
  xcrun simctl launch "$DEVICE" dev.forma.app "${BACKEND_ARGS[@]}"
fi
echo "Готово! Forma запущена в iOS Simulator ($DEVICE)."
