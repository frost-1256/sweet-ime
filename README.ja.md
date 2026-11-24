<p align="center">
  <img src="assets/icon/sweet-ime-128.png" width="96" height="96" alt="">
</p>

<h1 align="center">Sweet IME</h1>

<p align="center">
  Rust でゼロから書いた日本語入力。<br>
  Windows・Linux(fcitx5)・Android で使えます。
</p>

<p align="center">
  <a href="README.md">English</a> · <b>日本語</b>
</p>

---

## 特徴

- **速い辞書変換**:Mozc OSS の辞書と連接コストに、SudachiDict の語彙を足した
  ラティスを、Viterbi と N-best 探索で変換します。1 回の変換は約 0.1 ms です。
- **AI による並べ替え(任意)**:zenz(GPT-2)モデルが、カーソルより前の文を見て
  上位の候補を並べ替えます。AVX2 カーネルと、KV キャッシュを共有するツリー
  アテンションで、約 15〜25 ms で動きます。ライブ変換中は裏で動くので、
  入力を待たせません。
- **ふだんの入力が楽**:ライブ変換、予測候補(文の途中でも)、ローマ字の打ち間違い
  補正、学習、ユーザー辞書、日本語の中の英単語、かなで読んだ数字
  (さんぜんえん → 三千円 / 3000円)、文節の伸び縮み、F6〜F10 の文字種変換。
- **データは手元に**:学習・報告・設定は端末の中だけに保存します。Windows と
  Android では、パスワード欄やシークレットウィンドウでは何も覚えません
  (Linux 版はまだこれらを見分けられません)。

| 環境 | できること |
|---|---|
| **Windows** | 64 ビット・32 ビットアプリ用の TSF テキストサービス、ライト/ダークとアクセント色に合わせる Direct2D の候補ウィンドウ、「あ」/「A」の入力モード表示 |
| **Linux** | `input-method-v2` に対応した Wayland コンポジター(niri、Sway、Hyprland など)や、fcitx5 のほかのフロントエンドで使える fcitx5 アドオン |
| **Android** | 候補バー付きの QWERTY ローマ字キーボード、Material 3 のテーマ、端末内で動く AI 変換(Android 10 以上) |

## 変換精度

Mozc の `evaluation.tsv`(397 件)での 1 位正解率です。

| 構成                          | 1 位正解率 | 平均時間 |
|-------------------------------|-----------|----------|
| ラティスのみ                  | 70.8%     | 約 0.1 ms |
| + zenz v3.1 xsmall(既定)    | 82.6%     | 約 13 ms  |
| + zenz v3.1 small             | 85.9%     | 約 41 ms  |
| Mozc OSS(参考)              | 74.6%     | —        |

## インストール

### Windows

[最新のリリース](https://github.com/frost-1256/sweet-ime/releases/latest)から
`SweetIME-Setup.exe` をダウンロードして実行します。管理者権限を求めたあと、
次のことをします。

- `%ProgramFiles%\SweetIME` にインストールする
- 64 ビット・32 ビットのアプリ用に IME を登録する
- 必要なら、日本語のキーボード一覧への追加、スタートメニューへの登録、
  AI モデルのダウンロードをする
- 「インストールされているアプリ」に登録する(ここからアンインストールできます)

`SweetIME-Setup.exe --preview` で、何も変更せずに画面だけを確認できます。

**更新**:設定アプリを開いたときと、サインインしたとき(1 日 1 回まで)に、
GitHub Releases の新しいバージョンを確認します。更新すると、
`SweetIME-Setup.exe` をダウンロードして SHA-256 をリリースと照合し、
`--update` を付けて実行します。確認を止めるには **設定 → 情報** を使うか、
`update_check = off` にします。

### Android

[最新のリリース](https://github.com/frost-1256/sweet-ime/releases/latest)から
`SweetIME-Android.apk` をダウンロードしてインストールします。**Sweet IME**
アプリを開いてキーボードを有効にし、キーボードの切り替えで選びます。
辞書はアプリに入っています。

### Linux(fcitx5)

flake で fcitx5 アドオン、システム辞書、設定アプリ、モデル変換ツールをビルドします。
公開キャッシュ `https://sweet-ime.cachix.org` では、CI でテスト済みの
`x86_64-linux` と `aarch64-linux` のビルドを配布しています。指定したリビジョンと
依存関係に一致するビルドがあれば Nix が取得し、なければローカルでビルドします。

NixOS 以外では、ビルド前に [Cachix CLI](https://docs.cachix.org/using-a-cache) で
キャッシュを有効にします。

```sh
cachix use sweet-ime
nix build github:frost-1256/sweet-ime
```

NixOS では、キャッシュの URL と署名用の公開鍵をシステム設定に追加し、
`sudo nixos-rebuild switch` で反映します。

```nix
nix.settings = {
  extra-substituters = [ "https://sweet-ime.cachix.org" ];
  extra-trusted-public-keys = [
    "sweet-ime.cachix.org-1:KrnImhtgcdSD1qlYusdBprFJWl/AsgL24H+QUwDZbIY="
  ];
};
```

NixOS や Home Manager では、fcitx5 のアドオンに追加します。

```nix
# flake inputs
sweet-ime.url = "github:frost-1256/sweet-ime";

# NixOS or Home Manager
i18n.inputMethod.fcitx5.addons = [ inputs.sweet-ime.packages.${pkgs.system}.default ];
```

そのあと、`~/.config/fcitx5/profile` や fcitx5 の設定で、入力メソッドの一覧に
**Sweet IME**(入力メソッド名 `sweetime`)を追加します。**設定** から
`sweet-settings` が開きます。

Nix を使わない場合は CMake でビルドします。Rust 部分は cargo で
ビルドするので、Rust、CMake、fcitx5 の開発用ファイルが必要です。辞書は、
CI と同じリビジョンの Mozc OSS のソースから作ります。

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

配布版と同じ語彙を入れるには、`nix/package.nix` にある SudachiDict の CSV
3 つを展開し、そのディレクトリを `sweet-dictc` の 3 つ目の引数に渡します。
できた辞書を配布するときは、`assets/licenses/sudachidict.txt` もほかの辞書の
表示と一緒に入れてください。

### AI 変換(任意)

zenz モデルは CC BY-SA 4.0 なので、同梱していません。モデルがないときは、
辞書のラティスだけで変換します。

- **Windows**:**設定 → AI 変換** からダウンロードします。スクリプトを自分で
  実行してもかまいません(管理者権限は不要です)。

  ```powershell
  powershell -ExecutionPolicy Bypass -File "$env:ProgramFiles\SweetIME\get-zenz.ps1"
  ```

  `%LOCALAPPDATA%\SweetIME\zenz.swz` に保存され、次に IME を使うときに
  読み込まれます。`-Model Miwa-Keita/zenz-v3.1-small` を付けると、大きくて
  より正確なモデルになります。
- **Linux**:**設定 → AI 変換** でダウンロードするか、Nix のパッケージの
  `share/sweet-ime/get-zenz.sh`(チェックアウトでは `scripts/get-zenz.sh`。
  CMake のインストールには入りません)を実行し、fcitx5 を再起動します。
  モデルは `$XDG_DATA_HOME/sweet-ime/zenz.swz` に置かれます。
- **Android**:アプリの **AI 変換** からダウンロードします(xsmall は約 100 MB、
  small は約 380 MB)。モデルはスマホの中で変換されます。ライブ変換は裏で
  手直しされ、xsmall なら最近のスマホで 1 文あたり約 0.2 秒です。

## 入力のしかた

### キー

| キー                     | 入力中                            | 変換中                           |
|--------------------------|-----------------------------------|----------------------------------|
| Space                    | 変換                              | 次の候補 / 候補一覧              |
| Enter                    | 確定(ライブ変換の結果)          | 確定                             |
| Tab                      | 予測候補から選ぶ                  | —                                |
| Esc                      | 読みに戻す、もう一度で取り消し    | 読みに戻す                       |
| ← / →                    | カーソル移動                      | 文節の移動                       |
| Shift+← / Shift+→        | —                                 | 文節を縮める / 伸ばす            |
| 1〜9                     | —                                 | 候補を選ぶ                       |
| F6 / F7 / F8 / F9 / F10  | ひらがな / カタカナ / 半角カタカナ / 全角英数 / 半角英数 | 同じ |
| 半角/全角 か Alt+`、変換、無変換 | 切り替え / オン / オフ    |                                  |
| Ctrl+Backspace           | 確定の直後:読みに戻す(Windows) |                                  |
| Ctrl+F12                 | 確定の直後:その変換を報告する(Windows) |                          |

これは既定の割り当てです。設定アプリの **キー** ページで変えられ、
`%APPDATA%\SweetIME\keymap.txt` に保存されます。割り当てごとに「いつでも」
「入力中だけ」「入力中以外」を選べるので、たとえば 変換 キーを、入力中は変換、
それ以外は IME オンにできます。IME をオン/オフすると、カーソルの横に
「あ」か「A」が少しだけ出ます。

### 日本語の中で英語を打つ

- **Shift で英単語を始める**:Shift+英字で打ち始め、Shift を離してそのまま
  続けます。Space で半角スペース付き、Enter で単語だけを確定し、どちらも
  日本語入力に戻ります。Esc で取り消します。
- **知っている単語は自動で区切る**:`Googlehakensaku` は `Google` を確定して
  `は検索` に続き、`Googleha` のあと Enter で `Googleは` になります
  (`shift_one_word = off` なら Space か Enter まで英語のままです)。
- **技術用語**:`ime`、`ieee`、`github` と打って Space で `IME`、`IEEE`、
  `GitHub` になります。`あいえむいー` のような読みでも変換できます。
- **よく使う英単語は文の中でも変換**:`korehabestdesu` は `これはベストです`
  になります。よく使うカタカナ語がある単語はカタカナを先に、つづりを次の候補に
  出します(`english_katakana = off` ならつづりのまま)。それ以外はつづりのまま
  (`software`)です。単語は同梱の SCOWL の一覧から来ていて、F10 で直した単語など、
  一覧にない単語も確定すると覚えます。`english_words = off` で止められます。

### Android のキーボード

デスクトップと同じく Shift で英単語を始められ、地球儀キーで英字の直接入力に
切り替わります。パスワード・URL・メール・数字の欄は英字で始まります。
q〜p の長押しで数字、**?123** で記号とテンキー、スペースキーを左右になぞると
カーソル移動です。候補バーの戻すボタンで、直前の確定を読みに戻せます。
アプリでは、変換とキーボードの設定(キーの高さ、数字の段、振動、音)と
テーマを選べます。テーマには、Android 12 以降で壁紙の色を使う Material 3 と
Material 3 Expressive、デスクトップと同じ配色があります。

## 設定

Windows では、設定アプリ `sweet-settings.exe` を次のどれかで開きます。

- スタートメニューの **Sweet IME の設定**
- タスクバーの「あ」/「A」を右クリック
- Windows の言語の設定:Sweet IME → キーボード オプション

変更はすぐ保存され、次にアプリにフォーカスが移ったときに反映されます。
設定アプリでは、次のことができます。

- ライブ変換・予測候補・「あ」/「A」表示のオン/オフ
- 候補ウィンドウの配色(Linux では fcitx5 のテーマとして)
- キーの割り当ての変更
- AI 変換のオン/オフと、zenz モデルのダウンロード・削除
- 学習のオン/オフ、覚えた単語の確認・削除、直近 1 時間や 1 日の学習の取り消し
- 自分の単語の登録・編集・インポート・エクスポート
- うまくいかなかった変換の報告

### ファイルの場所

| 内容 | Windows | Linux |
|---|---|---|
| 設定(`config.txt`) | `%APPDATA%\SweetIME\` | `$XDG_CONFIG_HOME/sweet-ime/` |
| キーの割り当て(`keymap.txt`) | `%APPDATA%\SweetIME\` | `$XDG_CONFIG_HOME/sweet-ime/` |
| 学習した単語(`history.tsv`) | `%APPDATA%\SweetIME\` | `$XDG_CONFIG_HOME/sweet-ime/` |
| ユーザー辞書(`user_dict.tsv`) | `%APPDATA%\SweetIME\` | `$XDG_CONFIG_HOME/sweet-ime/` |
| 変換の報告(`reports.tsv`) | `%APPDATA%\SweetIME\` | `$XDG_CONFIG_HOME/sweet-ime/` |
| アプリごとのモード(`apps.txt`) | `%APPDATA%\SweetIME\` | — |
| zenz モデル(`zenz.swz`) | `%LOCALAPPDATA%\SweetIME\` | `$XDG_DATA_HOME/sweet-ime/` |

Linux で `XDG_CONFIG_HOME` や `XDG_DATA_HOME` が設定されていないときは、
`~/.config` と `~/.local/share` を使います。

`config.txt` はこんな形です。

```
live_conversion = on   # 入力中に変換結果を表示する
suggestions = on       # 入力中に予測候補を出す
neural = on            # zenz モデルがあれば候補を並べ替える
learning = on          # 選んだ候補を覚える
theme = default        # default, light, dark, sakura, matcha, midnight, lavender, dracula, adwaita
update_check = on      # GitHub で新しいリリースを確認する
dates = on             # きょう / いま -> 日付と時刻
emoji = on             # えがお -> 😊、かお -> 顔文字
calculator = on        # 1+2*3= -> 7
undo_commit = on       # 確定直後の Ctrl+Backspace で読みに戻す
private_fields = on    # パスワード欄とシークレットウィンドウでは何も覚えない
english_words = on     # よく使う英単語を文の中でも変換する
english_katakana = on  # ……カタカナ語を先に出す(best -> ベスト)
shift_one_word = on    # Shift で始めた英単語を、日本語が続いたら区切る
```

- **アプリごとのモード**:`apps.txt` に 1 行 1 つ、
  `program.exe = normal | english | off` の形で書きます(ひらがなで始める、
  英字の直接入力で始める、そのアプリでは何もしない)。`* = english` の行を
  書くと、一覧にないアプリもすべて英字で始まります。設定の **アプリ** ページで
  編集できます。
- **ユーザー辞書**:`user_dict.tsv` に 1 行 1 語、
  `読み<TAB>単語<TAB>品詞<TAB>コメント` の形で書きます。Mozc や Google 日本語
  入力のエクスポートと同じ形式なので、それらのファイル(と Microsoft IME の
  エクスポート)をインポートできます。ファイルを変えるとすぐ反映されます。
- **変換の報告**:設定の **学習** ページ(Android では **学習とプライバシー**)で
  読みと期待する変換を追加するか、直して確定した直後に Ctrl+F12 を押します。
  報告は端末の中だけに保存され、**GitHub に送る** を押したときだけ、内容入りの
  GitHub の Issue 作成画面を開きます。

### Linux のテーマとキー

**テーマ** ページで選んだ配色は、fcitx5 の classicui テーマ
(`~/.local/share/fcitx5/themes/sweet-ime`)として書き出され、
`~/.config/fcitx5/conf/classicui.conf` で選ばれます。ほかの入力メソッドにも
この見た目が使われます。**fcitx5 の標準テーマに戻す** で fcitx5 のテーマに
戻ります。Home Manager で `classicui.conf` を管理している場合は、そちらで
`Theme = sweet-ime` を設定してください(既定のテーマでは `DarkTheme` に
`sweet-ime-dark`)。

キーの割り当てのファイルは Windows と共通です。日本語キーボードのキー
(半角/全角、変換、無変換、ひらがな、英数)も同じように使えます。fcitx5 自身の
切り替えキー(既定は Ctrl+Space)で、Sweet IME とキーボード配列を切り替えます。

## 開発

### Windows でのビルド

必要なもの:

- MSVC ツールチェーンの Rust stable
- 32 ビット DLL 用の `i686-pc-windows-msvc` ターゲット
- `third_party/mozc-data` に置いた Mozc OSS のソース
  (`.github/workflows/windows.yml` で固定しているリビジョン)

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```

`dist\` にファイルをまとめ、1 ファイルのインストーラー
`out\SweetIME-Setup.exe` を作ります。初回は、固定した SudachiDict の辞書を
ダウンロードして検証します。辞書だけを作るには
`./scripts/build-dictionary.ps1 -Output data/system.dict` を実行します。
開発版は、管理者の PowerShell で `scripts\install.ps1 -AddToLanguageList` と
`scripts\uninstall.ps1` を使って入れたり消したりできます。

### Android 版のビルド

`android/build.ps1` は Gradle を使わずに APK を作ります(cargo-ndk、aapt2、
javac、d8、apksigner)。`platforms;android-36`・`build-tools;36.0.0`・NDK の
入った Android SDK、JDK 17 以上、`cargo install cargo-ndk`、
`rustup target add aarch64-linux-android x86_64-linux-android`、
`third_party/mozc-data` の Mozc ソースが必要です。同梱する辞書は、Mozc と固定した
SudachiDict から作ります。

```powershell
powershell -ExecutionPolicy Bypass -File android/build.ps1 -Install
```

APK は `out/SweetIME-Android.apk` にできます。

### テストとツール

- `cargo test --workspace` で単体テストと結合テストを実行します。辞書を使う
  テストのために、先に `dist\system.dict` を `data\system.dict` にコピーします。
- `target\release\examples\harness.exe` は、本物の TSF コア
  (`sweet_tsf_core.dll`)を登録せずにプロセス内で動かし、入口の DLL
  (`sweet_tsf.dll`)がフォーカス時にコアを切り替えることも確かめます。
- `crates/fcitx5/tests/run-dbus-harness.sh result` は、専用の fcitx5 を D-Bus
  越しに動かしてアドオンを確かめます(Linux で `nix build` のあと)。
- `sweet-cli data\system.dict eval <evaluation.tsv>` で精度を測ります。
  `SWEET_ZENZ=data\zenz-v3.1-xsmall.swz` を設定すると並べ替えも含めます。
  `crates\cli\data\terms.tsv` で、技術用語・ネットスラング・文中の数字を確かめます。
- `sweet-cli data\system.dict audit [読みの数]` は、普通名詞の順位をまちがえる
  Mozc のコストを探します。読みを短い文に入れて変換し、`SWEET_ZENZ` がはっきり
  別の名詞を選んだものを、補正値の案と一緒に一覧にします。
  `crates\dictc\src\cost_adjustments.tsv` に足す前に、1 つずつ目で確かめてください。
- `python scripts/make-icon.py` で `assets/icon/` のアイコンを描き直します。
  Pillow と Zen Maru Gothic Black フォント(SIL OFL)が必要で、
  `SWEET_ICON_FONT` にフォントのファイルを指定します。

### 更新が動いているアプリに届くしくみ

アプリが読み込むのは小さな入口の DLL `sweet_tsf.dll`(`crates/tsf-shim`)で、
これが隣の `core.txt` に書かれたコア(`crates/tsf` の
`sweet_tsf_core-<バージョン>.dll`)を読み込みます。更新では新しいコアと
`core.txt` を入れるので、動いているアプリも、次にフォーカスが移ったときに
再起動なしで新しいコアに切り替わります。

### ブランチ・CI・リリース

- 変更は `develop` で行い、CodeRabbit のレビューと CI のために `main` へ
  プルリクエストを出します。
- GitHub Actions では、ワークスペースのテストと Windows の TSF ハーネス、
  エミュレーターで Android の APK が同梱の辞書で起動するかの確認、x86-64 と
  ARM64 の Linux での fcitx5 パッケージのビルドとテストを行います。
  インストーラーと APK は `SweetIME-Windows` と `SweetIME-Android` の
  アーティファクトとして残ります。Actions タブから手動でも実行できます。
- Linux のビルドをキャッシュするには、Actions のリポジトリ変数
  `CACHIX_CACHE_NAME` に公開 Cachix キャッシュ名を設定します。PR でも
  キャッシュを読み込めます。書き込み権限のある `CACHIX_AUTH_TOKEN` を
  Actions シークレットに設定すると、push 時の CI がテスト済みの
  x86-64・ARM64 パッケージを登録します。
- リリースするときは、`Cargo.toml` と `nix/package.nix` の `version` を上げて
  `main` にマージし、そのコミットの `Release eligibility` が通ったら、
  `v0.1.3` のようなタグを打って push します。タグのビルドが、インストーラーと
  APK を GitHub Releases に公開します。

## ライセンス

- **コード**:MIT(`LICENSE` を参照)
- **辞書**(`system.dict`、同梱):Mozc OSS の辞書をもとにしています。これには
  それぞれのライセンスを持つ IPAdic と沖縄辞書が含まれます
  (`dist\licenses\dictionary-oss.txt` を参照)。追加の語彙は、
  Works Applications Co., Ltd. の
  [SudachiDict](https://github.com/WorksApplications/SudachiDict)
  (Apache License, Version 2.0)から来ています。表示と変更点は
  `assets/licenses/sudachidict.txt` を参照してください。
- **英単語の一覧**(`crates/engine/src/english_words.txt`、同梱):Kevin Atkinson
  の SCOWL / ESDB から作りました(`dist\licenses\english-words.txt` を参照)。
- **zenz モデル**(同梱なし):Miwa Keita さん作、CC BY-SA 4.0。使う人が自分で
  ダウンロードします。
