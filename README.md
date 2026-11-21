# Sweet IME

A Japanese input method written from scratch in Rust.

- **Engine**: a dictionary lattice with Viterbi and N-best search, using
  the Mozc OSS dictionary and connection costs. Conversion takes about
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
  other frontends. fcitx5 draws the candidate window.
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
`out\SweetIME-Setup.exe` (about 28 MB).

### CI and releases

GitHub Actions builds and verifies the Windows installer and runs the
workspace tests on pushes and pull requests. Each successful run provides
the installer as the `SweetIME-Windows` artifact. Pushing a version tag
such as `v0.1.0` publishes the verified installer to GitHub Releases.

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

On Linux the files live in XDG directories:

- Settings, key bindings and learned words: `~/.config/sweet-ime/`
- The zenz model: `~/.local/share/sweet-ime/zenz.swz`. Download it from
  **Settings → AI 変換** or with `share/sweet-ime/get-zenz.sh`, then restart
  fcitx5.

The key bindings file is shared with Windows. Japanese keyboard keys
(半角/全角, 変換, 無変換, ひらがな, 英数) work the same way. fcitx5's own
trigger key (Ctrl+Space by default) switches between Sweet IME and the
keyboard layout.

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
input. Escape cancels the word. For common technical terms, type `ime`,
`ieee`, or `github` and press Space to convert to `IME`, `IEEE`, or
`GitHub`. Japanese readings such as `あいえむいー` also work.

## Settings

Open the settings app, `sweet-settings.exe`, in any of these ways:

- From the Start menu entry **Sweet IME の設定**
- By right-clicking the "あ" / "A" indicator in the taskbar
- From Windows language settings: Sweet IME → Keyboard options

The app saves changes immediately. They apply the next time an app gets
focus.

In the app you can:

- Turn live conversion, suggestions and the "あ" / "A" indicator on or off
- Choose a color theme for the candidate window (Windows)
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
theme = default        # default, light, dark, sakura, matcha, midnight, lavender
update_check = on      # look for new releases on GitHub
dates = on             # きょう / いま -> dates and times
emoji = on             # えがお -> 😊, かお -> kaomoji
calculator = on        # 1+2*3= -> 7
undo_commit = on       # Ctrl+Backspace right after a commit restores the reading
private_fields = on    # learn nothing in password fields and private windows
```

Per-application modes are stored in `%APPDATA%\SweetIME\apps.txt`, one
program per line as `program.exe = normal | english | off` (start in
hiragana, start in direct input, or leave the program alone). The settings
app edits them on the アプリ page.

Learned words are stored in `%APPDATA%\SweetIME\history.tsv`.

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
- `python scripts/make-icon.py` redraws the app icon in `assets/icon/`. It
  needs Pillow and the Zen Maru Gothic Black font (SIL OFL); set
  `SWEET_ICON_FONT` to the font file.

## Licenses

- **Code**: MIT; see `LICENSE`.
- **Dictionary** (`system.dict`, bundled): derived from the Mozc OSS
  dictionary. It includes IPAdic and the Okinawa dictionary, which keep
  their own licenses; see `dist\licenses\dictionary-oss.txt`.
- **zenz model** (not bundled): by Miwa Keita, licensed under
  CC BY-SA 4.0. Users download it themselves with `get-zenz.ps1`.
