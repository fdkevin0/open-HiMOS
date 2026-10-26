# Segotep DM-850G serial protocol (reverse-engineering notes)

Status: draft v0.2 · 2026-10-26 · target device: **DM-850G** (physical unit used for captures)

Evidence:

- Runtime capture of the official `HiMOS 1.6.1` Electron client (Windows), real DM-850G on
  COM3, CH340 `VID_1A86&PID_7523`: 5 host frames + 48 device frames, full byte log.
- 8,377 status frames from the client's own log, cross-checked frame-by-frame against the
  client's parsed values.

Tags: **[observed]** bytes seen directly; **[derived]** validated by regression over many frames;
**[unknown]** not yet confirmed.

## At a glance

Frame: `55 7E | LEN | CMD | PAYLOAD | CRC8 | AE`.

- `LEN = payload.len() + 2`, i.e. total frame length minus 4.
- CRC-8: polynomial `0x07`, init `0x00`, no reflection, no final XOR.
  - device → host: over `frame[0..len-2]` (includes the `55 7E` marker)
  - host → device: over `frame[2..len-2]` (starts at `LEN`)
- The documented status frame (`CMD=0x02`) carries rails, power, frequency and temperatures;
  a second frame (`CMD=0x04`) is partially calibrated and does not carry per-rail current.

<details>
<summary>1. Link layer</summary>

- USB from the PSU to an internal motherboard header, exposed as a WCH CH340 (not CH341, but it
  uses the WCH CH341SER driver).
- Serial settings: **115200 8N1**, no flow control. The vendor client used
  `autoOpen=true`, `highWaterMark=65536`; these are client options, not wire settings.
- Binary protocol: no text, no encryption, no authentication.
- Test unit: model `DM-850G`, serial number redacted in this document, firmware `V28.25.06.09`,
  850 W 80+ Gold.

</details>

## 2. Frame format [observed]

```text
55 7E | LEN | CMD | PAYLOAD (LEN-2 bytes) | CRC8 | AE
```

- `LEN` = 1 (CMD) + payload length + 1 (CRC8) = **frame length − 4**.
  Examples: a 34-byte status frame → `LEN=30`; the 8-byte handshake frame → `LEN=4`.
- Frame trailer is always `0xAE`.
- CRC coverage differs by direction and was verified byte-by-byte on every captured frame.

<details>
<summary>Raw capture examples</summary>

```text
host → 55 7E 04 04 00 00 F3 AE                      # connect request (CMD=0x04, payload 00 00)
dev  → CMD=0x03: 55 7E 1B 03 "DM-850G D" ... "Segotep" ... A4 AE
dev  → CMD=0x05: 55 7E 12 05 "SBRY…" (15 ASCII bytes + 0x20 padding) 49 AE
dev  → CMD=0x02: 34-byte status frame, payload 28 bytes
```

</details>

## 3. Session flow [observed]

```text
host → CMD=0x04 (00 00)            connect request
dev  → CMD=0x03                    model information
dev  → CMD=0x05                    serial number
host → CMD=0x1E / 0x13 / 0x16 / 0x04(35 00)   four initialisation commands (semantics open)
dev  → CMD=0x02 and CMD=0x04 alternating, about one frame each per second
```

The sequence above records the vendor client. This project's GUI and `monitor` send only
`connect()` unless a control is explicitly requested. `calibrate` still sends all four
initialisation commands: clear fault, auto fan mode, 1000 ms reporting, and `0x04(35 00)`.
It does not restore previous device settings.

### 3.1 Model frame `CMD=0x03`

- First 9 bytes: `DM-850G D` — the model string; the trailing `D` is probably a revision/region
  suffix.
- Next 9 bytes: `1C 0C 46 64 F0 03 52 0A 47`; changes after reboot, suspected firmware/session
  data **[unknown]**.
- Tail: vendor string `Segotep`, sometimes partially overwritten by the previous field.

### 3.2 Serial frame `CMD=0x05`

- 15 ASCII bytes plus `0x20` padding. The official client uses the serial number as the
  persistence key for `powerSupplyInfo`.

## 4. Status frame `CMD=0x02` (28-byte payload) [derived]

Validated against 8,377 frames from the client's own parser; maximum difference < 0.01.
Offsets are relative to the **payload start** (frame byte 4).

| Offset | Type | Scale | Field | Example |
| --- | --- | --- | --- | --- |
| 0..1 | LE16 | /1000 V | +3.3 V rail | `0x0CF1` → 3.313 V |
| 2..3 | LE16 | /1000 V | +5 V rail | `0x13AF` → 5.039 V |
| 4..5 | LE16 | /1000 V | +12 V rail | `0x2F39` → 12.089 V |
| 6..7 | LE16 | /1000 V | +5 VSB rail | `0x13C7` → 5.063 V |
| 8..9 | LE16 | — | reserved (always 0) | `0x0000` |
| 10..11 | LE16 | — | status word (3 values observed) | `0x0541`, `0x0B15` |
| 12..13 | LE16 | **/20 W** | output power | `0x0BD2` → 151.3 W |
| 14..15 | LE16 | /10 Hz | AC input frequency | `0x01F9` → 50.5 Hz |
| 16..17 | LE16 | /256 °C | high-side temperature | `0x349D` → 52.58 °C |
| 18..19 | LE16 | /256 °C | low-side temperature | `0x439D` → 67.6 °C |
| 20..21 | LE16 | /256 °C | ambient temperature | `0x21B1` → 33.7 °C |
| 22..23 | LE16 | /10 V | AC input voltage | `0x08CC` → 225.2 V |
| 24..27 | — | — | reserved (always 0) | |

## 5. Extended frame `CMD=0x04` (28-byte payload, partially calibrated)

A controlled-load experiment (12 CPU threads, 20 s, total power 132.3 W → 191.9 W, 34 paired
`0x02`/`0x04` samples) produced the following:

| Field (LE16 offset) | Idle → load | Conclusion |
| --- | --- | --- |
| f09 (offset 18) | 15318 → 15371 | **equal to low-side temperature from `0x02` in 34/34 pairs**; duplicate report |
| f06 (offset 12) | 1274.6 → 1878.2 (+47.4%, r=0.988) | **inferred 12 V rail power**, 0.1 W units: 127.5 W / 187.8 W; equals total power minus 3.3 V/5 V rails |
| f02 (offset 4) | 561.9 → 670.2 (+19.3%, r=0.639) | slow load-dependent variable, possibly 0.1 °C (transformer/VRM), unconfirmed |
| f00/f01/f03/f04/f05/f07/f08/f10…f13 | essentially constant | thresholds/config/counters, uncalibrated |

**Negative result**: `0x04` is **not** a per-rail current frame. A +60 W step would imply roughly
+5 A on the 12 V rail, far larger than any observed field change. This matches the vendor UI,
which shows only voltage, power and temperature.

> Consequence: DM-850G likely does not report per-rail current; estimate it with `I = P / V` if
> needed.

## 6. Control commands [observed]

| Function | Frame | Notes |
| --- | --- | --- |
| Fan auto | `55 7E 04 13 00 00 47 AE` | `CMD=0x13`, payload `00 00` |
| Fan silent | `55 7E 04 13 00 01 40 AE` | payload `00 01` |
| Fan overclock | `55 7E 04 13 00 02 49 AE` | payload `00 02` |
| Fan custom | `55 7E 04 13 00 03 4E AE` | payload `00 03` |
| Fan dust removal | `55 7E 04 13 00 04 5B AE` | payload `00 04` |
| Reporting interval | `55 7E 04 16 00 0n CRC AE` | `CMD=0x16`, CRC varies with `n`; `n=(ms-1000)/500`; 1000/1500/2000/2500/3000 → 0/1/2/3/4 |
| Custom fan curve | `55 7E 1D 1B …` | `CMD=0x1B`, see below |
| Clear stored fault | `55 7E 04 1E 00 00 D6 AE` | `CMD=0x1E` |

### 6.1 Custom fan curve `CMD=0x1B` (27-byte payload)

```text
55 7E 1D 1B
   00                                      # curve id
   03 05 08 0A 0D 0F 12 14 17 19 1C 1E     # 12-byte temperature table
   28 32 3C 46 4E 55 5D 64                 # 8-byte duty table (40,50,60,70,78,85,93,100)
   28 14 3C 1E 50 46                       # three intermediate points (40,20) (60,30) (80,70)
   A8 AE
```

- Endpoints `(0,0)` and `(100,100)` are implied by the device, matching the client's `store.json`
  `custom` array.
- The two constant tables still need more samples to be fully understood **[unknown]**.

## 7. Open questions

1. **`CMD=0x04` field units** — needs another controlled-load calibration.
2. **`0x04(35 00)` semantics** — fourth initialisation command; not reproduced from the UI.
3. **Custom-curve constant tables** — compare captures after editing curve points.
4. **Temperature source switching** — clicking in the UI produced no TX; display-only or
   device-confirmed.
5. Firmware upgrade flow and LCD/pump-screen links (separate transports, out of scope).

## 8. Reproducing the capture (for review purposes)

<details>
<summary>Method</summary>

- Confirmed via Electron fuses that `EnableNodeCliInspectArguments` is enabled, then attached
  `--inspect` to the main process.
- Ran a second instance with `__COMPAT_LAYER=RunAsInvoker` to avoid the UAC requirement, after
  clearing the `himos` singleton lock and using a separate `--user-data-dir`.
- Hooked `serialport`'s `SerialPort.prototype.{list,open,write,emit}` to record all bytes;
  `mode=pass` forwarded to the real port (official client must be closed), `mode=fake` injected
  synthetic frames for state-machine experiments.

</details>
