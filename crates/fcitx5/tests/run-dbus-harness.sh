#!/bin/sh
# Runs dbus_harness.py against a private fcitx5 (own D-Bus session, own
# config and data directories) with the built addon.
#
#   crates/fcitx5/tests/run-dbus-harness.sh [package dir, default ./result]
#
# Needs fcitx5, dbus-run-session and python3 with dbus-python and PyGObject,
# e.g. inside `nix develop` or:
#   nix shell nixpkgs#fcitx5 --impure --expr \
#     'with import <nixpkgs> {}; python3.withPackages (p: [p.dbus-python p.pygobject3])'
set -eu

pkg=$(readlink -f "${1:-./result}")
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/config/fcitx5" "$tmp/data"
mkdir -p "$tmp/config/sweet-ime"
cat > "$tmp/config/sweet-ime/keymap.txt" <<KEYMAP
#! version 3
Kanji ime_toggle
LeftAlt ime_off
RightAlt ime_on
Enter commit
Escape cancel
Space convert
Tab suggest
F7 katakana
KEYMAP
cat > "$tmp/config/fcitx5/profile" <<PROFILE
[Groups/0]
Name=Default
Default Layout=us
DefaultIM=sweetime

[Groups/0/Items/0]
Name=keyboard-us
Layout=

[Groups/0/Items/1]
Name=sweetime
Layout=

[GroupOrder]
0=Default
PROFILE

export XDG_CONFIG_HOME="$tmp/config" XDG_DATA_HOME="$tmp/data"
export FCITX_ADDON_DIRS="$pkg/lib/fcitx5${FCITX_ADDON_DIRS:+:$FCITX_ADDON_DIRS}"
export XDG_DATA_DIRS="$pkg/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
unset WAYLAND_DISPLAY DISPLAY

set --
if [ -n "${SWEET_TEST_DBUS_CONFIG:-}" ]; then
    set -- "--config-file=$SWEET_TEST_DBUS_CONFIG"
fi
dbus-run-session "$@" -- sh -c '
    fcitx5 --disable=wayland,waylandim,xim,kimpanel,notificationitem,classicui >"$1/fcitx5.log" 2>&1 &
    pid=$!
    python3 "$2/dbus_harness.py"
    status=$?
    kill $pid 2>/dev/null
    wait "$pid" 2>/dev/null || :
    [ $status -eq 0 ] || { echo "--- fcitx5 log"; tail -40 "$1/fcitx5.log"; }
    exit $status
' sh "$tmp" "$here"
