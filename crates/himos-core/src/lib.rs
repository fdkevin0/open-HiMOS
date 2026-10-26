//! Protocol core for Segotep DM-series digital power supplies (DM-850G first).
//!
//! The crate is GUI- and OS-agnostic: [`frame`] handles the wire format,
//! [`dm850g`] the payloads and control commands, and [`link`] pairs a byte
//! transport with the frame splitter. The optional `serial` feature adds a
//! `serialport`-backed [`transport::SerialLink`] that works on Windows and Linux.

pub mod dm850g;
pub mod frame;
pub mod link;
#[cfg(feature = "serial")]
pub mod transport;

pub use dm850g::{
    AuxTelemetry, CurvePoint, DeviceInfo, FanMode, PsuStatus, RefreshInterval, SerialNumber,
};
pub use frame::{Frame, FrameError, FrameReader};
pub use link::{Link, Session};

/// Baud rate the DM-850G uses (8N1, no flow control).
pub const BAUD_RATE: u32 = 115_200;
