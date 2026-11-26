<p align="center">
  <img src="assets/icon/sweet-ime-128.png" width="96" height="96" alt="Sweet IME アイコン">
</p>

<h1 align="center">Sweet IME</h1>

<p align="center">
  Rust でゼロから書いた、高速・プライバシー重視の日本語入力（IME）<br>
  <b>Windows</b>・<b>Linux (fcitx5)</b>・<b>macOS</b>・<b>Android</b> 対応
</p>

<p align="center">
  <a href="https://github.com/frost-1256/sweet-ime/releases/latest"><img src="https://img.shields.io/github/v/release/frost-1256/sweet-ime?style=flat-square" alt="Latest Release"></a>
  <a href="https://github.com/frost-1256/sweet-ime/actions/workflows/windows.yml"><img src="https://img.shields.io/github/actions/workflow/status/frost-1256/sweet-ime/windows.yml?branch=main&style=flat-square&label=CI" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/frost-1256/sweet-ime?style=flat-square" alt="License"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <b>日本語</b>
</p>

---

## 特徴

- **超高速な辞書変換**: Mozc OSS 辞書＋SudachiDict 語彙のラティス探索（Viterbi / N-best）により、**約 0.1 ms** で変換します。
- **1桁 ms 台のオンデバイス AI 変換（任意）**: [zenz](https://huggingface.co/Miwa-Keita/zenz-v3.1-xsmall)（GPT-2）モデルを Rust ネイティブの AVX2 / NEON カーネルと KV キャッシュ共有ツリーアテンションで推論し、カーソル前の文脈を踏まえて **約 6 ms**（`xsmall`）で候補を並べ替えます。ライブ変換中はバックグラウンドで動くため入力を待たせません。
- **ふだんの入力が快適**: ライブ変換、文の途中からの予測変換、ローマ字の打ち間違い補正、文中の英単語変換（`korehabestdesu` → `これはベストです`）、かな数字変換（`さんぜんえん` → `三千円` / `3000円`）、日付・時刻・計算候補（学習履歴を汚さないよう自動除外）、文節リサイズ、F6〜F10 文字種変換。
- **自由度の高いテーマ機能**: システム配色に追従する標準テーマや 8 種のプリセットに加え、配色と**背景透過**を自由に設定できる**ユーザーテーマ**と**名前付き保存機能**を搭載。
- **プライバシー重視**: 変換・学習・設定はすべて端末内で完結します。パスワード欄やシークレットウィンドウでは学習しません（Windows / Android）。

## 対応プラットフォーム

| 環境 | 配布ファイル | 特徴 |
|---|---|---|
| **Windows** | `SweetIME-Setup.exe` | 64bit / 32bit 対応 TSF、Windows のライト/ダーク・アクセントカラー追従 Direct2D 候補ウィンドウ、透過ユーザーテーマ、ゲーミング枠線、「あ」/「A」バッジ、アプリ再起動不要のホットアップデート |
| **Linux (fcitx5)** | `SweetIME-Linux-{x86_64,aarch64}.tar.gz` / Nix Flake | Wayland（`input-method-v2`: niri / Sway / Hyprland 等）・X11 対応 fcitx5 アドオン、透過＋ブラー対応 classicui テーマ、文字サイズ・余白・縦横切替対応 |
| **macOS** | `SweetIME-macOS.zip` | Universal（Apple Silicon / Intel）InputMethodKit IME（macOS 15 以降）、コンパクト候補ウィンドウ、共通設定アプリ、アプリ内アップデート |
| **Android** | `SweetIME-Android.apk` | QWERTY & 12キーかな・英数フリックキーボード、Material 3 / Expressive 壁紙カラー、カスタム TTF/OTF フォント、端末内 AI 変換（Android 10 以上） |

## 変換精度・速度

Mozc `evaluation.tsv`（397 件）での 1 位正解率と Windows 上での平均変換時間:

| 構成 | 1 位正解率 | 平均変換時間 |
|---|---|---|
| ラティスのみ | 76.8% | 約 0.1 ms |
| **+ zenz v3.1 xsmall（既定）** | **89.7%** | **約 6.0 ms** |
| + zenz v3.1 small | 89.7% | 約 25 ms |
| Mozc OSS（参考） | 74.6% | — |

[AJIMEE-Bench](https://github.com/azooKey/AJIMEE-Bench) の評価も `sweet-cli data/system.dict eval-ajimee <evaluation_items.json>`（`SWEET_ZENZ` 指定可）で実行できます。

---

## インストール

**[最新リリース](https://github.com/frost-1256/sweet-ime/releases/latest)** からお使いの環境向けパッケージをダウンロードしてください。

### Windows
`SweetIME-Setup.exe` を実行します。`%ProgramFiles%\SweetIME` にインストールし、64bit / 32bit の IME を登録します（日本語キーボード一覧への追加や AI モデルのダウンロードも選択可能）。
- 設定アプリ起動時およびサインイン時（1日1回まで）に GitHub Releases を確認し、SHA-256 検証付きで自動更新できます（**設定 → 情報** でオフ可能）。
- `SweetIME-Setup.exe --preview` で変更せずに画面を確認できます。

### Linux (fcitx5)

**方法 A — ビルド済みバイナリ（Ubuntu / Arch / Fedora など）:**
最新リリースから `SweetIME-Linux-x86_64.tar.gz` または `SweetIME-Linux-aarch64.tar.gz` をダウンロードし、同梱の `install.sh` を実行します。

```sh
tar -xzf SweetIME-Linux-x86_64.tar.gz
cd sweet-ime-linux-x86_64
sudo ./install.sh
```

fcitx5 を再起動（`fcitx5 -r`）し、`fcitx5-configtool` で **Sweet IME**（`sweetime`）を追加してください。
`--prefix <dir>`、`--addon-dir <dir>`、`--uninstall` オプションにも対応しています。

**方法 B — Nix Flake / NixOS:**
公開 Cachix キャッシュ（`https://sweet-ime.cachix.org`）から `x86_64-linux` / `aarch64-linux` のテスト済みビルドを取得できます。

```nix
# flake.nix inputs
sweet-ime.url = "github:frost-1256/sweet-ime";

# NixOS / Home Manager
i18n.inputMethod.fcitx5.addons = [ inputs.sweet-ime.packages.${pkgs.system}.default ];
```

<details>
<summary>Cachix 設定・CMake での手動ビルド手順</summary>

NixOS 以外での Cachix 利用:
```sh
cachix use sweet-ime
nix build github:frost-1256/sweet-ime
```

NixOS での Cachix 設定:
```nix
nix.settings = {
  extra-substituters = [ "https://sweet-ime.cachix.org" ];
  extra-trusted-public-keys = [
    "sweet-ime.cachix.org-1:KrnImhtgcdSD1qlYusdBprFJWl/AsgL24H+QUwDZbIY="
  ];
};
```

CMake での手動ビルド（Rust・CMake・`libfcitx5core-dev` が必要）:
```sh
git clone --filter=blob:none --no-checkout https://github.com/google/mozc third_party/mozc-data
git -C third_party/mozc-data sparse-checkout set src/data/dictionary_oss
git -C third_party/mozc-data checkout 1527a4cd1217cec4611cd4bc8d46f8c66ea79e6a
cmake -S crates/fcitx5 -B build -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build && sudo cmake --install build
cargo build --release -p sweet-dictc
target/release/sweet-dictc third_party/mozc-data/src/data/dictionary_oss system.dict
sudo install -Dm644 system.dict /usr/share/sweet-ime/system.dict
```
</details>

### macOS
`SweetIME-macOS.zip` を展開し、**Install Sweet IME.command** を開きます（または `Sweet IME.app` を `~/Library/Input Methods` へコピー）。ログインし直したあと、**システム設定 → キーボード → テキスト入力 → 編集 → 日本語** から **Sweet IME** を追加してください。

ターミナルからのインストール・更新も可能です:
```sh
installer=$(mktemp -t sweet-ime-install) &&
  /usr/bin/curl -qfsSL https://raw.githubusercontent.com/frost-1256/sweet-ime/main/macos/install.sh -o "$installer" &&
  /bin/bash "$installer"
```

### Android
`SweetIME-Android.apk` をインストールし、**Sweet IME** アプリからキーボードを有効にして切り替えます。辞書は同梱されています。

### AI 変換モデル（任意）
zenz モデルは CC BY-SA 4.0 ライセンスのため同梱していません。
- **Windows / Linux / macOS**: **設定 → AI 変換** からワンクリックでダウンロードできます（`zenz-v3.1-xsmall` / `small`）。
- **Android**: アプリ内の **AI 変換** からダウンロードできます（`xsmall` 約 100 MB、`small` 約 380 MB）。

---

## 使い方・機能

### 既定のキー割り当て

| キー | 入力中 | 変換中 |
|---|---|---|
| `Space` | 変換 | 次の候補 / 候補一覧 |
| `Enter` | 確定（ライブ変換結果） | 確定 |
| `Tab` | 予測候補から選択 | — |
| `Esc` | 読みに戻す → もう一度で取り消し | 読みに戻す |
| `←` / `→` | カーソル移動 | 文節移動 |
| `Shift+←` / `Shift+→` | — | 文節を縮める / 伸ばす |
| `1`〜`9` | — | 候補を選択 |
| `F6`〜`F10` | ひらがな / カタカナ / 半角カタカナ / 全角英数 / 半角英数 | 同じ |
| `半角/全角`・`Alt+\``・`変換`・`無変換` | 切替 / オン / オフ | — |
| `Ctrl+Backspace` | 確定直後に読みへ戻す（Windows） | — |
| `Ctrl+F12` | 確定直後の変換を報告（Windows） | — |

設定アプリの **キー** ページ（`keymap.txt`）で自由にカスタマイズできます。

### 日本語の中での英語入力
- **Shift で英単語を開始**: `Shift+英字` で打ち始め、そのまま入力できます（`Space` でスペース付き確定、`Enter` で単語確定、`Esc` で取消）。知っている単語は後ろに日本語が続くと自動で区切られます（`Googlehakensaku` → `Googleは検索`）。
- **文中の英単語・技術用語**: `korehabestdesu` → `これはベストです`、`ime` / `github` → `IME` / `GitHub`。
- **URL・メールアドレス**: 日本語モードのまま `https://...`、`dev@example.com`、`localhost:3000` を半角で確定できます。

### テーマとカスタマイズ
**設定 → テーマ**（Windows / Linux）で見た目を細かく調整できます:
- 9 種の既存テーマに加え、配色・**背景の不透明度（35%〜100%）**を自由に調整できる**ユーザーテーマ**を作成可能。
- 作ったユーザーテーマに名前を付けて複数保存（`user_themes.txt`）し、プレビュータイルからワンクリックで切り替えられます。
- 候補ウィンドウのフォント変更に対応。Linux（fcitx5）では **候補の文字サイズ**・**縦/横並び切替**・**パネルの余白** も設定アプリから変更できます。
- Windows では **ゲーミングモード**（虹色アニメーション枠線）も選択できます。

<details>
<summary>設定ファイル一覧・<code>config.txt</code> リファレンス</summary>

| ファイル | Windows (`%APPDATA%\SweetIME\`) | Linux (`$XDG_CONFIG_HOME/sweet-ime/`) |
|---|---|---|
| `config.txt` | 一般設定 | 一般設定 |
| `keymap.txt` | キー割り当て | キー割り当て |
| `user_theme.txt` | 現在のユーザーテーマ | 現在のユーザーテーマ |
| `user_themes.txt` | 名前付き保存ユーザーテーマ | 名前付き保存ユーザーテーマ |
| `history.tsv` | 学習履歴 | 学習履歴 |
| `user_dict.tsv` | ユーザー辞書（Mozc / MS IME 形式インポート対応） | ユーザー辞書 |
| `reports.tsv` | 誤変換レポート | 誤変換レポート |
| `apps.txt` | アプリ別初期モード | — |
| `zenz.swz` | `%LOCALAPPDATA%\SweetIME\` | `$XDG_DATA_HOME/sweet-ime/` |

`config.txt` の例:
```ini
live_conversion = on   # ライブ変換
suggestions = on       # 予測変換
show_typed_input = off # 打ったローマ字を近くに表示 (Windows)
neural = on            # zenz モデルによる並べ替え
learning = on          # 選んだ候補の学習
theme = default        # default, light, dark, sakura, matcha, midnight, lavender, dracula, adwaita, custom
gaming_mode = off      # 虹色アニメーション枠線 (Windows)
update_check = on      # GitHub Releases の更新確認
dates = on             # きょう / いま -> 日付・時刻候補
emoji = on             # えがお -> 😊, かお -> 顔文字
calculator = on        # 1+2*3= -> 7
undo_commit = on       # 確定直後の Ctrl+Backspace で読みに戻す
private_fields = on    # パスワード欄・シークレットウィンドウで学習しない
english_words = on     # 文中の英単語変換
english_katakana = on  # カタカナ語を優先表示 (best -> ベスト)
shift_one_word = on    # Shift 英字入力を 1 単語で日本語へ戻す
```
</details>

<details>
<summary>Android キーボードの詳細</summary>

- **QWERTY** と **12キーかな・英数フリック入力** に対応（[フリック配列ガイド](assets/android/flick-guide.png)）。
- キーの高さ、数字行、バイブ・音、Material 3 / Expressive テーマ、カスタム TTF/OTF フォントを設定可能。
- **Simejiクラウド変換**（既定オフ、パスワード欄・シークレット欄では無効。[Simeji プライバシーポリシー](https://simeji.me/privacy/)）。
</details>

---

## 開発・ビルド

<details>
<summary>ソースからのビルドとテスト</summary>

### 必要なもの
- Stable Rust（Edition 2024）
- `third_party/mozc-data` に Mozc OSS 辞書（リビジョンは `.github/workflows/windows.yml` に固定）

### Windows
```powershell
rustup target add i686-pc-windows-msvc
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```
`dist\` とインストーラー `out\SweetIME-Setup.exe` を生成します。辞書のみ作成する場合は `./scripts/build-dictionary.ps1 -Output data/system.dict` を実行します。

### Linux
```sh
# Nix ビルド + D-Bus テストハーネス
nix build
nix develop --command sh crates/fcitx5/tests/run-dbus-harness.sh result

# ポータブルバイナリアーカイブ生成
bash scripts/package-linux.sh x86_64 out/SweetIME-Linux-x86_64.tar.gz
```

### macOS
```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pwsh -File scripts/build-dictionary.ps1 -Output data/system.dict
bash macos/build.sh
```

### Android
```powershell
powershell -ExecutionPolicy Bypass -File android/build.ps1 -Install
```
Android SDK（`platforms;android-36`, `build-tools;36.0.0`, NDK）、JDK 17+、`cargo-ndk`、`aarch64-linux-android` / `x86_64-linux-android` ターゲットが必要です。

### テストと評価ツール
```sh
cargo test --locked --workspace --all-targets -- --test-threads=1
```
- **Windows TSF ハーネス**: `cargo run --locked --release -p sweet-tsf --example harness`
- **精度・速度評価**: `sweet-cli data/system.dict eval <evaluation.tsv>`（`SWEET_ZENZ` 指定で AI 変換込み評価）
</details>

## ライセンス

- **ソースコード**: MIT（[LICENSE](LICENSE)）
- **システム辞書（`system.dict`）**: Mozc OSS 辞書（IPAdic・沖縄辞書を含む）および [SudachiDict](https://github.com/WorksApplications/SudachiDict)（Apache-2.0、[assets/licenses/sudachidict.txt](assets/licenses/sudachidict.txt)）を利用しています。
- **英単語リスト**: Kevin Atkinson 氏による SCOWL / ESDB（[assets/licenses/english-words.txt](assets/licenses/english-words.txt)）。
- **zenz モデル（任意・非同梱）**: Miwa Keita 氏による CC BY-SA 4.0 モデル。
