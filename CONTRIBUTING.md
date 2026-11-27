# Contributing to Sweet IME

[日本語](CONTRIBUTING.ja.md)

Bug reports, conversion examples, documentation improvements, and code changes are welcome.

## Open an issue

Check the [issue list](https://github.com/frost-1256/sweet-ime/issues) for similar reports, then pick the [template that matches your report](https://github.com/frost-1256/sweet-ime/issues/new/choose).

- **Bugs** should include your Sweet IME version, OS version, the app you were typing in, steps to reproduce, and what happened compared with what you expected.
- **Conversion errors or candidate ranking** reports should include the reading, the candidate you got, and the candidate you expected. Include the preceding text or your AI conversion setting if it affects the result.
- **Feature requests** should describe what you are trying to do and what would make it easier.

Replace personal details and private input history in examples, logs, and screenshots with information you can share publicly.

## Make a change

1. Fork the repository and create a working branch from `develop`.
2. Keep the change focused. Put shared conversion logic in `crates/engine` and platform-specific logic in the corresponding frontend.
3. Format the Rust code you changed with `rustfmt` and use a short, descriptive commit subject.
4. Verify the behavior you changed and normally open your PR against `main`. Check that the PR diff contains only your intended changes.

See [Development in the README](README.md#development) for Windows, Linux (fcitx5), macOS, and Android build instructions. Keep generated dictionaries, models, and build outputs out of Git.

## Run tests

Run the workspace tests serially.

```sh
cargo test --locked --workspace --all-targets -- --test-threads=1
```

Some conversion tests skip when `data/system.dict` is missing. If your change touches conversion, prepare a dictionary using the README instructions before running the tests. For a candidate ranking change, use the CLI's `eval` command to compare accuracy and latency before and after.

## Open a pull request

GitHub fills in the [English template](.github/pull_request_template.md) by default. For Japanese, add `template=japanese.md` to the PR creation URL or copy the [Japanese template](.github/PULL_REQUEST_TEMPLATE/japanese.md).

Explain why you made the change, what users will notice, which platforms it affects, and how you verified it. Link any related issues. Include screenshots for visual changes and measurement conditions and results for performance changes. Mention any checks you could not run.

Merging into `main` requires the GitHub Actions checks and CodeRabbit. Once CodeRabbit has reviewed the latest commit, read the inline comments. Fix valid findings or reply with your reasoning.
