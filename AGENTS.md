# Repository Guidelines

## Project Structure

Sweet IME is a Rust 2024 workspace for Windows, Linux (fcitx5), and Android.

- `crates/engine` contains the shared conversion engine and input session. `crates/zenz` implements neural reranking. The `dictc` and `cli` crates compile dictionaries and evaluate conversions.
- `crates/tsf` implements Windows TSF and the candidate window as `sweet_tsf_core.dll`. The registered entry DLL in `crates/tsf-shim` loads the core named in `core.txt` and switches running apps to a new core on focus.
- `crates/fcitx5` contains the Linux addon's Rust C ABI and CMake-built C++ frontend. `flake.nix` packages the addon, dictionary, and settings app.
- `crates/android` provides the Rust JNI bridge. `android/` contains the Android keyboard, settings, and APK build script.
- The `settings` and `ui` crates provide the desktop settings app and shared widgets. `crates/installer` packages the Windows app.

Unit tests live alongside source, integration tests under `crates/*/tests`, and the TSF harness under `crates/tsf/examples`.

## Build and Development

Use stable Rust with MSVC on Windows. Run `rustup target add i686-pc-windows-msvc` for the 32-bit DLL. Put the Mozc checkout under `third_party/mozc-data` and use the revision pinned in `.github/workflows/windows.yml` to match CI.

- `powershell -ExecutionPolicy Bypass -File scripts/build.ps1` builds both DLLs, stages `dist/`, and verifies `out/SweetIME-Setup.exe`.
- Build `data/system.dict` with `./scripts/build-dictionary.ps1 -Output data/system.dict` or copy it from `dist/system.dict`. Run `cargo test --locked --workspace --all-targets -- --test-threads=1` to match CI. Serial execution avoids CPU contention in dictionary-backed tests that exercise time-limited typo correction.
- On Linux, `nix build` builds the fcitx5 addon, dictionary, and settings app. Run `nix develop --command sh crates/fcitx5/tests/run-dbus-harness.sh result` to check the addon in a private fcitx5 instance.
- Build the Android APK with `powershell -ExecutionPolicy Bypass -File android/build.ps1`. Add `-Install` when testing on a connected device or emulator. See the README for the SDK and toolchain requirements.
- After a release build, run `cargo run --locked --release -p sweet-tsf --example harness` for in-process Windows input checks. Set `SWEET_IME_DICT` and `SWEET_IME_ZENZ` to the generated dictionary and local model if needed.
- Use GitHub Actions for full cross-platform builds and installer artifacts. Local Cargo builds and tests can regenerate the contents of `target/debug`.

## Coding and Testing

Use four-space Rust indentation, `snake_case` for functions, modules, and descriptive test names, and `UpperCamelCase` for types. Use rustfmt on edited Rust code and keep formatting changes scoped. Keep shared conversion logic in `engine` and platform integration in the corresponding frontend.

Use Rust's built-in test framework. Dictionary-backed session tests skip without `data/system.dict`. Model-backed tests need the local zenz files. For ranking changes, compare before-and-after accuracy and latency using the CLI `eval` command. Keep generated dictionaries, model files, and build outputs out of Git.

## Commits and Pull Requests

Work on `develop` and open PRs into `main`. Use short, imperative commit subjects such as `Fix delayed candidate layout`. Describe the behavior change, link related issues when available, and list the checks you ran. Include screenshots for visual changes and benchmark results for performance work.

When a PR brings a contribution from `develop` into `main`, link the original PR in its description so release notes can credit the original contributor.

Merge PRs from `develop` into `main` with a merge commit (`gh pr merge <n> --merge`), not a squash merge. A squash merge leaves the `develop` commits outside the history of `main`, so later PRs list those commits again and show unrelated authors as participants. After the merge, fast-forward `develop` to `main`.

Keep external contributors in the commit history. Merge their PRs into `develop` directly, so the merge or squash commit keeps them as the author and the merge commit into `main` carries it over. If you must recreate a contribution in a separate PR, add a `Co-authored-by: Name <email>` trailer for the original author, using the email from their commits or their GitHub noreply address.

GitHub Actions tests Windows, Android, and Linux. Enable auto-merge per PR when requested to merge after checks. The protected `main` branch requires the six platform CI checks and CodeRabbit.

A passing CodeRabbit check does not mean the review found nothing. CodeRabbit can post review comments after its check succeeds, and auto-merge does not wait for them. Before you merge a PR or enable auto-merge, wait until CodeRabbit has reviewed the latest pushed commit. Read its inline comments with `gh api repos/{owner}/{repo}/pulls/<n>/comments`. Fix each valid finding, or reply to say why you skipped it. Repeat this after every push to the PR. Never tag a release from a merge that has unaddressed review comments.

## Releases

Release timing is a separate, manual decision. Use the patch version for bug fixes and the minor version for compatible new features during the `0.x` series.

1. Bump the workspace version in `Cargo.toml`, the Sweet IME workspace entries in `Cargo.lock`, and the Nix package version in `nix/package.nix`. Merge through a PR and confirm that `Release eligibility` passes for the merged commit on `main`.
2. Create an annotated tag with `git tag -a vX.Y.Z <main-commit>`. Put the title on the first line, then a blank line and Markdown bullets for the user-visible changes since the previous release. Name the platforms affected by platform-specific changes.
3. Push that tag to start the release workflow. After it passes, verify that GitHub Releases shows the summary above the generated changelog and includes `SweetIME-Setup.exe` and `SweetIME-Android.apk`.
4. Check the author and PR link for each changelog entry that came through a separate PR into `main`. Confirm the original author's GitHub login with `gh pr view <original-pr> --json author,url`. If the generated entry credits the maintainer who opened the follow-up PR, update it to mention the original contributor and link their PR. Save the updated notes with `gh release edit <tag> --notes-file <file>` and verify the published attribution.
