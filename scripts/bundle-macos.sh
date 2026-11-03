#!/usr/bin/env bash
# Build a double-clickable macOS .app bundle from a Rust binary.
#
#   ./scripts/bundle-macos.sh        # the egui build  -> dist/Amigo.app
#   ./scripts/bundle-macos.sh gpui   # the GPUI port   -> dist/Amigo GPUI.app
#
# Both bundles use the same icon and the same storage directory, so they can be
# compared side by side while the port is finished.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET="${1:-egui}"
case "$TARGET" in
  egui)
    PACKAGE="amigo"
    BIN="amigo"
    APP_NAME="Amigo"
    BUNDLE_ID="com.stephen.amigo"
    ;;
  gpui)
    PACKAGE="amigo-gpui"
    BIN="amigo-gpui"
    APP_NAME="Amigo GPUI"
    BUNDLE_ID="com.stephen.amigo.gpui"
    ;;
  *)
    echo "usage: $0 [egui|gpui]" >&2
    exit 1
    ;;
esac

cargo build --release -p "$PACKAGE"

APP_DIR="$ROOT/dist/$APP_NAME.app"
CONTENTS="$APP_DIR/Contents"

rm -rf "$APP_DIR"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"

cp "$ROOT/target/release/$BIN" "$CONTENTS/MacOS/$BIN"

python3 "$ROOT/scripts/make_icons.py"
cp "$ROOT/assets/AppIcon.icns" "$CONTENTS/Resources/AppIcon.icns"

cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>$BIN</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

# Ad-hoc signature so macOS treats the bundle as a stable app.
codesign --force --deep --sign - "$APP_DIR" >/dev/null 2>&1 || true

echo "Built $APP_DIR"
