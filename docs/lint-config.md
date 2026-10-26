# Lint configuration (BeautifulRust)

Date: 2026-10-26 · Upstream: [morganlinton/BeautifulRust](https://github.com/morganlinton/BeautifulRust)
· Toolchain: cargo/clippy 1.99.1

## Summary

The effective configuration is the root `clippy.toml` plus `[workspace.lints]`. One structural
split is required: rules marked `[lib]` live in `himos-core` (the only library), because Cargo
does not allow a member to extend an inherited `[lints]` table.

```text
clippy.toml                     # upstream clippy settings (max-fn-params-bools = 0, …)
Cargo.toml                      # [workspace.lints]: upstream set minus the 5 [lib] rules
crates/himos-core/Cargo.toml    # complete [lints], including the 5 [lib] rules
crates/himos-cli/Cargo.toml     # [lints] workspace = true
crates/himos-gui/Cargo.toml     # [lints] workspace = true
```

A local `lints/` snapshot of the upstream files used to be kept for diffing. It was removed: it
was stale/mismatched and could be mistaken for the configuration actually in effect. The files
above are the single source of truth.

Strict mode is the CI gate:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

## Why the split

Cargo rejects a member manifest that reopens an inherited lint table (`failed to load manifest`).
The five library-only rules therefore live in the only library crate, while all other rules live
in the workspace table.

## Evidence that the configuration is active

1. **The root `clippy.toml` is read**: adding an unknown key made clippy fail with
   `unknown field name … clippy.toml:13`.
2. **Rules take effect**: a probe `#[test]` calling `Option::unwrap()` in `himos-core` failed
   `-D warnings` with `clippy::unwrap_used` (probe removed afterwards).
3. **The full gate passes** in CI (`fmt`, `clippy -D warnings`, `test --all`, `cargo doc`).

## Known upstream/toolchain difference

Upstream `clippy.toml` sets `allow-unwrap-in-tests = true`, but this has no effect on
clippy 1.99.1: a `#[test]` inside `#[cfg(test)] mod` is still rejected by `unwrap_used`.

- This project uses `expect("…")` in tests (`expect_used = "allow"`, also the BeautifulRust
  recommendation), so the difference is invisible here.
- If a future toolchain honours the option, nothing needs to change.
- It is an upstream/config-vs-toolchain gap, not a misconfiguration in this repository.
