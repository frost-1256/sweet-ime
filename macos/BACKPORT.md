# macOS 10.9 バックポート

Issue #27 `backport/macos-10.9` の起点は `v0.5.4` (`fdb3d84885fc5d9352afe8dcf8927ac581f39648`)。
このブランチは独立して保守し、main へのマージや main からの定期同期は行わない。
上流から取り込むのは対象に影響するセキュリティ修正のみ。修正コミットを確認し、
`git cherry-pick -x <commit>` で取り込む。依存する変更がある場合は修正に必要な差分を抽出し、元のコミットを記録する。

## 対象と検証

- Mac Pro Mid 2010、OS X 10.9 Mavericks、GTX 680 を対象とし、まず x86_64 の InputMethodKit 本体と辞書で検証する。
- 設定アプリを省いた構成では `~/Library/Application Support/SweetIME/config.txt` を UTF-8 の `key = value` 形式で編集する。キーと値は `crates/engine/src/session/options.rs` を参照する。
- 初回起動と基本変換を辞書だけで検証し、その後 zenz を測る。AVX2 と FMA を利用できない x86 向けのスカラー分岐は既存の `crates/zenz/src/kernels.rs` にある。
- 標準 Rust の x86_64 macOS 対応下限は 10.12。10.9にリンク・ロードできる Rust 標準ライブラリと依存ライブラリを先に検証する。現行の `macos/build.sh` は macOS 15 向けの配布ビルド。
- 最終成果物の Mach-O ロードコマンドと依存シンボルを `otool -l`、`otool -L`、`nm -u` で確認し、実機か同等の 10.9 環境で入力ソースの登録、再ログイン後の選択、ひらがな入力、候補表示、確定を確認する。

参考資料は [Rust の macOS 対応範囲](https://doc.rust-lang.org/rustc/platform-support/apple-darwin.html) と [Mavericks Porting Resources](https://github.com/wowfunhappy/mavericks-porting-resources)。
