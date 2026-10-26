//! `serialport`-backed link for Windows and Linux.

use std::io;
use std::time::Duration;

use serialport::{SerialPort, SerialPortInfo, SerialPortType};

use crate::link::Link;

/// USB vendor ID of the CH340 bridge inside DM-series PSUs.
pub const PSU_VID: u16 = 0x1A86;
/// USB product ID of the CH340 bridge inside DM-series PSUs.
pub const PSU_PID: u16 = 0x7523;

/// Default read timeout; the device pushes frames about once per second.
const READ_TIMEOUT: Duration = Duration::from_millis(200);
/// Pause between open attempts; Windows keeps the port briefly after the previous owner exits.
const RETRY_DELAY: Duration = Duration::from_millis(300);

/// A real serial connection to the PSU.
pub struct SerialLink {
    port: Box<dyn SerialPort>,
}

impl std::fmt::Debug for SerialLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerialLink")
            .field("path", &self.port.name())
            .finish_non_exhaustive()
    }
}

impl SerialLink {
    /// Opens `path` (for example `COM3` or `/dev/ttyUSB0`) at the DM-850G's
    /// 115200 8N1 settings.
    ///
    /// # Errors
    ///
    /// Returns [`serialport::Error`] when the port is missing or already open.
    pub fn open(path: &str) -> Result<Self, serialport::Error> {
        let port = serialport::new(path, crate::BAUD_RATE)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(READ_TIMEOUT)
            .open()?;
        Ok(Self { port })
    }

    /// Opens `path` with up to three attempts, 300 ms apart.
    ///
    /// Windows keeps the port briefly after the previous owner exits, so an
    /// immediate reopen reports "access denied" even though the port is free.
    ///
    /// # Errors
    ///
    /// Returns the last [`serialport::Error`] when every attempt fails.
    pub fn open_retry(path: &str) -> Result<Self, serialport::Error> {
        let mut last: Option<serialport::Error> = None;
        for attempt in 0..3u8 {
            match Self::open(path) {
                Ok(link) => return Ok(link),
                Err(err) => {
                    last = Some(err);
                    if attempt < 2 {
                        std::thread::sleep(RETRY_DELAY);
                    }
                }
            }
        }
        Err(last.unwrap_or_else(|| {
            serialport::Error::new(serialport::ErrorKind::Unknown, "serial port open failed")
        }))
    }

    /// Lists available serial ports.
    ///
    /// # Errors
    ///
    /// Returns [`serialport::Error`] when enumeration itself fails (missing
    /// driver, permission denied), which callers should surface instead of
    /// showing an empty list.
    pub fn available() -> Result<Vec<SerialPortInfo>, serialport::Error> {
        serialport::available_ports()
    }

    /// Returns true when `info` is the USB bridge a DM-series PSU uses.
    #[must_use]
    pub fn is_psu_port(info: &SerialPortInfo) -> bool {
        match &info.port_type {
            SerialPortType::UsbPort(usb) => usb.vid == PSU_VID && usb.pid == PSU_PID,
            _ => false,
        }
    }

    /// Returns true when `info` merely *looks* like a CH340 (vendor strings).
    ///
    /// Used only as a fallback when no exact VID/PID match exists; a CH340
    /// module could belong to another device, so [`Self::detect`] accepts it
    /// only when it is the sole candidate.
    #[must_use]
    pub fn looks_like_ch340(info: &SerialPortInfo) -> bool {
        let SerialPortType::UsbPort(usb) = &info.port_type else {
            return false;
        };
        let mentions_ch340 = |text: &Option<String>| {
            text.as_deref()
                .is_some_and(|value| value.to_ascii_lowercase().contains("ch340"))
        };
        mentions_ch340(&usb.product) || mentions_ch340(&usb.manufacturer)
    }

    /// Locates the PSU's serial port automatically.
    ///
    /// Prefers the exact CH340 VID/PID of the DM series and falls back to a
    /// single CH340-named USB port.
    ///
    /// # Errors
    ///
    /// Propagates serial port enumeration failures.
    pub fn detect() -> Result<Option<SerialPortInfo>, serialport::Error> {
        let mut fallback = Vec::new();
        for info in Self::available()? {
            if Self::is_psu_port(&info) {
                return Ok(Some(info));
            }
            if Self::looks_like_ch340(&info) {
                fallback.push(info);
            }
        }
        Ok(if fallback.len() == 1 {
            fallback.pop()
        } else {
            None
        })
    }
}

impl Link for SerialLink {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.port.read(buf) {
            Ok(n) => Ok(n),
            Err(err) if err.kind() == io::ErrorKind::TimedOut => Ok(0),
            Err(err) => Err(err),
        }
    }

    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.port.write_all(data)?;
        self.port.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serialport::UsbPortInfo;

    fn usb_port(port: &str, vid: u16, pid: u16, product: Option<&str>) -> SerialPortInfo {
        SerialPortInfo {
            port_name: port.to_owned(),
            port_type: SerialPortType::UsbPort(UsbPortInfo {
                vid,
                pid,
                serial_number: None,
                manufacturer: Some("wch.cn".to_owned()),
                product: product.map(ToOwned::to_owned),
            }),
        }
    }

    #[test]
    fn recognises_the_ch340_bridge() {
        let info = usb_port("COM3", PSU_VID, PSU_PID, Some("USB-SERIAL CH340 (COM3)"));
        assert!(SerialLink::is_psu_port(&info));
    }

    #[test]
    fn rejects_other_usb_devices() {
        let info = usb_port("COM9", 0x0403, 0x6001, Some("FT232R USB UART"));
        assert!(!SerialLink::is_psu_port(&info));
        assert!(!SerialLink::looks_like_ch340(&info));
    }

    #[test]
    fn generic_ch340_is_only_a_fallback() {
        let info = usb_port("COM8", 0x1234, 0x5678, Some("USB Serial CH340"));
        assert!(!SerialLink::is_psu_port(&info));
        assert!(SerialLink::looks_like_ch340(&info));
    }

    #[test]
    fn non_usb_ports_are_ignored() {
        let info = SerialPortInfo {
            port_name: "COM1".to_owned(),
            port_type: SerialPortType::Unknown,
        };
        assert!(!SerialLink::is_psu_port(&info));
        assert!(!SerialLink::looks_like_ch340(&info));
    }
}
