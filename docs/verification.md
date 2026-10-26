# Verification record

Device: Segotep DM-850G (CH340 `1A86:7523`, serial number redacted), Windows 11, Rust 1.99.1.
All checks below were run against the physical PSU unless marked offline.

## Summary

| Area | Evidence | Result |
| --- | --- | --- |
| Build / lints / tests | `cargo fmt`, `clippy -D warnings`, `cargo test --all` | clean, 57 tests passing |
| Documentation build | `cargo doc -p himos-core --no-deps` | 0 warnings |
| Protocol | golden vectors + 8,377 captured status frames | decoded values match vendor parser (< 0.01) |
| CLI | ports / selftest / monitor / write commands | works on COM3 |
| GUI | `--smoke`, in-app screenshots, history capture | frames arrive, charts render |
| Auto-detect | unplug/replug, CH340 VID/PID match | reconnects without `--port` |
| Themes | OS light/dark + accent, explicit overrides, persistence | verified |
| Window | restored geometry, fullscreen, native chrome | verified |
| History | live 1 Hz CSV archive, fixed-scale chart | verified |

<details>
<summary>Environment and commands used</summary>

```text
cargo fmt --all -- --check                                exit 0
cargo clippy --all-targets --all-features -- -D warnings   exit 0
cargo test --all                                          57 passed (core 20 / cli 3 / gui 34)
cargo doc -p himos-core --no-deps                         0 warnings
```

</details>

## 1. CLI

| Command | Result |
| --- | --- |
| `himos ports` | lists ports; COM3 identified as `UsbPort(1a86:7523, "wch.cn", "USB-SERIAL CH340")` |
| `himos selftest` | `decoded 3 frames: power 151.3 W, +12V 12.089 V, temp high 52.1 C`, exit 0 |
| `himos monitor --seconds 4` | auto-detects COM3, prints model/SN/rails/power/temperatures and the extended frame |
| `himos monitor --fan-mode silent` | prints `sent: fan mode silent`; stream keeps running (restored to `auto` afterwards) |
| `himos monitor --curve <117 points>` | returns a typed error instead of panicking (exit 1) |

<details>
<summary>Sample monitor output</summary>

```text
device: DM-850G D / Segotep
serial: SBRY6B50•••••
  140.1W  12V 12.102V  5V 5.039V  3.3V 3.313V  5VSB 5.062V  222.8V/50.5Hz  T 54.4/69.3/34.2C
```

</details>

## 2. GUI

`--smoke` drives the real event loop and serial worker without manual interaction:

```text
cargo run -p himos-gui -- --smoke 9
gui smoke: frames=22 connected=true port=COM3 power=158.2 model="DM-850G D"
exit 0
```

- Frames arrive (~2 per second), the port connects, telemetry decodes, and the process exits with
  status 0; zero frames would exit with status 1.
- `--screenshot <path> [--level …]` renders one frame with the application's own renderer, so
  documentation images are reproducible and independent of the desktop compositor.

## 3. Auto-detection

Detection matches the CH340 USB VID/PID (`1A86:7523`) exactly; if there is no exact match it only
falls back when exactly one serial port is identified as a CH340. The GUI scans every 2 s and
reconnects after a drop.

```text
himos ports
  COM3  UsbPort(1a86:7523, "wch.cn", "USB-SERIAL CH340")  <- PSU

himos-gui --smoke 9          # no --port
  gui smoke: frames=21 connected=true port=COM3 power=135.9 model="DM-850G D"
  exit 0
```

## 4. Issues found during verification (and fixed)

1. **Transient `Access denied` when reopening the port** — Windows had not released COM3 after the
   previous process exited. `SerialLink::open_retry` now retries three times (300 ms apart); CLI,
   GUI and calibration all use it.
2. **Extended-frame `12V rail` occasionally high right after reconnect** — the derived 12 V rail
   power is only stable after the first paired frames. The UI/CLI label it `12 V rail ≈` (GUI adds
   `inferred`), and the protocol notes keep the inference tag.

## 5. Themes and OS sync

- Windows system theme (`AppsUseLightTheme`) is followed by default; `--theme dark|light` forces a
  palette.
- System accent is read from `HKCU\…\DWM\ColorizationColor` (fallback `AccentColorMenu`); macOS
  uses `defaults read -g AppleAccentColor`; Linux has no system accent and falls back to the
  default for the active palette.
- Explicit accents (`--accent rose`, …) override the OS; the selection is persisted in
  `appearance.conf` and reloaded on the next start.
- Light mode darkens overly bright accents to keep text contrast.

<details>
<summary>Evidence</summary>

```text
AppsUseLightTheme = 1                          → default screenshot is light (background 255,255,255)
ColorizationColor = 0xc40078d4                 → #D47800 accent (darkened for the light palette)
--theme dark                                   → background 14,21,29
--accent rose                                  → rose accent, verified visually
mode=light / accent=violet in appearance.conf  → loaded on the next start without flags
```

The macOS accent parser is covered by unit tests, but the macOS branch has not been run on real
macOS hardware.

</details>

## 6. Window management and rendering

- Native decorations are preserved: minimize/maximize/close, Alt+F4, Snap Layouts, per-monitor
  DPI v2, dark title bar via `DwmSetWindowAttribute`.
- Geometry is persisted to `window.conf` and restored, clamped to the current monitor.
- `F11` toggles fullscreen, `Esc` leaves it.
- The native title bar mirrors the device and link state (`open-HiMOS · DM-850G · COM3`,
  `· searching`, `· link lost`).
- Rendering uses eframe glow (OpenGL 2.0+) with vsync; the footer shows `egui · glow · CPU x%`
  (measured ~0.5% at 2 Hz telemetry). Forcing `HardwareAcceleration::Off` fails on Windows, so no
  software-rendering switch is provided.

<details>
<summary>Geometry restore evidence</summary>

```text
window.conf: width=980 height=700 x=80 y=50 maximized=false
Win32 GetWindowRect after start: L=80 T=50 size=995x737
  (= requested inner size + system frame/title bar; position preserved)
```

</details>

## 7. History recording and chart

- One sample per second into a 7,200-sample ring buffer (2 hours) and a rotating CSV archive
  (rotated past 8 MB).
- `HISTORY` page: metric (POWER / +12V / TEMP / AC) and fixed-scale span (1M / 5M / 15M / 1H / ALL,
  default 1H); min/avg/max/last; pause, clear, export CSV.
- The x axis is pinned with `Plot::default_x_bounds(-span, 0)`, so it does not rescale while the
  buffer is filling; the y axis stays automatic.
- `--history-span` overrides the span (useful for screenshots); `--settle` delays the screenshot.

<details>
<summary>Live archive evidence</summary>

```text
6 s run, graceful close:
  history.csv   512 B, header + 3 samples at 1 Hz
  window.conf    58 B (geometry persisted)
HISTORY screenshot with --history-span 1m --settle 55: 50 samples, full-minute fixed axis
```

</details>

Data directory resolution: `--data-dir` / `HIMOS_DATA_DIR`, then `%APPDATA%\open-HiMOS`,
`%LOCALAPPDATA%\open-HiMOS`, the temp directory, and finally `<workspace>/.open-HiMOS`. On a normal
desktop the per-user directory is writable and used; the fallback exists for portable/read-only
setups and sandboxes.

## 8. Known limitations

- macOS: accent parsing is unit-tested, but no end-to-end run on real macOS hardware yet.
- Linux: no system accent colour; dark/light still follows the windowing system.
- The extended frame (`0x04`) remains partially calibrated; the charting/telemetry code does not
  depend on its unconfirmed fields.
- Write commands were validated one by one on the listed device/firmware. Firmware variants may
  behave differently; the protocol notes mark inferred fields explicitly.
