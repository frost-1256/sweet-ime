#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
if [[ $(uname -s) != Darwin ]]; then
    echo "Build with Xcode Command Line Tools on macOS 15 or later." >&2
    exit 1
fi
export MACOSX_DEPLOYMENT_TARGET=15.0
export CARGO_TARGET_DIR="$root/target"
dictionary=${1:-"$root/data/system.dict"}
mozc=${MOZC_DATA:-"$root/third_party/mozc-data/src/data/dictionary_oss"}
test -s "$dictionary"
test -f "$mozc/README.txt"

version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml)
bundle_version=${version%%[-+]*}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app="$work/Sweet IME.app"
resources="$app/Contents/Resources"
settings="$resources/Sweet IME Settings.app"
settings_resources="$settings/Contents/Resources"
mkdir -p "$app/Contents/MacOS" "$settings/Contents/MacOS" "$settings_resources/licenses"
cp macos/Info.plist "$app/Contents/Info.plist"
cp macos/Settings.plist "$settings/Contents/Info.plist"
for plist in "$app/Contents/Info.plist" "$settings/Contents/Info.plist"; do
    plutil -replace CFBundleVersion -string "$bundle_version" "$plist"
    plutil -replace CFBundleShortVersionString -string "$bundle_version" "$plist"
    plutil -lint "$plist"
done
cp "$dictionary" "$resources/system.dict"
cp LICENSE "$settings_resources/licenses/sweet-ime.txt"
cp "$mozc/README.txt" "$settings_resources/licenses/dictionary-oss.txt"
cp assets/licenses/{english-words,sudachidict}.txt "$settings_resources/licenses/"
cp scripts/get-zenz.sh "$settings_resources/"

iconset="$work/SweetIME.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
    cp "assets/icon/sweet-ime-${size}.png" "$iconset/icon_${size}x${size}.png"
    cp "assets/icon/sweet-ime-$((size * 2)).png" "$iconset/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$iconset" -o "$resources/SweetIME.icns"
cp "$resources/SweetIME.icns" "$settings_resources/"

for arch in arm64 x86_64; do
    rust_arch=$arch
    [[ $arch != arm64 ]] || rust_arch=aarch64
    target="$rust_arch-apple-darwin"
    cargo build --locked --release --target "$target" -p sweet-macos -p sweet-settings -p sweet-zenz
    binaries="$CARGO_TARGET_DIR/$target/release"
    xcrun clang -arch "$arch" -fobjc-arc -Wall -Wextra -Wno-unused-parameter \
        macos/main.m "$binaries/libsweet_macos.a" -o "$work/SweetIME-$arch" \
        -framework AppKit -framework Carbon -framework InputMethodKit \
        -liconv -lSystem -lc -lm
    cp "$binaries/sweet-settings" "$work/sweet-settings-$arch"
    cp "$binaries/sweet-zenz-pack" "$work/sweet-zenz-pack-$arch"
done

lipo -create "$work"/SweetIME-{arm64,x86_64} -output "$app/Contents/MacOS/SweetIME"
lipo -create "$work"/sweet-settings-{arm64,x86_64} -output "$settings/Contents/MacOS/sweet-settings"
lipo -create "$work"/sweet-zenz-pack-{arm64,x86_64} -output "$settings/Contents/MacOS/sweet-zenz-pack"
for exe in "$app/Contents/MacOS/SweetIME" "$settings/Contents/MacOS/"*; do
    chmod +x "$exe"
    lipo "$exe" -verify_arch arm64 x86_64
done

identity=${MACOS_CODESIGN_IDENTITY:--}
codesign --force --sign "$identity" "$settings/Contents/MacOS/sweet-zenz-pack"
codesign --force --sign "$identity" "$settings"
codesign --force --sign "$identity" "$app"
codesign --verify --deep --strict "$app"

mkdir -p "$root/out"
archive="$root/out/SweetIME-macOS.zip"
ditto -c -k --keepParent "$app" "$work/SweetIME-macOS.zip"
mv -f "$work/SweetIME-macOS.zip" "$archive"
printf '%s\n' "$archive"
