# Sweet IME

A Japanese input method written from scratch in Rust.

- **Engine**: a dictionary lattice with Viterbi and N-best search, using
  the Mozc OSS dictionary and connection costs, with additional vocabulary
  from SudachiDict. Conversion takes about
  0.1 ms.
- **Neural reranking**: an optional zenz (GPT-2) model reranks the top
  lattice candidates using the text before the cursor. It runs natively
  with AVX2 kernels and tree attention over a shared KV cache, in about
  15–25 ms. During live conversion it runs on a background thread, so
  typing never waits for it.
- **Windows**: a TSF text service for both 64-bit and 32-bit applications.
  It has a Direct2D candidate window that follows light/dark mode and the
  accent color and accepts mouse clicks, plus a taskbar input mode
  indicator ("あ" / "A").
- **Linux**: an fcitx5 addon. It works on Wayland compositors that support
  `input-method-v2` (such as niri, Sway and Hyprland) and with fcitx5's
  other frontends. fcitx5 draws the candidate window, with a Sweet IME
  theme chosen in the settings app.
- **Android** (early): an input method with a QWERTY romaji keyboard and
  a candidate bar.
- **Features**: live conversion, predictive suggestions (including
  mid-sentence), romaji typo correction, per-user learning, a user
  dictionary, numbers read out in kana (さんぜんえん → 三千円 / 3000円),
  bunsetsu resizing, and F6–F10 transliteration.

## Accuracy

Top-1 results on Mozc's `evaluation.tsv` (397 cases):

| Configuration                    | Top-1 | Mean latency |
|----------------------------------|-------|--------------|
| Lattice only                     | 70.8% | ~0.1 ms      |
| + zenz v3.1 xsmall (default)     | 82.6% | ~13 ms       |
| + zenz v3.1 small                | 85.9% | ~41 ms       |
| Mozc OSS (reference)             | 74.6% | —            |

## Build

Requirements:

- Rust stable with the MSVC toolchain
- The `i686-pc-windows-msvc` target, for the 32-bit DLL
- The Mozc OSS dictionary in `third_party/mozc-data`

To build, run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```

This stages the files in `dist\` and creates the single-file installer
`out\SweetIME-Setup.exe`. The build downloads and verifies the pinned
SudachiDict lexicons on first use. To build just the dictionary, run
`./scripts/build-dictionary.ps1 -Output data/system.dict`.

### CI and releases

GitHub Actions runs the workspace tests and Windows TSF harness, checks
that the Android APK starts with its bundled dictionary in an emulator,
and builds and tests the fcitx5 package on x86-64 and ARM64 Linux.
The Windows installer and Android APK are available as the
`SweetIME-Windows` and `SweetIME-Android` artifacts. Pushing a version tag
such as `v0.1.0` publishes both packages after all checks pass.
Runs can also be started manually from the Actions tab.

## Install

Run `SweetIME-Setup.exe`. It asks for administrator rights, then:

- Installs to `%ProgramFiles%\SweetIME`
- Registers the IME for 64-bit and 32-bit apps
- Optionally adds it to the Japanese keyboard list, adds a Start menu
  entry, and downloads the AI model
- Registers it in Installed apps, where you can uninstall it

`SweetIME-Setup.exe --preview` shows the screens without changing
anything.

### Updates

The settings app checks GitHub Releases for a newer version when it opens,
and the installer registers a check at sign-in (at most once a day). Either
one offers to update. The update downloads `SweetIME-Setup.exe` from the
release, checks its SHA-256 against the release, and runs it with
`--update`, which updates the existing install without asking again. To
turn the checks off, use **Settings → 情報**, or set `update_check = off`.

To publish an update, raise `version` in `Cargo.toml` and `nix/package.nix`,
then push a matching tag such as `v0.1.3`.

For development, `scripts\install.ps1 -AddToLanguageList` and
`scripts\uninstall.ps1` do the same from an elevated PowerShell.

### Optional: neural reranking

The zenz model is not bundled, because it is licensed under
CC BY-SA 4.0. To enable reranking, download it from **Settings → AI 変換**.
You can also run the script yourself. Administrator rights are not
needed:

```powershell
powershell -ExecutionPolicy Bypass -File "$env:ProgramFiles\SweetIME\get-zenz.ps1"
```

The script writes `%LOCALAPPDATA%\SweetIME\zenz.swz`, which the IME
loads the next time it is activated. Pass
`-Model Miwa-Keita/zenz-v3.1-small` to use the larger, more accurate
model. Without the model, the IME converts with the dictionary lattice
only.

## Linux (fcitx5)

The flake builds the fcitx5 addon, the system dictionary, the settings app
and the model packer:

```sh
nix build github:frost-1256/sweet-ime
```

On NixOS, add the package to the fcitx5 addons and to the input method
group:

```nix
# flake inputs
sweet-ime.url = "github:frost-1256/sweet-ime";

# NixOS or Home Manager
i18n.inputMethod.fcitx5.addons = [ inputs.sweet-ime.packages.${pkgs.system}.default ];
```

Then add **Sweet IME** to the input method list (input method name
`sweetime`), for example in `~/.config/fcitx5/profile` or through the fcitx5
settings. Its **設定** entry opens `sweet-settings`.

Without Nix, build with CMake. It runs cargo for the Rust part, so you
need Rust, CMake and the fcitx5 development files:

```sh
cmake -S crates/fcitx5 -B build -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build && sudo cmake --install build
cargo build --release -p sweet-dictc
target/release/sweet-dictc third_party/mozc-data/src/data/dictionary_oss system.dict
sudo install -Dm644 system.dict /usr/share/sweet-ime/system.dict
```

To include the same vocabulary as the packaged builds, extract the three
pinned SudachiDict CSV files listed in `nix/package.nix` and pass their
directory as the third argument to `sweet-dictc`. Install
`assets/licenses/sudachidict.txt` alongside the other dictionary notices
when distributing the resulting dictionary.

On Linux the files live in XDG directories:

- Settings, key bindings and learned words: `~/.config/sweet-ime/`
- The zenz model: `~/.local/share/sweet-ime/zenz.swz`. Download it from
  **Settings → AI 変換** or with `share/sweet-ime/get-zenz.sh`, then restart
  fcitx5.

The **テーマ** page writes the chosen colors as an fcitx5 classicui theme
(`~/.local/share/fcitx5/themes/sweet-ime`) and selects it in
`~/.config/fcitx5/conf/classicui.conf`, which also styles other input
methods. **fcitx5 の標準テーマに戻す** selects fcitx5's own theme again. If
Home Manager manages `classicui.conf`, set `Theme = sweet-ime` there
instead (`sweet-ime-dark` as `DarkTheme` for the default theme).

The key bindings file is shared with Windows. Japanese keyboard keys
(半角/全角, 変換, 無変換, ひらがな, 英数) work the same way. fcitx5's own
trigger key (Ctrl+Space by default) switches between Sweet IME and the
keyboard layout.

## Android

`android/` holds an input method with a QWERTY romaji keyboard, built
without Gradle by `android/build.ps1` (cargo-ndk, aapt2, javac, d8 and
apksigner). It needs the Android SDK with `platforms;android-36`,
`build-tools;36.0.0` and an NDK, a JDK 17 or newer, `cargo install
cargo-ndk`, `rustup target add aarch64-linux-android x86_64-linux-android`, and
the Mozc sources in `third_party/mozc-data`. The script builds the bundled
dictionary from Mozc and the pinned SudachiDict lexicons:

```powershell
powershell -ExecutionPolicy Bypass -File android/build.ps1 -Install
```

The APK (`out/SweetIME-Android.apk`) bundles the dictionary and copies
it to the app's storage on first use. Open **Sweet IME** to turn the
keyboard on, then pick it with the keyboard switcher. Shift starts an
English word as on the desktop, and the globe key switches to direct
English input. Password, URL, email and number fields start in English.
The layout follows common Android keyboards: long-press q–p for digits,
**?123** for symbols and a number pad, and swipe along the space bar to
move the caret. The app holds the conversion and keyboard settings (key
height, number row, vibration, sound, theme). Themes include Material 3 and
Material 3 Expressive, which take the wallpaper colors (Material You) on
Android 12 and later, and the desktop color themes.
For AI conversion, download a zenz model under **AI 変換** in the app
(xsmall, about 100 MB, or small, about 380 MB). The app converts it on the
phone and reranks conversions with it; live conversion is refined in the
background. With xsmall a sentence takes about 0.2 s on a recent phone.

## Keys

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

These are the default bindings. You can change them on the **キー** page of
the settings app, which saves them to `%APPDATA%\SweetIME\keymap.txt`. A
binding can apply always, only while composing, or only while not
composing. For example, 変換 can convert while composing and turn the IME on
otherwise.

When the IME is turned on or off, "あ" or "A" appears briefly next to the
text cursor.

While the IME is on, start a word with Shift+letter to type it in English.
Release Shift to continue typing the word. Space commits it with a
half-width space, and Enter commits the word alone. Both return to Japanese
input. Escape cancels the word. A known word also ends by itself when
Japanese follows it: `Googlehakensaku` commits `Google` and goes on with
`は検索`, and `Googleha` followed by Enter commits `Googleは`
(`shift_one_word = off` keeps English until Space or Enter). For common
technical terms, type `ime`,
`ieee`, or `github` and press Space to convert to `IME`, `IEEE`, or
`GitHub`. Japanese readings such as `あいえむいー` also work.

Common English words whose keys are not romaji convert inside a sentence
too: `korehabestdesu` gives `これはベストです`. A word with a common katakana
loanword shows in katakana first, with the spelling as the next candidate
(`english_katakana = off` keeps the spelling); others keep their spelling
(`software`). The words come from a bundled
list (SCOWL). Words that are not on it, such as one fixed with F10 after a
typo correction, are learned when committed and convert the same way next
time. To turn this off, set `english_words = off`.

## Settings

Open the settings app, `sweet-settings.exe`, in any of these ways:

- From the Start menu entry **Sweet IME の設定**
- By right-clicking the "あ" / "A" indicator in the taskbar
- From Windows language settings: Sweet IME → Keyboard options

The app saves changes immediately. They apply the next time an app gets
focus.

In the app you can:

- Turn live conversion, suggestions and the "あ" / "A" indicator on or off
- Choose a color theme for the candidate window (on Linux, as an fcitx5
  theme)
- Change key bindings
- Turn AI conversion on or off, and download or remove the zenz model
- Turn learning on or off, and review, delete or clear learned words, or
  forget what was learned in the last hour or day
- Register, edit and delete your own words, and import or export them

Settings are stored in `%APPDATA%\SweetIME\config.txt`:

```
live_conversion = on   # show converted text while typing
suggestions = on       # show predictions while typing
neural = on            # rerank with the zenz model when installed
learning = on          # learn from chosen candidates
theme = default        # default, light, dark, sakura, matcha, midnight, lavender, dracula, adwaita
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

Per-application modes are stored in `%APPDATA%\SweetIME\apps.txt`, one
program per line as `program.exe = normal | english | off` (start in
hiragana, start in direct input, or leave the program alone). The settings
app edits them on the アプリ page.

Learned words are stored in `%APPDATA%\SweetIME\history.tsv`.

Conversions you report are stored in `%APPDATA%\SweetIME\reports.tsv`.
Add a reading and the text you wanted on the settings app's 学習 page (on
Android, 学習とプライバシー), or press Ctrl+F12 right after committing a
conversion you had to fix. The file stays on your device; the settings
app lists the reports and opens a prefilled GitHub issue only when you
press GitHub に送る.

Your own words are stored in `%APPDATA%\SweetIME\user_dict.tsv`, one per
line as `reading<TAB>word<TAB>part of speech<TAB>comment`. Mozc and Google
Japanese Input export the same format, so their files (and Microsoft IME
exports) can be imported from the settings app. The IME picks up changes
to the file right away.

## Development

- `cargo test --workspace`
- `target\release\examples\harness.exe` drives the real TSF core
  (`sweet_tsf_core.dll`) in-process without registering it, and checks
  that the entry DLL (`sweet_tsf.dll`) switches cores on focus.
- Applications load the small entry DLL `sweet_tsf.dll` (`crates/tsf-shim`),
  which loads the core named in `core.txt` next to it
  (`sweet_tsf_core-<version>.dll`, from `crates/tsf`). An update installs a
  new core and `core.txt`; running applications move to it the next time
  they get focus, without a restart.
- `crates/fcitx5/tests/run-dbus-harness.sh result` drives the fcitx5 addon
  through a private fcitx5 over D-Bus (Linux, after `nix build`).
- `sweet-cli data\system.dict eval <evaluation.tsv>` measures accuracy.
  Set `SWEET_ZENZ=data\zenz-v3.1-xsmall.swz` to include the reranker.
  `crates\cli\data\terms.tsv` checks technical terms, internet slang and
  numbers in context.
- `sweet-cli data\system.dict audit [readings]` looks for Mozc costs that
  put the wrong common noun first: it converts each reading in a few short
  phrases and lists the ones where `SWEET_ZENZ` clearly prefers another
  noun, with a suggested offset. Check each suggestion by hand before
  adding it to `crates\dictc\src\cost_adjustments.tsv`.
- `python scripts/make-icon.py` redraws the app icon in `assets/icon/`. It
  needs Pillow and the Zen Maru Gothic Black font (SIL OFL); set
  `SWEET_ICON_FONT` to the font file.

## Licenses

- **Code**: MIT; see `LICENSE`.
- **Dictionary** (`system.dict`, bundled): derived from the Mozc OSS
  dictionary. It includes IPAdic and the Okinawa dictionary, which keep
  their own licenses. See `dist\licenses\dictionary-oss.txt`. Additional
  vocabulary comes from [SudachiDict](https://github.com/WorksApplications/SudachiDict)
  by Works Applications Co., Ltd., under the Apache License, Version 2.0.
  See `assets/licenses/sudachidict.txt` for notices and changes.
- **English word list** (`crates/engine/src/english_words.txt`, bundled):
  generated from SCOWL / ESDB by Kevin Atkinson; see
  `dist\licenses\english-words.txt`.
- **zenz model** (not bundled): by Miwa Keita, licensed under
  CC BY-SA 4.0. Users download it themselves with `get-zenz.ps1`.
