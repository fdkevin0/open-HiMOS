# Changelog

## 0.1.0 — 2026-10-27

Initial public release.

- `himos-core`: DM-850G framing/CRC (`0x07`), status (`0x02`) and extended (`0x04`) telemetry,
  model/serial frames, fan mode, reporting interval, custom curve and clear-fault commands,
  serialport transport with retry and CH340 auto-detection.
- `himos-cli`: `ports`, `monitor`, `calibrate`, `selftest`; auto-detect by default, `--port`
  override for troubleshooting.
- `himos-gui`: four progressive-disclosure pages (Overview / Details / Advanced / History),
  native Windows/macOS/Linux window management, dark/light palettes with OS theme and accent
  sync, fixed-scale history chart with rotating CSV archive, smoke and screenshot self-test
  hooks.
- Documentation: protocol notes with evidence tags, verification record, lint configuration,
  Windows driver download record.
- Tooling: BeautifulRust lints, Windows + Linux CI, dual MIT/Apache-2.0 licensing.
