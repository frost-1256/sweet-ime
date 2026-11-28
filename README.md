<p align="center">
  <img src="assets/icon/sweet-ime-128.png" width="96" height="96" alt="Sweet IME icon">
</p>

<h1 align="center">Sweet IME</h1>

<p align="center">
  A fast, private Japanese input method written from scratch in Rust<br>
  for <b>Windows</b>, <b>Linux (fcitx5)</b>, <b>macOS</b>, and <b>Android</b>.
</p>

<p align="center">
  <a href="https://github.com/frost-1256/sweet-ime/releases/latest"><img src="https://img.shields.io/github/v/release/frost-1256/sweet-ime?style=flat-square" alt="Latest Release"></a>
  <a href="https://github.com/frost-1256/sweet-ime/actions/workflows/windows.yml"><img src="https://img.shields.io/github/actions/workflow/status/frost-1256/sweet-ime/windows.yml?branch=main&style=flat-square&label=CI" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/frost-1256/sweet-ime?style=flat-square" alt="License"></a>
</p>

<p align="center">
  <b>English</b> · <a href="README.ja.md">日本語</a>
</p>

---

## Highlights

- **Sub-millisecond dictionary engine.** Lattice Viterbi & N-best search over the Mozc OSS dictionary + SudachiDict vocabulary, converting in **~0.1 ms**.
- **Ultra-fast on-device AI conversion (optional).** Native Rust [zenz](https://huggingface.co/Miwa-Keita/zenz-v3.1-xsmall) (GPT-2) inference with per-column **INT8 (Q8) quantized matmul**, AVX2+FMA / NEON kernels, prompt prefix KV caching, and a **reading-guarded neural generation hybrid** reranks candidates and recovers rare/proper nouns in **~3.4 ms** (`xsmall`) or **~12.4 ms** (`small`) on a background thread—typing never waits.
- **Comfortable everyday typing.** Live conversion (with optional debounce), mid-sentence predictive suggestions, romaji typo correction (including long inputs and punctuation), mixed Japanese-English conversion (`korehabestdesu` → `これはベストです`, `pcha` → `PCは`), kana numbers (`さんぜんえん` → `三千円` / `3000円`), date/time & calculator candidates (excluded from learning history), bunsetsu resizing, and F6–F10 transliteration.
- **Unicode symbols by reading.** Convert `やじるし` for arrows, `すうがくきごう` for math symbols, `つうかきごう` for currency signs, `おんぷ` for music symbols, or `まるつきすうじ` for circled numbers. Greek letter names such as `あるふぁ` offer both cases (`α` / `Α`).
- **Customizable appearance.** Built-in themes (Default following system light/dark & accent, Light, Dark, Sakura, Matcha, Midnight, Lavender, Dracula, GTK Adwaita) plus a **User Theme** editor with custom colors, **background transparency**, and **named saved presets**.
- **Private by default.** Conversion runs locally. Settings, learning history, and reports stay on your device by default. A Windows Alpha build can offer [opt-in conversion feedback](contrib/alpha-feedback/README.md). Password fields and private windows never learn (Windows, Android).

## Platforms

| Platform | Download | Highlights |
|---|---|---|
| **Windows** | `SweetIME-Setup.exe` | 64-bit & 32-bit TSF text service, Direct2D candidate window (follows Windows light/dark & accent color, transparent user themes, optional rainbow border), live "あ"/"A" badge, one hot-swap to a new core per running app (restart the app to apply further updates) |
| **Linux (fcitx5)** | `SweetIME-Linux-{x86_64,aarch64}.tar.gz` / Nix Flake | fcitx5 addon for Wayland (`input-method-v2` native caret popup: niri, Sway, Hyprland, …) & X11, transparent classicui themes with blur, customizable font size / padding / vertical-horizontal layout |
| **macOS** | `SweetIME-macOS.zip` | Universal (Apple Silicon & Intel) InputMethodKit IME for macOS 15+, compact candidate window, shared Settings app, in-app updates |
| **Android** | `SweetIME-Android.apk` | QWERTY & 12-key kana/alphanumeric flick keyboards, Material 3 / Expressive wallpaper colors, custom TTF/OTF fonts, on-device AI conversion (Android 10+) |

## System Requirements

| | Minimum | Recommended (AI + Live Conversion) |
|---|---|---|
| **Windows** | Windows 10 (64-bit `x86_64`; supports 64-bit & 32-bit apps) | Windows 11 (64-bit `x86_64`) |
| **Linux** | 64-bit (`x86_64` / `aarch64`) with fcitx5 (Wayland or X11) | Recent distro with fcitx5 + Wayland (`input-method-v2`) |
| **macOS** | macOS 15 (Sequoia) or later (Apple Silicon / Intel) | macOS 15+ on Apple Silicon |
| **Android** | Android 10 (API 29) or later (`arm64-v8a` / `x86_64`) | Android 12+ (`arm64-v8a`) |
| **CPU** | 64-bit processor (`x86_64` or `aarch64`) | 4+ cores with **AVX2 + FMA** (`x86_64`) or **NEON** (`aarch64`) |
| **RAM** | ~100 MB free (dictionary only) / ~250 MB free (`xsmall`) | 4 GB+ total (512 MB+ free for `zenz-v3.1-small`) |
| **Storage** | ~100 MB free (~200 MB with `xsmall`) | ~500 MB free (with `zenz-v3.1-small` + user history) |

> **Note:** AI conversion (`zenz`) runs on any supported CPU using a portable fallback, but **AVX2 + FMA** (Intel Haswell / AMD Excavator or newer) or **ARM64 NEON** is strongly recommended for single-digit-millisecond live conversion.

## Accuracy & Speed

Measured on Windows (`x86_64` AVX2+FMA) with Q8 quantized `zenz` inference:

| Configuration | `eval/everyday.tsv` (169 cases) | Mean latency | Max latency |
|---|---|---|---|
| Lattice only | 95.9% | ~0.13 ms | ~4.7 ms |
| **+ zenz v3.1 xsmall (default)** | **95.9%** | **~3.4 ms** | **~11.6 ms** |
| **+ zenz v3.1 small** | **97.6%** | **~12.4 ms** | **~41.9 ms** |

On Mozc's `evaluation.tsv` (397 cases), Sweet IME scores **89.7%** Top-1 with `zenz` vs **76.8%** lattice-only (**74.6%** for Mozc OSS reference). You can also benchmark against [AJIMEE-Bench](https://github.com/azooKey/AJIMEE-Bench) with `sweet-cli data/system.dict eval-ajimee <evaluation_items.json>` (set `SWEET_ZENZ` to evaluate with a model).

---

## Install

Download the package for your platform from the **[Latest Release](https://github.com/frost-1256/sweet-ime/releases/latest)**.

### Windows
Run `SweetIME-Setup.exe`. It installs to `%ProgramFiles%\SweetIME`, registers both 64-bit and 32-bit TSF services, and can optionally add Sweet IME to your Japanese keyboard list and download the AI model.
- Updates are checked automatically via GitHub Releases (toggleable under **設定 → 情報**) and verified by SHA-256.
- Run `SweetIME-Setup.exe --preview` to preview the installer UI without changing anything.

### Linux (fcitx5)

**Option A — Prebuilt Binary Archive (Ubuntu, Arch, Fedora, etc.):**
Download `SweetIME-Linux-x86_64.tar.gz` or `SweetIME-Linux-aarch64.tar.gz` from the latest release and run the bundled installer:

```sh
tar -xzf SweetIME-Linux-x86_64.tar.gz
cd sweet-ime-linux-x86_64
sudo ./install.sh
```

Restart fcitx5 (`fcitx5 -r`) and add **Sweet IME** (`sweetime`) in `fcitx5-configtool`.
Pass `--prefix <dir>`, `--addon-dir <dir>`, or `--uninstall` as needed.

On GNOME (Wayland), which has no `wlr-layer-shell`, the candidate window comes from the [Input Method Panel (KIMPanel)](https://extensions.gnome.org/extension/261/kimpanel/) GNOME Shell extension or fcitx5's classic UI instead of Sweet IME's own window, so Sweet IME's candidate window themes do not apply there.

On Ubuntu, the default Snap build of Firefox may show no preedit or commit text and place candidates at the window's top-left with any fcitx5 input method. Install the `.deb` build from [Mozilla's APT repository](https://support.mozilla.org/kb/install-firefox-linux) instead.

When fcitx5 starts, Sweet IME checks for a new release (at most once a day) and shows a desktop notification (`notify-send`). **更新する** in the notification or in the settings app downloads the archive, verifies its SHA-256, runs its `install.sh` with the same locations through `pkexec` (polkit), and restarts fcitx5. Turn the check off in **設定 → 情報**.

**Option B — Nix Flake / NixOS:**
A public Cachix binary cache (`https://sweet-ime.cachix.org`) serves CI-tested `x86_64-linux` and `aarch64-linux` builds.

```nix
# flake.nix inputs
sweet-ime.url = "github:frost-1256/sweet-ime";

# NixOS / Home Manager
i18n.inputMethod.fcitx5.addons = [ inputs.sweet-ime.packages.${pkgs.system}.default ];
```

Nix builds also notify you of new releases, but they never update themselves: run `nix flake update sweet-ime` and rebuild.

<details>
<summary>Cachix setup & manual CMake build on Linux</summary>

On non-NixOS with Nix (for building into `./result`; to install on non-NixOS distros, use Option A above):
```sh
cachix use sweet-ime
nix build github:frost-1256/sweet-ime
```

On NixOS:
```nix
nix.settings = {
  extra-substituters = [ "https://sweet-ime.cachix.org" ];
  extra-trusted-public-keys = [
    "sweet-ime.cachix.org-1:KrnImhtgcdSD1qlYusdBprFJWl/AsgL24H+QUwDZbIY="
  ];
};
```

Manual CMake build (requires Rust, CMake, `libfcitx5core-dev`; installs only the fcitx5 addon and `system.dict`—use Option A or the Nix package to also install `sweet-settings` and `get-zenz.sh`):
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

If the first letter sometimes bypasses the IME after focusing a field on Niri, run `fcitx5 -r --verbose default=4,sweetime=5` and reproduce the issue. The `First letter after input-context activation` line reports the IME mode, modifier flags, and whether Sweet IME handled the event, without logging the letter itself.

### macOS
Extract `SweetIME-macOS.zip` and open **Install Sweet IME.command** (or copy `Sweet IME.app` to `~/Library/Input Methods`). Log out and back in, then add **Sweet IME** under **System Settings → Keyboard → Text Input → Edit → Japanese**.

Or install / update from Terminal:
```sh
installer=$(mktemp -t sweet-ime-install) &&
  /usr/bin/curl -qfsSL https://raw.githubusercontent.com/frost-1256/sweet-ime/main/macos/install.sh -o "$installer" &&
  /bin/bash "$installer"
```

### Android
Install `SweetIME-Android.apk`, open **Sweet IME** to enable the keyboard, and select it from the keyboard switcher. The system dictionary is bundled.

### AI Conversion Model (Optional)
Because the zenz model is licensed under CC BY-SA 4.0, it is downloaded separately on demand:
- **Windows / Linux / macOS**: Open **設定 → AI 変換** and click download (`zenz-v3.1-xsmall`, `small`, or `sweet-zenz-v1-small`). On Linux, restart fcitx5 (`fcitx5 -r`) afterward so the running process loads the new model; on macOS, log out and back in.
- **Android**: Open **AI 変換** in the app (~100 MB for `xsmall`, ~380 MB for `small` and `sweet-zenz-v1-small`).
- **sweet-zenz v1 small (alpha)**: [`zenz-v3.1-small` fine-tuned](https://huggingface.co/haruuuuu-1256/sweet-zenz-v1-small-gguf) for homophones and for VRChat, technical, and internet terms. It is downloaded as `ggml-model-Q8_0.gguf` and converted on the device. Its results may change in later releases.

---

## Typing & Features

### Default Key Bindings

| Key | While typing | While converting |
|---|---|---|
| `Space` | Convert | Next candidate / open list |
| `Enter` | Commit (live result) | Commit |
| `Tab` | Choose from suggestions | — |
| `Esc` | Show reading, then cancel | Back to reading |
| `←` / `→` | Move caret | Move between segments |
| `Shift+←` / `Shift+→` | — | Shrink / extend segment |
| `1`–`9` | — | Select candidate |
| `F6`–`F10` | Hiragana / Katakana / Half-width Katakana / Full-width ASCII / ASCII | Same |
| `半角/全角`, `Alt+\``, `変換`, `無変換` | Toggle / On / Off | — |
| `Ctrl+Backspace` | Undo last commit back to reading (Windows) | — |
| `Ctrl+F12` | Report last conversion (Windows) | — |

Customize any binding on the **キー** page of Settings (`keymap.txt`).
Left and right Shift, Ctrl, and Alt (Option on macOS) can be bound separately. A modifier binding runs when you release that key after pressing it by itself.

Edit the romaji-to-kana table on the **ローマ字** page (`romaji.tsv`). Rows have an input, an output, and an optional next input (`tt` → `っ` + `t`). You can import and export tables in the Mozc / Google Japanese Input format.

### English inside Japanese
- **Shift starts an English word:** Type `Shift+letter`, release Shift, and keep typing (`Space` commits with a space, `Enter` commits alone, `Esc` cancels). Known words end automatically when Japanese follows (`Googlehakensaku` → `Googleは検索`).
- **Mid-sentence English & tech terms:** `korehabestdesu` → `これはベストです`, `ime` / `github` → `IME` / `GitHub`.
- **Addresses stay ASCII:** URLs, `dev@example.com`, and `localhost:3000` commit as ASCII in Japanese mode.

### Themes & Customization
Open **設定 → テーマ** (Windows & Linux):
- Choose from 9 built-in themes or create a **User Theme** with custom colors and **background opacity (35%–100%)**.
- Save multiple custom themes by name (`user_themes.txt`) and switch between them from live preview tiles.
- Pick any installed font for candidate windows. On Linux (fcitx5), you can also adjust **candidate font size**, **vertical/horizontal layout**, and **panel padding** directly from Settings.
- On Windows, enable **ゲーミングモード** for an animated rainbow border.

<details>
<summary>Settings files & <code>config.txt</code> reference</summary>

| File | Windows (`%APPDATA%\SweetIME\`) | Linux (`$XDG_CONFIG_HOME/sweet-ime/`) |
|---|---|---|
| `config.txt` | General options | General options |
| `keymap.txt` | Key bindings | Key bindings |
| `romaji.tsv` | Romaji table (Mozc compatible; built-in table when missing) | Romaji table |
| `user_theme.txt` | Active custom user theme | Active custom user theme |
| `user_themes.txt` | Saved named user themes | Saved named user themes |
| `history.tsv` | Learned words | Learned words |
| `user_dict.tsv` | User dictionary (Mozc / MS IME compatible) | User dictionary |
| `reports.tsv` | Conversion reports | Conversion reports |
| `apps.txt` | Per-app input modes | — |
| `zenz.swz` | `%LOCALAPPDATA%\SweetIME\` | `$XDG_DATA_HOME/sweet-ime/` |

Example `config.txt`:
```ini
live_conversion = on   # show converted text while typing
suggestions = on       # show predictions while typing
show_typed_input = off # show romaji beside the composition (Windows)
neural = on            # rerank with the zenz model when installed
learning = on          # learn from chosen candidates
theme = default        # default, light, dark, sakura, matcha, midnight, lavender, dracula, adwaita, custom
gaming_mode = off      # animated rainbow border (Windows)
update_check = on      # check GitHub Releases
dates = on             # きょう / いま -> dates and times
emoji = on             # えがお -> 😊, かお -> kaomoji
calculator = on        # 1+2*3= -> 7
undo_commit = on       # Ctrl+Backspace right after commit restores reading
private_fields = on    # skip learning in password fields / private windows
english_words = on     # convert common English words mid-sentence
english_katakana = on  # prefer katakana loanword first (best -> ベスト)
shift_one_word = on    # auto-end Shift-started English words when Japanese follows
```
</details>

<details>
<summary>Android keyboard details</summary>

- Supports both **QWERTY** and **12-key kana / alphanumeric flick** layouts ([flick layout guide](assets/android/flick-guide.png)).
- Customize key height, number row, vibration, sound, Material 3 / Expressive themes, and custom TTF/OTF fonts.
- Optional **Simejiクラウド変換** (off by default, disabled in password/private fields; [Simeji privacy policy](https://simeji.me/privacy/)).
</details>

---

## Development

See the [contribution guide](CONTRIBUTING.md) for reporting issues and submitting pull requests.

<details>
<summary>Building from source & running tests</summary>

### Prerequisites
- Stable Rust (2024 edition)
- Mozc OSS dictionary checked out at `third_party/mozc-data` (revision pinned in `.github/workflows/windows.yml`)

### Windows
```powershell
rustup target add i686-pc-windows-msvc
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```
Produces `dist\` and `out\SweetIME-Setup.exe`. Build just the dictionary with `./scripts/build-dictionary.ps1 -Output data/system.dict`.

### Linux
```sh
# Nix build + D-Bus fcitx5 harness
nix build
nix develop --command sh crates/fcitx5/tests/run-dbus-harness.sh result

# Portable binary archive
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
Requires Android SDK (`platforms;android-36`, `build-tools;36.0.0`, NDK), JDK 17+, `cargo-ndk`, and Android Rust targets (`aarch64-linux-android`, `x86_64-linux-android`).

### Test Suite & CLI
```sh
cargo test --locked --workspace --all-targets -- --test-threads=1
```
- **Windows TSF in-process harness:** `cargo run --locked --release -p sweet-tsf --example harness`
- **Accuracy & latency eval:** `sweet-cli data/system.dict eval <evaluation.tsv>` (set `SWEET_ZENZ` to include neural reranking)
</details>

## Licenses

- **Code:** MIT ([LICENSE](LICENSE))
- **System dictionary (`system.dict`):** Derived from Mozc OSS (including IPAdic and Okinawa dictionary) and [SudachiDict](https://github.com/WorksApplications/SudachiDict) (Apache-2.0, see [assets/licenses/sudachidict.txt](assets/licenses/sudachidict.txt)).
- **English word list:** Derived from SCOWL / ESDB by Kevin Atkinson ([assets/licenses/english-words.txt](assets/licenses/english-words.txt)).
- **zenz model (optional, not bundled):** By Miwa Keita, licensed under CC BY-SA 4.0.
- **sweet-zenz model (optional, not bundled):** Fine-tuned from `zenz-v3.1-small`, licensed under CC BY-SA 4.0. See its [model card](https://huggingface.co/haruuuuu-1256/sweet-zenz-v1-small-gguf) for the training data and its licenses.
