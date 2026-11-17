#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ]; then
    echo "macOS에서 실행하세요." >&2
    exit 1
fi
cargo build --locked --release
mkdir -p dist/Boorstar.app/Contents/MacOS
cp target/release/boorstar dist/Boorstar.app/Contents/MacOS/Boorstar
cat > dist/Boorstar.app/Contents/Info.plist <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Boorstar</string>
<key>CFBundleIdentifier</key><string>dev.boorstar.desktop</string>
<key>CFBundleName</key><string>Boorstar</string>
<key>CFBundleDisplayName</key><string>Boorstar</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --deep --sign - dist/Boorstar.app
echo "앱 생성: $(pwd)/dist/Boorstar.app"
