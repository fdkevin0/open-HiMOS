# Contributing

Thanks for helping. This project talks to real power supplies, so correctness and safety beat
speed.

## Quick checklist

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo run -p himos-cli -- selftest     # no hardware required
```

All four must pass before a pull request is reviewed. CI runs the same matrix on Windows and
Ubuntu.

## Ground rules

- Keep `himos-core` free of OS/GUI dependencies; I/O belongs in `transport` or in the front ends.
- Every protocol change needs a test: golden frame vectors for framing, field checks for decoding,
  and an explicit note in `docs/protocol-dm850g.md` for anything still inferred.
- Write commands are dangerous. New write paths need a clear failure mode when the link is down
  (the GUI refuses commands while offline; the CLI reports errors).
- No `unwrap`/`expect` outside tests; `expect_used` is allowed for tests only.
- Documentation and UI text are English; UI capability stays limited to digital-PSU features.

## Hardware testing

<details>
<summary>Safety notes</summary>

- Close the official HiMOS client before running tools that open the serial port; it takes
  exclusive ownership.
- Write commands change fan behaviour immediately. Verify one command at a time and keep a fan
  mode you can tolerate.
- Never attach the PSU to a machine you cannot afford to disturb while testing custom curves or
  interval changes.
- If you test on a firmware variant other than `V28.25.06.09`, record model, firmware, and the
  observed frames in the pull request.

</details>

## Pull requests

1. Describe the problem and the behaviour change.
2. List the checks you ran (including whether real hardware was involved).
3. Update `docs/` when the protocol, CLI flags, or data files change.
4. Keep commits focused; the initial history is a single squashed release commit.

## License

Contributions are dual licensed under Apache-2.0 and MIT, matching the rest of the project.
