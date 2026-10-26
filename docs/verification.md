# Verification

## Offline checks

Run the [contribution checks](../CONTRIBUTING.md#checks); they do not need a connected PSU.

## Hardware diagnostics

Close the vendor client before opening the serial port.

```bash
cargo run -p himos-cli -- ports
cargo run -p himos-cli -- monitor --seconds 4
cargo run -p himos-gui -- --smoke 9
cargo run -p himos-gui -- --screenshot overview.png --settle 5
```

The GUI smoke check exits with status 1 if no frames arrive. Screenshots show the live UI;
without telemetry they show the waiting state. Check reconnect behaviour by unplugging and
reconnecting the device. Verify write commands individually on the target firmware.

## Current implementation

- History retains up to 7,200 samples in memory, samples fresh telemetry at most once per second,
  and rotates the CSV archive past 8 MiB. The chart starts empty on each launch.
- `--smoke` and `--screenshot` use the real worker and window; they need hardware for live data
  and are not part of CI. They disable archive writes and saved geometry restoration.
- `--data-dir` uses the supplied path directly. Otherwise the first writable location is chosen:
  `HIMOS_DATA_DIR`, then the platform path below, then the temporary directory's `open-HiMOS`,
  then `.open-HiMOS` under the current working directory.

| Platform | Platform path candidates, in order |
| --- | --- |
| Windows | `%APPDATA%\open-HiMOS`, `%LOCALAPPDATA%\open-HiMOS` |
| macOS | `$HOME/Library/Application Support/open-HiMOS` |
| Linux / other Unix | `$XDG_CONFIG_HOME/open-HiMOS` if set, otherwise `$HOME/.config/open-HiMOS` |
