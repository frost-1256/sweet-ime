<p align="center">
  <img src="assets/icon/sweet-ime-128.png" width="96" height="96" alt="">
</p>

<h1 align="center">Sweet IME</h1>

<p align="center">
  A Japanese input method written from scratch in Rust,<br>
  for Windows, Linux (fcitx5), Android and macOS.
</p>

<p align="center">
  <b>English</b> · <a href="README.ja.md">日本語</a>
</p>

---

## Highlights

- **Fast dictionary conversion.** A lattice with Viterbi and N-best search
  over the Mozc OSS dictionary and connection costs, with extra vocabulary
  from SudachiDict. A conversion takes about 0.1 ms.
- **Optional AI reranking.** A zenz (GPT-2) model reranks the top
  candidates using the text before the cursor. It runs natively with AVX2
  kernels and tree attention over a shared KV cache, in about 15–25 ms, on
  a background thread during live conversion, so typing never waits.
- **Comfortable everyday typing.** Live conversion, predictive suggestions
  (also mid-sentence), romaji typo correction, learning, a user dictionary,
  English words inside Japanese, numbers read out in kana
  (さんぜんえん → 三千円 / 3000円), bunsetsu resizing and F6–F10
  transliteration.
- **Private by default.** Learning, reports and settings stay on your
  device. On Windows and Android, nothing is learned in password fields
  or private windows (the Linux frontend does not detect them yet).

| Platform | What you get |
|---|---|
| **Windows** | A TSF text service for 64-bit and 32-bit apps, a Direct2D candidate window that follows light/dark mode and the accent color, and an "あ" / "A" mode indicator |
| **Linux** | An fcitx5 addon for Wayland compositors with `input-method-v2` (niri, Sway, Hyprland, …) and fcitx5's other frontends |
| **Android** | QWERTY romaji and 12-key kana flick keyboards, a candidate bar, Material 3 themes and on-device AI conversion (Android 10 or later) |
| **macOS** | InputMethodKit with a compact candidate window and the shared settings app (macOS 15 or later, Intel and Apple Silicon) |

## Accuracy

Top-1 accuracy on Mozc's `evaluation.tsv` (397 cases) and mean conversion
latency measured locally on Windows.

| Configuration                | Top-1 | Mean latency |
|------------------------------|-------|--------------|
| Lattice only                 | 75.8% | ~0.1 ms      |
| + zenz v3.1 xsmall (default) | 87.9% | ~8.6 ms      |
| + zenz v3.1 small            | 88.4% | ~27 ms       |
| Mozc OSS (reference)         | 74.6% | —            |

## Install

### Windows

Download `SweetIME-Setup.exe` from the
[latest release](https://github.com/frost-1256/sweet-ime/releases/latest)
and run it. It asks for administrator rights, then:

- installs to `%ProgramFiles%\SweetIME`;
- registers the IME for 64-bit and 32-bit apps;
- optionally adds it to the Japanese keyboard list, adds a Start menu
  entry, and downloads the AI model;
- registers itself in Installed apps, where you can uninstall it.

`SweetIME-Setup.exe --preview` shows the screens without changing
anything.

**Updates.** The settings app checks GitHub Releases when it opens, and the
installer registers a check at sign-in (at most once a day). Either one
offers to update: it downloads `SweetIME-Setup.exe`, checks its SHA-256
against the release, and runs it with `--update`. To turn the checks off,
use **Settings → 情報**, or set `update_check = off`.

### macOS

Download and extract `SweetIME-macOS.zip` from the
[latest release](https://github.com/frost-1256/sweet-ime/releases/latest).
Open **Install Sweet IME.command** beside the app to install or replace it for
your account. If the ZIP has no installer, copy `Sweet IME.app` into
`~/Library/Input Methods`, replacing the previous copy if present.

Log out and back in to load the installed version. On the first install,
add **Sweet IME** under Japanese in **System Settings → Keyboard → Text Input
→ Edit** and select it from the input menu. The same menu opens **設定…**
(Settings). For future updates, go to **情報 → アップデート** and select **更新する**
when a new version is available. Your settings, history and models stay in
their existing locations.
If a v0.5.1 update was interrupted and cannot be retried in Settings, run
the installer from the latest ZIP. It restores the previous app if it was
moved aside, then installs the new version.

You can also install or update from Terminal:

```sh
installer=$(mktemp -t sweet-ime-install) &&
  /usr/bin/curl -qfsSL https://raw.githubusercontent.com/frost-1256/sweet-ime/main/macos/install.sh -o "$installer" &&
  /bin/bash "$installer"
```

When updating from `v0.5.0-alpha.1`, replace the previous `Sweet IME.app`
before logging back in. If the source is missing, run this in Terminal to
check whether macOS recognizes it in the current login session:

```sh
"$HOME/Library/Input Methods/Sweet IME.app/Contents/MacOS/SweetIME" --check-input-source
```

If macOS displays a warning about a downloaded build, see
[Apple's guidance](https://support.apple.com/en-us/102445).

The macOS version converts text within the current composition.
Its settings, history and optional
zenz model live in `~/Library/Application Support/SweetIME`. After installing
or removing a zenz model, log out and back in to reload it. Zenz uses AVX2 and
FMA when available on Intel and NEON on Apple Silicon.

### Android

Download `SweetIME-Android.apk` from the
[latest release](https://github.com/frost-1256/sweet-ime/releases/latest)
and install it. Open **Sweet IME** to turn the keyboard on, then pick it
with the keyboard switcher. The dictionary is bundled.

### Linux (fcitx5)

The flake builds the fcitx5 addon, the system dictionary, the settings app
and the model packer. The public Cachix cache at
`https://sweet-ime.cachix.org` serves CI-tested builds for `x86_64-linux`
and `aarch64-linux`. Nix uses a cached build when it matches the requested
revision and dependencies, and builds locally on a cache miss.

On Linux distributions other than NixOS, enable the cache with the
[Cachix CLI](https://docs.cachix.org/using-a-cache) before building.

```sh
cachix use sweet-ime
nix build github:frost-1256/sweet-ime
```

On NixOS, add the cache URL and its public signing key to your system
configuration and apply it with `sudo nixos-rebuild switch`.

```nix
nix.settings = {
  extra-substituters = [ "https://sweet-ime.cachix.org" ];
  extra-trusted-public-keys = [
    "sweet-ime.cachix.org-1:KrnImhtgcdSD1qlYusdBprFJWl/AsgL24H+QUwDZbIY="
  ];
};
```

On NixOS or Home Manager, add the package to the fcitx5 addons:

```nix
# flake inputs
sweet-ime.url = "github:frost-1256/sweet-ime";

# NixOS or Home Manager
i18n.inputMethod.fcitx5.addons = [ inputs.sweet-ime.packages.${pkgs.system}.default ];
```

Then add **Sweet IME** (input method name `sweetime`) to the input method
list, for example in `~/.config/fcitx5/profile` or in the fcitx5 settings.
Its **設定** entry opens `sweet-settings`.

Without Nix, build with CMake. It runs cargo for the Rust part, so you
need Rust, CMake and the fcitx5 development files. The dictionary is
built from the Mozc OSS sources, at the revision CI uses:

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

To include the same vocabulary as the packaged builds, extract the three
pinned SudachiDict CSV files listed in `nix/package.nix` and pass their
directory as the third argument to `sweet-dictc`. When you distribute the
resulting dictionary, install `assets/licenses/sudachidict.txt` with the
other dictionary notices.

### AI conversion (optional)

The zenz model is not bundled, because it is licensed under CC BY-SA 4.0.
Without it, Sweet IME converts with the dictionary lattice only.

- **Windows**: download it from **Settings → AI 変換**, or run the script
  yourself (no administrator rights needed):

  ```powershell
  powershell -ExecutionPolicy Bypass -File "$env:ProgramFiles\SweetIME\get-zenz.ps1"
  ```

  It writes `%LOCALAPPDATA%\SweetIME\zenz.swz`, which the IME loads the
  next time it is activated. Pass `-Model Miwa-Keita/zenz-v3.1-small` for
  the larger, more accurate model.
- **Linux**: download it from **Settings → AI 変換**, or run
  `share/sweet-ime/get-zenz.sh` from the Nix package (`scripts/get-zenz.sh`
  in a checkout; the CMake install does not include it), then restart
  fcitx5. The model lives in `$XDG_DATA_HOME/sweet-ime/zenz.swz`.
- **Android**: download it under **AI 変換** in the app (xsmall, about
  100 MB, or small, about 380 MB). The app converts it on the phone. Live
  conversion is refined in the background; with xsmall a sentence takes
  about 0.2 s on a recent phone.

## Typing

### Keys

| Key                      | While typing                      | While converting                 |
|--------------------------|-----------------------------------|----------------------------------|
| Space                    | Convert                           | Next candidate / open list       |
| Enter                    | Commit (live result)              | Commit                           |
| Tab                      | Choose from suggestions           | —                                |
| Esc                      | Show reading, then cancel         | Back to reading                  |
| ← / →                    | Move caret                        | Move between segments            |
| Shift+← / Shift+→        | —                                 | Shrink or extend the segment     |
| 1–9                      | —                                 | Select candidate                 |
| F6 / F7 / F8 / F9 / F10  | Hiragana / katakana / half-width katakana / full-width ASCII / ASCII | same |
| 半角/全角 or Alt+`, 変換, 無変換 | Toggle / on / off              |                                  |
| Ctrl+Backspace           | Right after a commit: back to the reading (Windows) |                |
| Ctrl+F12                 | Right after a commit: report the conversion (Windows) |              |

These are the defaults. Change them on the **キー** page of the settings
app, which saves them to `%APPDATA%\SweetIME\keymap.txt`. A binding can
apply always, only while composing, or only while not composing; for
example, 変換 can convert while composing and turn the IME on otherwise.
When the IME is turned on or off, "あ" or "A" appears briefly next to the
text cursor.

### English in Japanese

- **Shift starts an English word.** Type Shift+letter, release Shift and
  keep typing. Space commits the word with a half-width space, Enter
  commits the word alone, and both return to Japanese. Escape cancels it.
- **A known word ends by itself.** `Googlehakensaku` commits `Google` and
  goes on with `は検索`; `Googleha` then Enter commits `Googleは`
  (`shift_one_word = off` keeps English until Space or Enter).
  A comma can introduce Japanese too. For example, start `Anthropic` with
  Shift and continue with `,kyouhadousuru` to type `Anthropic、今日はどうする`.
- **Technical terms.** Type `ime`, `ieee` or `github` and press Space for
  `IME`, `IEEE` or `GitHub`. Japanese readings such as `あいえむいー` work
  too.
- **Common English words inside a sentence.** `korehabestdesu` gives
  `これはベストです`. A word with a common katakana loanword shows in
  katakana first, with the spelling next (`english_katakana = off` keeps
  the spelling); others keep their spelling (`software`). The words come
  from a bundled SCOWL list, and other words you commit, such as one fixed
  with F10, are learned. `english_words = off` turns this off.
- **Addresses in Japanese mode.** Type a URL, `dev@example.com`, or
  `localhost:3000` in a regular text field and press Enter to keep it in ASCII.

### Android keyboard

Turn on **かなフリック入力** under **キーボード** in Sweet IME settings to
use the 12-key Japanese layout. Tap **あ** for あ, or flick left, up, right
or down for い, う, え or お. Use **小゛゜** to cycle the kana before the
caret through its small, voiced or semi-voiced forms while composing.
[See the kana flick layout.](assets/android/flick-guide.png)

Turn on **英数フリック入力** to use a 12-key layout for direct English input.
Tap **abc** for a or flick left, up or down for b, c or 2. The letter keys
also enter digits with a downward flick. Password fields always open QWERTY.

Enable **変換 → Simejiクラウド変換** to look up kaomoji. While composing kana,
tap the cloud icon in the candidate bar to send the reading to Simeji and
open the expanded list with the results. Tap a result to commit it.
The feature is off by default and unavailable in
password or private fields. [Simeji's privacy policy](https://simeji.me/privacy/)

Use **テーマとフォント** to change the font for key labels, flick guides and
conversion candidates. You can select a system style or open a TTF or OTF file.

On QWERTY, Shift starts an English word as on the desktop. The globe key
on QWERTY and the **英数** / **かな** key on 12-key layouts switch between
Japanese and direct English input. Password, URL, email and number fields start in
English. Long-press q–p for digits, use **?123** for symbols and a number
pad, and swipe along the space bar to move the caret. The undo button in
the candidate bar takes the last commit back to its reading. The app holds
the conversion and keyboard settings (key height, number row, vibration,
sound) and the themes: Material 3 and Material 3 Expressive, which take the
wallpaper colors on Android 12 and later, and the desktop color themes.

## Settings

On Windows, Linux and macOS, **入力 → Simejiクラウド変換** is off by default.
When enabled, starting conversion with Space sends a single-segment kana reading
to Simeji and adds any kaomoji results after the local candidates. Password and
private fields are excluded when the platform identifies them.

On Windows and Linux, select an installed candidate font under **テーマ** →
**候補のフォント**. Linux uses fcitx5 Classic UI, so its setting applies to
other input methods that use Classic UI as well.

On Windows, open the settings app, `sweet-settings.exe`, from:

- the Start menu entry **Sweet IME の設定**;
- a right-click on the "あ" / "A" indicator in the taskbar;
- Windows language settings: Sweet IME → Keyboard options.

Reopening Windows settings brings the existing window forward and restores
it if minimized. Changes are saved right away and apply the next time an app
gets focus. In the app you can:

- turn live conversion, suggestions and the "あ" / "A" indicator on or off;
- turn on **入力した文字を表示** under **入力** on Windows to see the typed
  romaji beside the composition. It also works without suggestions and
  disappears when you commit or cancel. After an edit, the preview uses the
  current reading in romaji if the original keystrokes no longer match.
  The preview stays hidden in password and private fields, and when the IME
  cannot determine the input scope;
- choose a color theme for the candidate window (on Linux, as an fcitx5
  theme), or enable the animated rainbow border under **テーマ** →
  **ゲーミングモード** on Windows;
- change key bindings;
- turn AI conversion on or off, and download or remove the zenz model;
- turn learning on or off, review, delete or clear learned words, or forget
  what was learned in the last hour or day;
- register, edit, import and export your own words;
- report conversions that went wrong.

### Files

| What | Windows | Linux |
|---|---|---|
| Settings (`config.txt`) | `%APPDATA%\SweetIME\` | `$XDG_CONFIG_HOME/sweet-ime/` |
| Key bindings (`keymap.txt`) | `%APPDATA%\SweetIME\` | `$XDG_CONFIG_HOME/sweet-ime/` |
| Learned words (`history.tsv`) | `%APPDATA%\SweetIME\` | `$XDG_CONFIG_HOME/sweet-ime/` |
| Your words (`user_dict.tsv`) | `%APPDATA%\SweetIME\` | `$XDG_CONFIG_HOME/sweet-ime/` |
| Reports (`reports.tsv`) | `%APPDATA%\SweetIME\` | `$XDG_CONFIG_HOME/sweet-ime/` |
| Per-app modes (`apps.txt`) | `%APPDATA%\SweetIME\` | — |
| zenz model (`zenz.swz`) | `%LOCALAPPDATA%\SweetIME\` | `$XDG_DATA_HOME/sweet-ime/` |

When `XDG_CONFIG_HOME` or `XDG_DATA_HOME` is unset, Linux uses
`~/.config` and `~/.local/share`.

`config.txt` looks like this:

```
live_conversion = on   # show converted text while typing
suggestions = on       # show predictions while typing
show_typed_input = off # show romaji beside the composition (Windows)
neural = on            # rerank with the zenz model when installed
learning = on          # learn from chosen candidates
theme = default        # default, light, dark, sakura, matcha, midnight, lavender, dracula, adwaita
gaming_mode = off      # animate a rainbow border on the candidate window (Windows)
update_check = on      # look for new releases on GitHub
dates = on             # きょう / いま -> dates and times
emoji = on             # えがお -> 😊, かお -> kaomoji
calculator = on        # 1+2*3= -> 7
undo_commit = on       # Ctrl+Backspace right after a commit restores the reading
private_fields = on    # learn nothing in password fields and private windows
english_words = on     # common English words convert inside sentences
english_katakana = on  # ... as their katakana loanword first (best -> ベスト)
shift_one_word = on    # a word started with Shift ends when Japanese follows
```

- **Per-app modes**: `apps.txt` has one program per line as
  `program.exe = normal | english | off` (start in hiragana, start in
  direct input, or leave the program alone). A `* = english` line makes
  every other program start in direct input too. The **アプリ** page edits it.
- **Your words**: `user_dict.tsv` has one word per line as
  `reading<TAB>word<TAB>part of speech<TAB>comment`. Mozc and Google
  Japanese Input export the same format, so their files (and Microsoft IME
  exports) can be imported. The IME picks up changes right away.
- **Reports**: add a reading and the text you wanted on the **学習** page
  (on Android, **学習とプライバシー**), or press Ctrl+F12 right after
  committing a conversion you had to fix. Reports stay on your device. The
  settings app opens a prefilled GitHub issue only when you press
  **GitHub に送る**.

### Linux themes and keys

The **テーマ** page writes the chosen colors as an fcitx5 classicui theme
(`~/.local/share/fcitx5/themes/sweet-ime`) and selects it in
`~/.config/fcitx5/conf/classicui.conf`, which also styles other input
methods. **fcitx5 の標準テーマに戻す** selects fcitx5's own theme again. If
Home Manager manages `classicui.conf`, set `Theme = sweet-ime` there instead
(`sweet-ime-dark` as `DarkTheme` for the default theme).

The key bindings file is shared with Windows. Japanese keyboard keys
(半角/全角, 変換, 無変換, ひらがな, 英数) work the same way. fcitx5's own
trigger key (Ctrl+Space by default) switches between Sweet IME and the
keyboard layout.

## Development

### Build on Windows

You need:

- Rust stable with the MSVC toolchain;
- the `i686-pc-windows-msvc` target, for the 32-bit DLL;
- the Mozc OSS sources in `third_party/mozc-data`, at the revision pinned
  in `.github/workflows/windows.yml`.

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```

This stages the files in `dist\` and creates the single-file installer
`out\SweetIME-Setup.exe`. On first use it downloads and verifies the pinned
SudachiDict lexicons. To build just the dictionary, run
`./scripts/build-dictionary.ps1 -Output data/system.dict`.
`scripts\install.ps1 -AddToLanguageList` and `scripts\uninstall.ps1` install
and remove a development build from an elevated PowerShell.

### Build for Android

`android/build.ps1` builds the APK without Gradle (cargo-ndk, aapt2, javac,
d8 and apksigner). It needs the Android SDK with `platforms;android-36`,
`build-tools;36.0.0` and an NDK, JDK 17 or newer, `cargo install cargo-ndk`,
`rustup target add aarch64-linux-android x86_64-linux-android`, and the Mozc
sources in `third_party/mozc-data`. It builds the bundled dictionary from
Mozc and the pinned SudachiDict lexicons:

```powershell
powershell -ExecutionPolicy Bypass -File android/build.ps1 -Install
```

The APK is written to `out/SweetIME-Android.apk`.

### Build for macOS

On macOS 15 or later, install the Xcode Command Line Tools, stable Rust and
PowerShell. Check out Mozc under `third_party/mozc-data` at the revision
pinned in `.github/workflows/macos.yml`.

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pwsh -File scripts/build-dictionary.ps1 -Output data/system.dict
bash macos/build.sh
```

The output is `out/SweetIME-macOS.zip`. The bundle includes both architectures,
the system dictionary, the settings app and the zenz packing tool. The build
uses ad-hoc signing by default for local testing. GitHub Actions runs a
conversion smoke test on both architectures.

### Tests and tools

- To run the same unit and integration tests as CI, put the system dictionary
  at `data/system.dict` and run
  `cargo test --locked --workspace --all-targets -- --test-threads=1`.
  Running serially avoids contention in the time-limited typo correction tests.
- `target\release\examples\harness.exe` drives the real TSF core
  (`sweet_tsf_core.dll`) in-process without registering it, and checks that
  the entry DLL (`sweet_tsf.dll`) switches cores on focus.
- After `nix build` on Linux,
  `nix develop --command sh crates/fcitx5/tests/run-dbus-harness.sh result`
  drives the addon through a private fcitx5 instance over D-Bus.
- `sweet-cli data\system.dict eval <evaluation.tsv>` measures accuracy. Set
  `SWEET_ZENZ=data\zenz-v3.1-xsmall.swz` to include the reranker.
  `crates\cli\data\terms.tsv` checks technical terms, internet slang and
  numbers in context.
- `sweet-cli data\system.dict audit [readings]` looks for Mozc costs that
  put the wrong common noun first. It converts each reading in a few short
  phrases and lists the ones where `SWEET_ZENZ` clearly prefers another
  noun, with a suggested offset. Check each suggestion by hand before adding
  it to `crates\dictc\src\cost_adjustments.tsv`.
- `python scripts/make-icon.py` redraws the app icon in `assets/icon/`. It
  needs Pillow and the Zen Maru Gothic Black font (SIL OFL); set
  `SWEET_ICON_FONT` to the font file.

### How updates reach running apps

Applications load the small entry DLL `sweet_tsf.dll` (`crates/tsf-shim`),
which loads the core named in `core.txt` next to it
(`sweet_tsf_core-<version>.dll`, from `crates/tsf`). An update installs a
new core and `core.txt`; running applications move to it the next time
they get focus, without a restart.

### Branches, CI and releases

- Make changes on `develop`, and open a pull request into `main` for
  CodeRabbit review and CI. Enable auto-merge on that PR if you want GitHub
  to merge it once the seven required checks pass.
- GitHub Actions runs the workspace tests and the Windows TSF harness,
  checks in an emulator that the Android APK starts with its bundled
  dictionary, and builds and tests the fcitx5 package on x86-64 and ARM64
  Linux. The installer and the APK are kept as the `SweetIME-Windows` and
  `SweetIME-Android` artifacts. Runs can also be started from the Actions
  tab. Download the artifacts from the run to test a build without compiling
  it locally.
- To cache Linux builds, set the repository Actions variable
  `CACHIX_CACHE_NAME` to a public Cachix cache. Pull requests can read the
  cache. Set `CACHIX_AUTH_TOKEN` as an Actions secret with write access to
  let push workflows publish the tested x86-64 and ARM64 packages.
- Release timing is a manual decision. For the `0.x` series, use patch
  versions for fixes and minor versions for compatible new features.
  To release, update `Cargo.toml`, the workspace entries in `Cargo.lock`,
  and `nix/package.nix`. Merge into `main` and wait for `Release eligibility`
  to pass for the merged commit.
  Create an annotated tag with `git tag -a vX.Y.Z <main-commit>`. Enter a
  release title, a blank line, and Markdown bullets describing what users
  will notice since the previous release. Push the tag to publish the
  installer and APK with the summary above GitHub's generated changelog.
  Verify the summary and both download files on GitHub Releases.

## Licenses

- **Code**: MIT; see `LICENSE`.
- **Dictionary** (`system.dict`, bundled): derived from the Mozc OSS
  dictionary, which includes IPAdic and the Okinawa dictionary under their
  own licenses; see `dist\licenses\dictionary-oss.txt`. Additional
  vocabulary comes from [SudachiDict](https://github.com/WorksApplications/SudachiDict)
  by Works Applications Co., Ltd., under the Apache License, Version 2.0;
  see `assets/licenses/sudachidict.txt` for notices and changes.
- **English word list** (`crates/engine/src/english_words.txt`, bundled):
  generated from SCOWL / ESDB by Kevin Atkinson; see
  `dist\licenses\english-words.txt`.
- **zenz model** (not bundled): by Miwa Keita, under CC BY-SA 4.0. Users
  download it themselves.
