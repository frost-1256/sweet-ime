#!/bin/sh
# Downloads a zenz model (by Miwa Keita, CC BY-SA 4.0) from Hugging Face and
# converts it for Sweet IME. Optional: without it Sweet IME converts with the
# dictionary lattice only.
#
#   get-zenz.sh [model] [output] [file]
#
# model:  Miwa-Keita/zenz-v3.1-xsmall (default, ~100 MB, fastest) or
#         Miwa-Keita/zenz-v3.1-small (~380 MB, more accurate, ~4x slower)
# output: the Sweet IME data directory for the current platform
# file:   a GPT-2 .gguf file in the repository to convert instead of the
#         safetensors model, such as ggml-model-Q8_0.gguf
set -eu

model=${1:-Miwa-Keita/zenz-v3.1-xsmall}
file=${3:-}
case $(uname -s) in
    Darwin) data="$HOME/Library/Application Support/SweetIME" ;;
    *) data="${XDG_DATA_HOME:-$HOME/.local/share}/sweet-ime" ;;
esac
out=${2:-"$data/zenz.swz"}

here=$(cd "$(dirname "$0")" && pwd)
tool=
for t in "$here/../MacOS/sweet-zenz-pack" "$here/../../bin/sweet-zenz-pack" "$here/../target/release/sweet-zenz-pack" "$(command -v sweet-zenz-pack || true)"; do
    if [ -n "$t" ] && [ -x "$t" ]; then
        tool=$t
        break
    fi
done
if [ -z "$tool" ]; then
    echo "sweet-zenz-pack not found" >&2
    exit 1
fi

echo "The model is licensed under CC BY-SA 4.0:"
echo "  https://huggingface.co/$model"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
if [ -n "$file" ]; then
    case $file in
        *[!A-Za-z0-9._-]* | .gguf) echo "not a .gguf file name: $file" >&2; exit 1 ;;
        *.gguf) ;;
        *) echo "not a .gguf file name: $file" >&2; exit 1 ;;
    esac
    files=$file
    source="$tmp/$file"
else
    files="config.json vocab.json merges.txt model.safetensors"
    source=$tmp
fi
for f in $files; do
    echo "downloading $f"
    curl -fsSL --retry 3 -o "$tmp/$f" "https://huggingface.co/$model/resolve/main/$f"
done
mkdir -p "$(dirname "$out")"
"$tool" "$source" "$out.new"
mv -f "$out.new" "$out"
echo "installed $out"
