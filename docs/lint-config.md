# Lint configuration (BeautifulRust)

Date: 2026-10-26 · Upstream: [morganlinton/BeautifulRust](https://github.com/morganlinton/BeautifulRust)
· Toolchain: cargo/clippy 1.99.1

## Summary

The workspace uses the upstream lints with one structural split: rules marked `[lib]` live in
`himos-core` (the only library), because Cargo does not allow a member to extend an inherited
`[lints]` table. Everything else is in `[workspace.lints]`.

```text
clippy.toml                     # upstream copy (max-fn-params-bools = 0, test allowances, …)
lints/lints.toml                # upstream copy, kept for diffing/upgrading
Cargo.toml                      # [workspace.lints]: upstream set minus the 5 [lib] rules
crates/himos-core/Cargo.toml    # complete [lints], including the 5 [lib] rules
crates/himos-cli/Cargo.toml     # [lints] workspace = true
crates/himos-gui/Cargo.toml     # [lints] workspace = true
```

Strict mode is the CI gate:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

## What was fixed while wiring the upstream configuration

| # | Issue | Fix |
| --- | --- | --- |
| 1 | only `lints/clippy.toml` existed; the root `clippy.toml` was missing, so `max-fn-params-bools` and test allowances were inactive | copied to the repository root |
| 2 | `[workspace.lints]` was a subset of upstream (missing `missing_docs`, `missing_panics_doc`, `missing_errors_doc`, `multiple_unsafe_ops_per_block`, `print_stdout`, `print_stderr`, `redundant_clone`, `rc_buffer`, `string_add`, `wildcard_imports`, `module_inception`, `enum_glob_use`) | filled from the current upstream file |
| 3 | `lints/lints.toml` was stale (2845 → 3560 bytes upstream) | downloaded again |
| 4 | `himos-core` used `#![deny(missing_docs)]` instead of lint configuration | attribute removed; lints own it |
| 5 | upstream now ships three skills (`rust-best-practices`, `rust-review`, `rust-port`) | installed locally for development, not vendored |

## Evidence that the configuration is active

1. **The root `clippy.toml` is read**: adding an unknown key made clippy fail with
   `unknown field name … clippy.toml:13`.
2. **Rules take effect**: a probe `#[test]` calling `Option::unwrap()` in `himos-core` failed
   `-D warnings` with `clippy::unwrap_used` (probe removed afterwards).
3. **Full gate passes**: `fmt`, `clippy -D warnings`, `test --all` (see repository CI).

## Known upstream/toolchain difference

Upstream `clippy.toml` sets `allow-unwrap-in-tests = true`, but this has no effect on
clippy 1.99.1: a `#[test]` inside `#[cfg(test)] mod` is still rejected by `unwrap_used`.

- This project uses `expect("…")` in tests (`expect_used = "allow"`, also the BeautifulRust
  recommendation), so the difference is invisible here.
- If a future toolchain honours the option, nothing needs to change.
- It is an upstream/config-vs-toolchain gap, not a misconfiguration in this repository.
