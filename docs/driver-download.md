# Windows USB-serial driver

The DM-850G uses a WCH CH340 USB-serial bridge. Install the CH341SER driver before
connecting with open-HiMOS.

## Download

Use the digital-PSU USB-serial driver entry on Segotep's official download page:

- [Chinese downloads](https://www.segotep.com/cn/service/down)
- [English downloads](https://www.segotep.com/EN/service/down)

## Connect

1. Install the driver following the installer's instructions.
2. Connect the PSU's USB cable; check that a serial port appears in Device Manager.
3. Close the vendor client, then run `cargo run -p himos-cli -- ports`.
4. Start monitoring with `cargo run -p himos-cli -- monitor`; use `--port COM3`
   (substituting the actual port) if automatic detection does not find the device.

The vendor client and open-HiMOS cannot own the same serial port simultaneously.
