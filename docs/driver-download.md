# Windows driver download (verified record)

Verified: 2026-10-26. All sources are the official Segotep website.

## Summary

On Windows the DM-series digital PSU is exposed through a **WCH CH341SER USB-serial driver**.
Segotep distributes it in the official download section as
`CH341-64bit` (the page title is localized; English entry: "CH341-64bit, DM series PSU must be driven"). The Windows client is
**HiMOS2**, whose installer also bundles the same driver.

## Official pages

- Chinese downloads: <https://www.segotep.com/cn/service/down>
- English downloads: <https://www.segotep.com/EN/service/down>
  (English entry name: `CH341-64bit(DM series PSU must be driven)`)

## Direct links (checked 2026-10-26, HTTP 200/206)

| Entry | File | Size | SHA-256 |
| --- | --- | --- | --- |
| CH341 64-bit driver, 2025-05-15 | <https://www.segotep.com/CN/uploads/avatar/2024/09/20240906174855109478.rar> | 179,316 B | `5C79A8C123D3FA03274FDCCC8FAD722AAF3B703DF35C39B4B42F0B5BFBB26DFD` |
| HiMOS2 (2026-03-27) | <https://www.segotep.com/cn/upload/form/130/202603/5bba2047-17e6-4ff2-a196-bb40f40195a8.rar> | 221,225,965 B | `3E7787B429D1CD95A8EE131069C08EF6ABCD3F825B3A4701016C4D130D0619DF` |
| HiMOS (older, 2025-06-04) | <https://www.segotep.com/cn/upload/form/130/202506/d76a4808-df3a-4fe2-9e3e-c55d37c32b07.rar> | 108,820,892 B | `615F5F5A81F6238DD9B5FEA5C29780F0994B697051BCD76C88B8FD03F0E1101B` |
| HiMOS Android app (2026-03-27) | <https://www.segotep.com/cn/upload/form/130/202603/edd06910-bc3f-4eb6-800e-1ba7c591f4f6.rar> | 64,709,062 B | `7F0428502F7911D41E7363282AAF455F885841EED52A224DC79C3F98410CA5DE` |

## Package contents (verified with UnRAR)

<details>
<summary>Archive listings</summary>

- Driver archive → a single `CH341-64bit.exe` (localized filename; 233,018 B, unsigned); it contains `CH341SER.INF`,
  `CH341SER.SYS`, `CH341S64.SYS`, `CH341S98.SYS`, `CH341SER.CAT`, `CH341PT.DLL`, `CH341SER.VXD`
  — i.e. the WCH CH341SER installer, 64-bit only.
- HiMOS2 archive → `HiMOS_1.6.1.exe` (206,144,054 B, NSIS, unsigned) + the same CH341 installer
  (byte-identical SHA-256) + a usage video.
- Older HiMOS archive → `HiMOS Setup 1.2.3.exe` + the driver installer.
- Android archive → `HiMOS_20260327_1.0.9.apk` + a usage video (not a Windows driver).

</details>

## Recommended install order

1. Run the CH341 installer, then reboot.
2. Connect the PSU's USB cable to an internal motherboard USB header; Device Manager should show
   a WCH CH341 serial device.
3. Install HiMOS2 if you want the vendor client. **Do not run it simultaneously with this
   project** — it owns the serial port.

## Notes

- Only the 64-bit CH341 driver is published by Segotep; for 32-bit Windows use the full CH341SER
  package from the chip vendor (wch.cn).
- The `KL-Center`, `Segotep Digital` and display-driver entries on the download page are for
  liquid coolers / LCD panels, not for the digital PSU.
