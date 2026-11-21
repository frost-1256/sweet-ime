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
- **Features**: live conversion, predictive suggestions (including
  mid-sentence), per-user learning, bunsetsu resizing, and F6–F10
  transliteration.

## Accuracy

Top-1 results on Mozc's `evaluation.tsv` (397 cases):

| Configuration                    | Top-1 | Mean latency |
|----------------------------------|-------|--------------|
| Lattice only                     | 69.5% | ~0.1 ms      |
| + zenz v3.1 xsmall (default)     | 79.6% | ~16 ms       |
| + zenz v3.1 small                | 84.1% | ~66 ms       |
| Mozc OSS (reference)             | 74.6% | —            |

## Build

Requirements:

- Rust stable with the MSVC toolchain
- The `i686-pc-windows-msvc` target, for the 32-bit DLL
- The Mozc OSS dictionary in `third_party/mozc-data`

To stage a build into `dist\`, run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```

## Install

Run the installer from an elevated PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -AddToLanguageList
```

This installs the files to `%ProgramFiles%\SweetIME`, registers both
architectures with `regsvr32`, and adds the IME to the Japanese keyboard
list. To remove it, run `scripts\uninstall.ps1`.

### Optional: neural reranking

The zenz model is not bundled, because it is licensed under
CC BY-SA 4.0. To enable reranking, download and convert it yourself.
Administrator rights are not needed:

```powershell
powershell -ExecutionPolicy Bypass -File "$env:ProgramFiles\SweetIME\get-zenz.ps1"
```

The script writes `%LOCALAPPDATA%\SweetIME\zenz.swz`, which the IME
loads the next time it is activated. Pass
`-Model Miwa-Keita/zenz-v3.1-small` to use the larger, more accurate
model. Without the model, the IME converts with the dictionary lattice
only.

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
| 半角/全角, 変換, 無変換   | Toggle / on / off                 |                                  |

## Settings

Settings are read from `%APPDATA%\SweetIME\config.txt` each time the IME
is activated:

```
live_conversion = on   # show converted text while typing
suggestions = on       # show predictions while typing
```

Learned words are stored in `%APPDATA%\SweetIME\history.tsv`.

## Development

- `cargo test --workspace`
- `target\release\examples\harness.exe` drives the real TSF DLL in-process
  without registering it.
- `sweet-cli data\system.dict eval <evaluation.tsv>` measures accuracy.
  Set `SWEET_ZENZ=data\zenz-v3.1-xsmall.swz` to include the reranker.

## Licenses

- **Code**: MIT; see `LICENSE`.
- **Dictionary** (`system.dict`, bundled): derived from the Mozc OSS
  dictionary. It includes IPAdic and the Okinawa dictionary, which keep
  their own licenses; see `dist\licenses\dictionary-oss.txt`.
- **zenz model** (not bundled): by Miwa Keita, licensed under
  CC BY-SA 4.0. Users download it themselves with `get-zenz.ps1`.
