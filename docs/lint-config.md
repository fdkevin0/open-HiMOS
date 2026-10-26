# Lint configuration

The effective configuration is [`clippy.toml`](../clippy.toml) and the Cargo manifests.
The rules are based on [BeautifulRust](https://github.com/morganlinton/BeautifulRust), with
project-specific choices recorded in those files.

| File | Scope |
| --- | --- |
| [Root Cargo.toml](../Cargo.toml) | Workspace lints inherited by CLI and GUI |
| [Core Cargo.toml](../crates/himos-core/Cargo.toml) | Full library lint table, including documentation and stdout/stderr rules |
| [clippy.toml](../clippy.toml) | Clippy thresholds and test allowances |
| [rust-toolchain.toml](../rust-toolchain.toml) | Stable Rust with rustfmt and Clippy |

The core table repeats shared rules because Cargo cannot extend an inherited lint table.
Update both tables when changing a shared rule. `expect_used` is allowed; `unwrap_used` warns.
The workspace manifest declares the minimum Rust version.

<details>
<summary>Validation and CI enforcement</summary>

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo doc -p himos-core --no-deps
```

[CI](../.github/workflows/ci.yml) runs these checks on Windows and Ubuntu, treats rustdoc
warnings as errors, and builds release binaries.

</details>
