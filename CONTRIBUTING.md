# Contributing

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace
cargo test --locked -p himos-core --no-default-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked -p himos-core --no-deps
cargo run --locked -p himos-cli -- selftest
node --test .github/scripts/release.test.cjs
```

[CI](.github/workflows/ci.yml) runs on PRs and `main`: Linux runs quality checks, tests and
an additional Rust 1.95 compilation check; Windows runs native tests. Superseded PR runs are
cancelled. Release builds run separately.

Lint settings live in `clippy.toml` and the workspace Cargo manifest. The core manifest repeats
shared rules and adds library-specific lints because Cargo cannot extend an inherited table.
Update both tables when changing a shared rule.

## Changes

- Keep protocol logic in `himos-core`; serial I/O belongs in `transport` or the front ends.
- Test protocol changes with captured vectors; mark inferred fields in the
  [protocol notes](docs/protocol-dm850g.md).
- Validate external input without panicking. Write paths must report disconnected links.
- Keep documentation and UI text in English, and features limited to digital PSUs.
- Describe the behaviour change and checks in the PR; include firmware/model for hardware tests.

Close the vendor client before hardware testing. Write commands affect the PSU immediately;
verify one at a time on a machine you can safely disturb. See [verification](docs/verification.md).

## Releases

1. Update `workspace.package.version` in `Cargo.toml`, run `cargo check --workspace` to update
   `Cargo.lock`, and commit both in a PR.
2. Merge into `main`. Once CI passes, it creates `v<VERSION>` at that commit and dispatches dist.
3. dist runs native tests, builds separate CLI/GUI archives for Windows x64 and Linux x64, and
   publishes them with checksums to GitHub Releases.

Unchanged or already-published versions are skipped. Existing tags are never moved. The workflows
use `GITHUB_TOKEN` with contents/actions write permissions; no PAT or crates.io publishing is needed.

<details>
<summary>First release, retry and workflow maintenance</summary>

For an initial release without changing versions, run **Actions → Release** on tested `main`
with the current version tag. For a failed release, rerun that workflow run or dispatch against
its existing tag, not a newer branch. Wait for the previous attempt to finish before retrying.
Use `dry-run` as the input tag to build without publishing. PRs only check the dist plan.

Use [dist 0.33.1](https://axodotdev.github.io/cargo-dist/book/install.html) to edit and validate
release configuration:

```bash
dist generate
dist generate --check
dist plan
```

Edit `dist-workspace.toml` and `.github/dist-build-setup.yml`; do not hand-edit generated
`release.yml`. Linux archives use Ubuntu 22.04 system libraries. Windows binaries are unsigned.

</details>

Contributions are dual licensed under Apache-2.0 and MIT.
