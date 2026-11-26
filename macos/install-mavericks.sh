#!/bin/bash
set -euo pipefail

fail() { printf '%s\n' "$*" >&2; exit 1; }
[[ $(uname -s) == Darwin && $(uname -m) == x86_64 ]] || fail 'Intel Mac が必要です。'
[[ $EUID != 0 ]] || fail 'ログインしているユーザーで実行してください。'

source_dir=$(cd "$(dirname "$0")" && pwd)
app="$source_dir/Sweet IME.app"
methods="$HOME/Library/Input Methods"
destination="$methods/Sweet IME.app"
[[ -x "$app/Contents/MacOS/SweetIME" && -s "$app/Contents/Resources/system.dict" ]] || fail '同じフォルダーに Sweet IME.app が必要です。'
[[ ! -e $destination && ! -L $destination ]] || fail '既存の Sweet IME.app があります。退避してから実行してください。'
mkdir -p "$methods"
ditto "$app" "$destination"
printf '%s\n' 'インストールしました。ログアウトして入り直し、システム環境設定の「キーボード」→「入力ソース」から Sweet IME を追加してください。'
