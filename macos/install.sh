#!/bin/bash
set -euo pipefail

fail() { printf '%s\n' "$*" >&2; exit 1; }

[[ $(/usr/bin/uname -s) == Darwin ]] || fail "このインストーラーは macOS 用です。"
os_version=$(/usr/bin/sw_vers -productVersion)
[[ ${os_version%%.*} -ge 15 ]] || fail "macOS 15 以降が必要です。"
[[ $EUID -ne 0 ]] || fail "ログインしているユーザーで実行してください。"

app_name='Sweet IME.app'
methods=${SWEET_IME_INPUT_METHODS_DIR:-"$HOME/Library/Input Methods"}
[[ $methods == /* && $methods != / ]] || fail "インストール先は絶対パスで指定してください。"
target="$methods/$app_name"
bundle_id=io.github.sweetime.inputmethod.japanese
release_url=https://github.com/frost-1256/sweet-ime/releases/latest/download/SweetIME-macOS.zip

source_bundle=
archive=
case $# in
    0)
        if [[ -n ${BASH_SOURCE[0]:-} ]]; then
            nearby="$(/usr/bin/dirname "${BASH_SOURCE[0]}")/$app_name"
            [[ ! -d $nearby ]] || source_bundle=$nearby
        fi
        ;;
    2)
        case "$1" in
            --archive) archive=$2 ;;
            --bundle) source_bundle=$2 ;;
            *) fail "使い方: install.sh [--archive ZIP | --bundle APP]" ;;
        esac
        [[ -n $2 ]] || fail "インストールするファイルを指定してください。"
        ;;
    *) fail "使い方: install.sh [--archive ZIP | --bundle APP]" ;;
esac

/bin/mkdir -p "$methods"
work="$methods/.sweet-ime-install"
backup="$work/previous.app"
# Keep the same lock inode across installs. The kernel releases the lock
# when the last descriptor closes, including after a forced exit or reboot.
exec 9>>"$methods/.sweet-ime-install.lock"
/usr/bin/perl -MFcntl=:flock -e 'flock(STDIN, LOCK_EX | LOCK_NB) or exit 1' <&9 \
    || fail "別のインストールが実行中か、ロックを取得できません。"

plist_id() {
    /usr/bin/plutil -extract CFBundleIdentifier raw -o - "$1/Contents/Info.plist"
}

check_existing_app() {
    [[ -d $1 && ! -L $1 ]] || fail "アプリ以外のファイルがあります: $1"
    local id
    id=$(plist_id "$1") || fail "既存のアプリの識別子を確認できません: $1"
    case "$id" in
        "$bundle_id"|io.github.sweetime.inputmethod) ;;
        *) fail "別のアプリがあります: $1" ;;
    esac
}

if [[ -e $target || -L $target ]]; then
    check_existing_app "$target"
fi
if [[ -e $work || -L $work ]]; then
    [[ -d $work && ! -L $work ]] || fail "一時フォルダーを確認できません: $work"
    if [[ -e $backup || -L $backup ]]; then
        check_existing_app "$backup"
        if [[ ! -e $target ]]; then
            /bin/mv "$backup" "$target"
            printf '%s\n' '中断された更新から以前の Sweet IME を復元しました。'
        fi
    fi
    /bin/rm -rf "$work"
fi
/bin/mkdir "$work"
installed=false
cleanup() {
    if [[ $installed == false && -e $backup ]]; then
        if [[ -e $target || -L $target ]] || ! /bin/mv "$backup" "$target"; then
            printf '以前のアプリを %s に保存しました。\n' "$backup" >&2
            return
        fi
    fi
    /bin/rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [[ -z $source_bundle ]]; then
    if [[ -z $archive ]]; then
        archive="$work/SweetIME-macOS.zip"
        printf '%s\n' 'GitHub から最新版をダウンロードしています…'
        /usr/bin/curl -q --fail --location --proto '=https' --proto-redir '=https' \
            --connect-timeout 15 --max-time 900 --output "$archive" "$release_url"
    fi
    [[ $archive == /* ]] || archive="$PWD/$archive"
    /usr/bin/ditto -x -k "$archive" "$work/unpacked"
    source_bundle="$work/unpacked/$app_name"
fi

[[ -d $source_bundle && ! -L $source_bundle ]] || fail "ZIP または指定先に Sweet IME.app が見つかりません。"
[[ $source_bundle == /* ]] || source_bundle="$PWD/$source_bundle"
incoming="$work/$app_name"
/usr/bin/ditto "$source_bundle" "$incoming"
[[ $(plist_id "$incoming") == "$bundle_id" ]] || fail "Sweet IME の識別子が一致しません。"
/usr/bin/codesign --verify --deep --strict "$incoming" \
    || fail "Sweet IME のコード署名を検証できませんでした。"
version=$(/usr/bin/plutil -extract CFBundleShortVersionString raw -o - "$incoming/Contents/Info.plist")

[[ ! -d $target ]] || /bin/mv "$target" "$backup"
/bin/mv "$incoming" "$target"
installed=true
printf 'Sweet IME %s をインストールしました。\n' "$version"
printf '%s\n' 'ログアウトして再ログインすると、新しいバージョンが読み込まれます。' \
    '初回のみ「システム設定 → キーボード → テキスト入力 → 編集」から Sweet IME を追加してください。'
