# open-HiMOS

[![CI](https://github.com/fdkevin0/open-HiMOS/actions/workflows/ci.yml/badge.svg)](https://github.com/fdkevin0/open-HiMOS/actions/workflows/ci.yml)

Open-source Rust controller for Segotep DM-series digital PSUs, with the **DM-850G** as the
primary target. One protocol core, two front ends: a progressive-disclosure desktop GUI and a
scriptable CLI. Windows and Linux are supported.

This project exists because the vendor's Windows client is closed and Windows-only. The wire
protocol was recovered from runtime observation (see [protocol notes](docs/protocol-dm850g.md))
and is implemented here as a typed, tested Rust library.

## Quick start

Requirements: Rust stable (MSRV 1.95), a CH340 USB-serial driver, and a supported PSU.

```bash
cargo run -p himos-cli -- selftest    # offline replay of captured frames
cargo run -p himos-cli -- ports       # list serial ports, mark the detected PSU
cargo run -p himos-cli -- monitor     # auto-detect and read telemetry
cargo run -p himos-gui                # desktop GUI (auto-connects)
```

Close the official HiMOS client first: it owns the serial port. The GUI re-scans every two
seconds, so unplug/replug does not require restarting it.

## What it does

- **Read-only telemetry**: +3.3 V / +5 V / +12 V / +5 VSB, total power, AC voltage/frequency,
  three temperatures.
- **Control**: fan mode (auto/silent/overclock/custom/dust), reporting interval, custom fan curve,
  clear stored fault.
- **Progressive disclosure**: Overview (power, rails, temperatures and device health) → Details (measurements and registers) →
  Advanced (write commands + event log) → History (recorded telemetry and trend chart).
- **Scope**: digital-PSU capabilities only. No host CPU/GPU telemetry, no LCD or pump-screen
  integration.

## Interfaces

### CLI

| Command | Purpose |
| --- | --- |
| `himos ports` | enumerate serial ports and mark the detected PSU |
| `himos monitor` | auto-detect, stream parsed telemetry (optionally write fan mode/curve) |
| `himos calibrate` | extended-frame calibration; changes PSU settings and creates CPU load |
| `himos selftest` | offline replay of captured frames; no hardware needed |

Monitoring sends only the handshake unless a control flag is supplied. Run
`cargo run -p himos-cli -- monitor --help` for options.

Calibration clears the stored fault, sets auto fan mode and a 1000 ms reporting interval,
sends the unconfirmed `0x04(35 00)` command, and creates CPU load. It does not restore settings.

### GUI

The GUI opens on Overview. Keys: 1/2/3/4 switch pages, F11 toggles fullscreen, Esc leaves it.
Run `cargo run -p himos-gui -- --help` for theme, port, history and capture options.
See [verification](docs/verification.md) for hardware diagnostics and data storage.

## How it is organised

| Crate | Responsibility |
| --- | --- |
| [`crates/himos-core`](crates/himos-core) | protocol core: framing, CRC, DM-850G fields, commands, serial transport — no GUI dependency |
| [`crates/himos-cli`](crates/himos-cli) | `himos` CLI: monitor / calibrate / ports / selftest |
| [`crates/himos-gui`](crates/himos-gui) | egui/eframe desktop panel with four disclosure levels |

The GUI keeps the serial port on a worker thread and exchanges owned messages through channels.
The library uses `std`; its optional `serial` feature supplies the serial transport.
Disable default features to use the codec without that transport.

## Documentation

| Document | Read it when you want to… |
| --- | --- |
| [Protocol: DM-850G](docs/protocol-dm850g.md) | understand frames, fields, commands, and what is still unknown |
| [Verification](docs/verification.md) | run offline checks and hardware diagnostics |
| [Driver download](docs/driver-download.md) | install the CH340 driver on Windows |
| [Contributing](CONTRIBUTING.md) | run checks, change code, or publish a release |

Linux builds need `pkg-config libudev-dev libxkbcommon-dev libwayland-dev libx11-dev
libxcursor-dev libxrandr-dev libxi-dev` (see [CI](.github/workflows/ci.yml)).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you shall be dual licensed as above, without additional terms.

## Disclaimer

The protocol was obtained for interoperability by observing a device you own. Write commands
change real PSU behaviour (fan mode, fan curve, stored fault); verify each command before relying
on it. Use at your own risk; the authors are not responsible for any damage or loss.

Segotep, HiMOS and DM-850G are trademarks of their respective owners. This project is not
affiliated with or endorsed by Segotep.
