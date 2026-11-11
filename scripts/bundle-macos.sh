#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != Darwin ]]; then
  echo 'Build the macOS app bundle on macOS.' >&2
  exit 1
fi
profile="${1:-release}"
case "$profile" in
  dev)
    output=debug
    cargo build --locked --profile dev
    ;;
  release)
    output=macos-dist
    # The executable calls its native code directly and needs no public exports.
    cargo rustc --locked --profile macos-dist --bin spotify-ferro -- -C link-arg=-Wl,-no_exported_symbols
    ;;
  *) echo 'Usage: scripts/bundle-macos.sh [dev|release]' >&2; exit 1 ;;
esac

target="${CARGO_TARGET_DIR:-target}"
bundle="dist-desktop/Zunify.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$target/$output/spotify-ferro" "$bundle/Contents/MacOS/zunify.new"
if [[ "$profile" == release ]]; then
  xcrun strip -u -r "$bundle/Contents/MacOS/zunify.new"
fi
mv -f "$bundle/Contents/MacOS/zunify.new" "$bundle/Contents/MacOS/zunify"

icons="$target/Zunify.iconset"
mkdir -p "$icons"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" wwwroot/icon-512.png --out "$icons/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" wwwroot/icon-512.png --out "$icons/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil --convert icns --output "$bundle/Contents/Resources/Zunify.icns" "$icons"

cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>zunify</string>
  <key>CFBundleIdentifier</key><string>org.spotife.app</string>
  <key>CFBundleName</key><string>Zunify</string>
  <key>CFBundleDisplayName</key><string>Zunify</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleIconFile</key><string>Zunify.icns</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --sign - "$bundle"
printf '%s/%s\n' "$PWD" "$bundle"
