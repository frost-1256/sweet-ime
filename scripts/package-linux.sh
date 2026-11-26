#!/usr/bin/env bash
set -euo pipefail

# Packages Sweet IME for Linux (fcitx5 addon, settings app, dictionary, themes,
# icons, licenses, and an installer script) into a portable .tar.gz archive.
#
# Usage: scripts/package-linux.sh <arch> [output-tarball]
# Example: scripts/package-linux.sh x86_64 out/SweetIME-Linux-x86_64.tar.gz

ARCH="${1:-$(uname -m)}"
OUT="${2:-out/SweetIME-Linux-${ARCH}.tar.gz}"
PREFIX="/usr/local"
BUILD_DIR="${BUILD_DIR:-target/fcitx5-cmake}"
STAGE_DIR="$(mktemp -d)"
trap 'rm -rf "$STAGE_DIR"' EXIT

PKG_ROOT="$STAGE_DIR/sweet-ime-linux-${ARCH}"
mkdir -p "$PKG_ROOT" "$(dirname "$OUT")"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"

# Build system.dict if not already present (e.g. restored from CI cache).
if [ ! -f data/system.dict ]; then
    echo "Building data/system.dict..."
    cargo build --release --locked --bin sweet-dictc
    SUDACHI_VER="20261015"
    SUDACHI_DIR="$STAGE_DIR/sudachidict"
    mkdir -p "$SUDACHI_DIR"
    for name in small_lex core_lex notcore_lex; do
        curl -fsSL "https://sudachi.s3.ap-northeast-1.amazonaws.com/sudachidict-raw/v1/${SUDACHI_VER}/${name}.zip" \
            -o "$STAGE_DIR/${name}.zip"
        unzip -q "$STAGE_DIR/${name}.zip" -d "$SUDACHI_DIR"
    done
    mkdir -p data
    "$CARGO_TARGET_DIR/release/sweet-dictc" \
        third_party/mozc-data/src/data/dictionary_oss \
        data/system.dict \
        "$SUDACHI_DIR"
fi

# Build the fcitx5 addon via CMake.
cmake -S crates/fcitx5 -B "$BUILD_DIR" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DCMAKE_INSTALL_LIBDIR=lib \
    -DSWEET_DATADIR="${PREFIX}/share/sweet-ime" \
    -DSWEET_SETTINGS="${PREFIX}/bin/sweet-settings"
cmake --build "$BUILD_DIR" -j"$(nproc)"
DESTDIR="$PKG_ROOT" cmake --install "$BUILD_DIR"

# Flatten DESTDIR/$PREFIX into $PKG_ROOT/files so install.sh can target any prefix.
mkdir -p "$PKG_ROOT/files"
cp -a "$PKG_ROOT/${PREFIX#/}/." "$PKG_ROOT/files/"
rm -rf "$PKG_ROOT/usr"

# Build settings app and zenz packer.
cargo build --release --locked --bin sweet-settings --bin sweet-zenz-pack
install -Dm755 "$CARGO_TARGET_DIR/release/sweet-settings" "$PKG_ROOT/files/bin/sweet-settings"
install -Dm755 "$CARGO_TARGET_DIR/release/sweet-zenz-pack" "$PKG_ROOT/files/bin/sweet-zenz-pack"

# Dictionary, fcitx5 themes, desktop entry, helper script, licenses.
install -Dm644 data/system.dict "$PKG_ROOT/files/share/sweet-ime/system.dict"
"$CARGO_TARGET_DIR/release/sweet-settings" --write-fcitx-themes "$PKG_ROOT/files/share/fcitx5/themes"
install -Dm644 crates/settings/sweet-settings.desktop "$PKG_ROOT/files/share/applications/sweet-settings.desktop"
install -Dm755 scripts/get-zenz.sh "$PKG_ROOT/files/share/sweet-ime/get-zenz.sh"
install -Dm644 LICENSE "$PKG_ROOT/files/share/sweet-ime/licenses/sweet-ime.txt"
install -Dm644 assets/licenses/english-words.txt "$PKG_ROOT/files/share/sweet-ime/licenses/english-words.txt"
install -Dm644 assets/licenses/sudachidict.txt "$PKG_ROOT/files/share/sweet-ime/licenses/sudachidict.txt"
if [ -f third_party/mozc-data/src/data/dictionary_oss/README.txt ]; then
    install -Dm644 third_party/mozc-data/src/data/dictionary_oss/README.txt \
        "$PKG_ROOT/files/share/sweet-ime/licenses/dictionary-oss.txt"
fi

cat > "$PKG_ROOT/install.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

PREFIX="/usr/local"
CUSTOM_PREFIX=0
UNINSTALL=0

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            PREFIX="$2"
            CUSTOM_PREFIX=1
            shift 2
            ;;
        --uninstall)
            UNINSTALL=1
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [--prefix /usr/local] [--uninstall]"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ADDON_DIR=""
if [ "$CUSTOM_PREFIX" -eq 0 ] && command -v pkg-config >/dev/null 2>&1; then
    ADDON_DIR="$(pkg-config --variable=addondir Fcitx5Core 2>/dev/null || true)"
fi
if [ -z "$ADDON_DIR" ]; then
    ADDON_DIR="${PREFIX}/lib/fcitx5"
fi

if [ "$UNINSTALL" -eq 1 ]; then
    rm -f "${ADDON_DIR}/sweetime.so" \
        "${PREFIX}/bin/sweet-settings" \
        "${PREFIX}/bin/sweet-zenz-pack" \
        "${PREFIX}/share/fcitx5/addon/sweetime.conf" \
        "${PREFIX}/share/fcitx5/inputmethod/sweetime.conf" \
        "${PREFIX}/share/applications/sweet-settings.desktop"
    rm -rf "${PREFIX}/share/sweet-ime" "${PREFIX}/share/fcitx5/themes"/sweet-ime-*
    echo "Sweet IME uninstalled from ${PREFIX} (addon dir: ${ADDON_DIR})."
    exit 0
fi

install -Dm755 "$SCRIPT_DIR/files/lib/fcitx5/sweetime.so" "${ADDON_DIR}/sweetime.so"
install -Dm755 "$SCRIPT_DIR/files/bin/sweet-settings" "${PREFIX}/bin/sweet-settings"
install -Dm755 "$SCRIPT_DIR/files/bin/sweet-zenz-pack" "${PREFIX}/bin/sweet-zenz-pack"
mkdir -p "${PREFIX}/share"
cp -a "$SCRIPT_DIR/files/share/." "${PREFIX}/share/"

echo "Sweet IME installed to ${PREFIX} (fcitx5 addon: ${ADDON_DIR}/sweetime.so)."
echo "Restart fcitx5 (fcitx5 -r) and add Sweet IME in fcitx5-configtool."
EOF
chmod +x "$PKG_ROOT/install.sh"

tar -C "$STAGE_DIR" -czf "$OUT" "$(basename "$PKG_ROOT")"
ls -lh "$OUT"
