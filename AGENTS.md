# Repository Guidelines

## Project Structure

Sweet IME is a Rust 2024 workspace for Windows. The platform-independent conversion engine and input session live in `crates/engine`. The Windows Text Services Framework integration and candidate window live in `crates/tsf`. Neural reranking lives in `crates/zenz`. The `dictc` and `cli` crates compile the dictionary and evaluate conversions. The `settings`, `ui`, and `installer` crates provide the configuration app and Windows installer. Unit tests live alongside source, integration tests under `crates/*/tests`, and the TSF harness under `crates/tsf/examples`.

## Build and Development

Use stable Rust with MSVC on Windows. Run `rustup target add i686-pc-windows-msvc` for the 32-bit DLL. Put the Mozc checkout under `third_party/mozc-data` and use the revision pinned in `.github/workflows/windows.yml` to match CI.

- `powershell -ExecutionPolicy Bypass -File scripts/build.ps1` builds both DLLs, stages `dist/`, and verifies `out/SweetIME-Setup.exe`.
- Create `data/` if needed, copy `dist/system.dict` to `data/system.dict`, and run `cargo test --locked --workspace --all-targets` to match CI.
- After a release build, run `cargo run --locked --release -p sweet-tsf --example harness` for in-process Windows input checks. Set `SWEET_IME_DICT` and `SWEET_IME_ZENZ` to the generated dictionary and local model if needed.

## Coding and Testing

Use four-space Rust indentation, `snake_case` for functions, modules, and descriptive test names, and `UpperCamelCase` for types. Use rustfmt on edited Rust code and keep formatting changes scoped. Keep conversion logic in `engine` and Windows-specific behavior in `tsf`.

Use Rust's built-in test framework. Dictionary-backed session tests skip without `data/system.dict`. Model-backed tests need the local zenz files. For ranking changes, compare before-and-after accuracy and latency using the CLI `eval` command. Keep generated dictionaries, model files, and build outputs out of Git.

## Commits and Pull Requests

Use short, imperative commit subjects such as `Fix delayed candidate layout`. In pull requests, describe the behavior change, link related issues when available, and list the checks you ran. Include screenshots for visual changes and benchmark results for performance work. GitHub Actions runs Windows checks on pushes and pull requests. Version tags such as `v0.1.0` trigger release packaging.
