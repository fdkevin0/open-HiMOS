//! DM-850G payloads and control commands.
//!
//! Field offsets and scaling come from runtime captures cross-checked against
//! the vendor client's own parser over 8377 frames; see
//! `docs/protocol-dm850g.md`.

use std::fmt;

use thiserror::Error;

use crate::frame::{Frame, FrameError, MAX_PAYLOAD};

/// Host to device: connect request.
pub const CMD_CONNECT: u8 = 0x04;
/// Device to host: model information.
pub const CMD_INFO: u8 = 0x03;
/// Device to host: serial number.
pub const CMD_SERIAL: u8 = 0x05;
/// Device to host: electrical status.
pub const CMD_STATUS: u8 = 0x02;
/// Device to host: extended telemetry.
pub const CMD_AUX: u8 = 0x04;
/// Host to device: fan mode.
pub const CMD_FAN_MODE: u8 = 0x13;
/// Host to device: reporting interval.
pub const CMD_REFRESH: u8 = 0x16;
/// Host to device: custom fan curve.
pub const CMD_FAN_CURVE: u8 = 0x1B;
/// Host to device: clear fault code.
pub const CMD_CLEAR_FAULT: u8 = 0x1E;

/// Temperature/duty tables the vendor sends with a custom curve.
const CURVE_TEMP_TABLE: [u8; 12] = [
    0x03, 0x05, 0x08, 0x0A, 0x0D, 0x0F, 0x12, 0x14, 0x17, 0x19, 0x1C, 0x1E,
];
const CURVE_DUTY_TABLE: [u8; 8] = [0x28, 0x32, 0x3C, 0x46, 0x4E, 0x55, 0x5D, 0x64];

fn u16_at(payload: &[u8], offset: usize) -> u16 {
    u16::from(payload[offset]) | (u16::from(payload[offset + 1]) << 8)
}

/// Payload too short for the requested parse.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("payload too short: {actual} bytes, need {needed}")]
pub struct ShortPayload {
    /// Bytes required.
    pub needed: usize,
    /// Bytes available.
    pub actual: usize,
}

/// Electrical status reported by [`CMD_STATUS`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PsuStatus {
    /// +3.3V rail voltage in volts.
    pub voltage_3v3: f32,
    /// +5V rail voltage in volts.
    pub voltage_5v: f32,
    /// +12V rail voltage in volts.
    pub voltage_12v: f32,
    /// +5VSB rail voltage in volts.
    pub voltage_5vsb: f32,
    /// Reserved flag word (always 0 on DM-850G).
    pub flags: u16,
    /// Vendor status word, only three values observed.
    pub state_word: u16,
    /// Total output power in watts.
    pub power_w: f32,
    /// AC input frequency in hertz.
    pub input_frequency_hz: f32,
    /// High-side temperature in degrees Celsius.
    pub temp_high_c: f32,
    /// Low-side temperature in degrees Celsius.
    pub temp_low_c: f32,
    /// Ambient temperature in degrees Celsius.
    pub temp_ambient_c: f32,
    /// AC input voltage in volts.
    pub input_voltage_v: f32,
}

impl PsuStatus {
    /// Parses a `0x02` payload.
    ///
    /// # Errors
    ///
    /// Returns [`ShortPayload`] when fewer than 24 bytes are available.
    pub fn parse(payload: &[u8]) -> Result<Self, ShortPayload> {
        if payload.len() < 24 {
            return Err(ShortPayload {
                needed: 24,
                actual: payload.len(),
            });
        }
        Ok(Self {
            voltage_3v3: f32::from(u16_at(payload, 0)) / 1000.0,
            voltage_5v: f32::from(u16_at(payload, 2)) / 1000.0,
            voltage_12v: f32::from(u16_at(payload, 4)) / 1000.0,
            voltage_5vsb: f32::from(u16_at(payload, 6)) / 1000.0,
            flags: u16_at(payload, 8),
            state_word: u16_at(payload, 10),
            power_w: f32::from(u16_at(payload, 12)) / 20.0,
            input_frequency_hz: f32::from(u16_at(payload, 14)) / 10.0,
            temp_high_c: f32::from(u16_at(payload, 16)) / 256.0,
            temp_low_c: f32::from(u16_at(payload, 18)) / 256.0,
            temp_ambient_c: f32::from(u16_at(payload, 20)) / 256.0,
            input_voltage_v: f32::from(u16_at(payload, 22)) / 10.0,
        })
    }
}

/// Model information reported by [`CMD_INFO`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Model string, for example `DM-850G D`.
    pub model: String,
    /// Vendor string when the payload carries one.
    pub vendor: String,
}

impl DeviceInfo {
    /// Parses a `0x03` payload.
    ///
    /// # Errors
    ///
    /// Returns [`ShortPayload`] when the payload cannot hold a model string.
    pub fn parse(payload: &[u8]) -> Result<Self, ShortPayload> {
        if payload.len() < 9 {
            return Err(ShortPayload {
                needed: 9,
                actual: payload.len(),
            });
        }
        let model = String::from_utf8_lossy(&payload[..9]).trim().to_owned();
        // The tail is normally `Segotep`, but a firmware field can overwrite part
        // of it; only a complete trailing vendor run counts as the vendor.
        let tail_start = payload
            .iter()
            .rposition(|&b| !(b.is_ascii_graphic() || b == b' '))
            .map_or(0, |index| index + 1);
        let tail = &payload[tail_start..];
        let vendor = if tail.to_ascii_lowercase().ends_with(b"segotep") {
            "Segotep".to_owned()
        } else {
            String::new()
        };
        Ok(Self { model, vendor })
    }
}

/// Serial number reported by [`CMD_SERIAL`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialNumber {
    /// Serial number with padding removed.
    pub sn: String,
}

impl SerialNumber {
    /// Parses a `0x05` payload.
    #[must_use]
    pub fn parse(payload: &[u8]) -> Self {
        let sn = String::from_utf8_lossy(payload)
            .trim_matches(|c: char| c == ' ' || c == '\0')
            .to_owned();
        Self { sn }
    }
}

/// Extended telemetry reported by [`CMD_AUX`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AuxTelemetry {
    /// Low-side temperature; identical to [`PsuStatus::temp_low_c`].
    pub temp_low_c: f32,
    /// 12V rail power in watts (inferred: total power minus the minor rails).
    pub power_12v_w: f32,
    /// Raw little-endian words for fields that remain unidentified.
    pub raw_words: Vec<u16>,
}

impl AuxTelemetry {
    /// Parses a `0x04` payload.
    ///
    /// # Errors
    ///
    /// Returns [`ShortPayload`] when fewer than 20 bytes are available.
    pub fn parse(payload: &[u8]) -> Result<Self, ShortPayload> {
        if payload.len() < 20 {
            return Err(ShortPayload {
                needed: 20,
                actual: payload.len(),
            });
        }
        let raw_words = payload
            .chunks_exact(2)
            .map(|pair| u16::from(pair[0]) | (u16::from(pair[1]) << 8))
            .collect();
        Ok(Self {
            temp_low_c: f32::from(u16_at(payload, 18)) / 256.0,
            power_12v_w: f32::from(u16_at(payload, 12)) / 10.0,
            raw_words,
        })
    }
}

/// Fan operating mode accepted by [`CMD_FAN_MODE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FanMode {
    /// Firmware controlled curve.
    Auto,
    /// Quiet curve.
    Silent,
    /// Performance curve.
    Overclock,
    /// User supplied curve.
    Custom,
    /// Dust removal burst.
    Dust,
}

impl FanMode {
    /// Payload byte sent to the device.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::Silent => 1,
            Self::Overclock => 2,
            Self::Custom => 3,
            Self::Dust => 4,
        }
    }

    /// Human readable label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Silent => "silent",
            Self::Overclock => "overclock",
            Self::Custom => "custom",
            Self::Dust => "dust",
        }
    }
}

impl fmt::Display for FanMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Unrecognised fan mode name.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("unknown fan mode: {0} (expected auto, silent, overclock, custom or dust)")]
pub struct ParseFanModeError(pub String);

impl std::str::FromStr for FanMode {
    type Err = ParseFanModeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "silent" => Ok(Self::Silent),
            "overclock" | "oc" => Ok(Self::Overclock),
            "custom" => Ok(Self::Custom),
            "dust" => Ok(Self::Dust),
            other => Err(ParseFanModeError(other.to_owned())),
        }
    }
}

/// Reporting interval accepted by [`CMD_REFRESH`]: 1000..=3000 ms in 500 ms steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshInterval {
    millis: u16,
    code: u8,
}

impl RefreshInterval {
    /// Smallest accepted interval.
    pub const MIN_MS: u16 = 1000;
    /// Largest accepted interval.
    pub const MAX_MS: u16 = 3000;
    /// Accepted step.
    pub const STEP_MS: u16 = 500;

    /// Validates and stores an interval in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns [`IntervalError`] when the value is out of range or not a
    /// multiple of [`Self::STEP_MS`].
    pub fn try_new(ms: u16) -> Result<Self, IntervalError> {
        let in_range = (Self::MIN_MS..=Self::MAX_MS).contains(&ms);
        if !in_range || (ms - Self::MIN_MS) % Self::STEP_MS != 0 {
            return Err(IntervalError { ms });
        }
        let steps = (ms - Self::MIN_MS) / Self::STEP_MS;
        let code = u8::try_from(steps).map_err(|_| IntervalError { ms })?;
        Ok(Self { millis: ms, code })
    }

    /// Stored interval in milliseconds.
    #[must_use]
    pub fn millis(self) -> u16 {
        self.millis
    }

    /// Encoded payload byte: `(ms - 1000) / 500`.
    #[must_use]
    pub fn code(self) -> u8 {
        self.code
    }
}

impl Default for RefreshInterval {
    fn default() -> Self {
        Self {
            millis: Self::MIN_MS,
            code: 0,
        }
    }
}

/// Interval outside the values the DM-850G accepts.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("interval {ms} ms is not one of 1000/1500/2000/2500/3000")]
pub struct IntervalError {
    /// Rejected value.
    pub ms: u16,
}

/// One point of a custom fan curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurvePoint {
    /// Temperature in whole degrees Celsius.
    pub temperature_c: u8,
    /// Duty cycle in percent.
    pub duty_percent: u8,
}

/// Builds the connect request.
#[must_use]
pub fn connect() -> Frame {
    Frame::new(CMD_CONNECT, [0x00, 0x00])
}

/// Builds the clear-fault request.
#[must_use]
pub fn clear_fault() -> Frame {
    Frame::new(CMD_CLEAR_FAULT, [0x00, 0x00])
}

/// Builds a fan mode request.
#[must_use]
pub fn set_fan_mode(mode: FanMode) -> Frame {
    Frame::new(CMD_FAN_MODE, [0x00, mode.code()])
}

/// Builds a reporting interval request.
#[must_use]
pub fn set_refresh_interval(interval: RefreshInterval) -> Frame {
    Frame::new(CMD_REFRESH, [0x00, interval.code()])
}

/// Builds a custom fan curve request; the device implies `(0,0)` and `(100,100)`.
///
/// # Errors
///
/// Returns [`FrameError::PayloadTooLong`] when the curve has too many points for
/// the protocol's one-byte length field (117 points already exceed it).
pub fn set_custom_curve(points: &[CurvePoint]) -> Result<Frame, FrameError> {
    let payload_len = 1 + CURVE_TEMP_TABLE.len() + CURVE_DUTY_TABLE.len() + points.len() * 2;
    if payload_len > MAX_PAYLOAD {
        return Err(FrameError::PayloadTooLong(payload_len));
    }
    let mut payload = Vec::with_capacity(payload_len);
    payload.push(0x00);
    payload.extend_from_slice(&CURVE_TEMP_TABLE);
    payload.extend_from_slice(&CURVE_DUTY_TABLE);
    for point in points {
        payload.push(point.temperature_c);
        payload.push(point.duty_percent);
    }
    Ok(Frame::new(CMD_FAN_CURVE, payload))
}

/// Frames the vendor client sends after connecting.
#[must_use]
pub fn init_sequence(interval: RefreshInterval, mode: FanMode) -> Vec<Frame> {
    vec![
        clear_fault(),
        set_fan_mode(mode),
        set_refresh_interval(interval),
        Frame::new(CMD_CONNECT, [0x35, 0x00]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    #[test]
    fn parses_captured_status() {
        let payload = (0..28).map(|_| 0u8).collect::<Vec<_>>();
        let status = PsuStatus::parse(&payload).expect("28 bytes is enough");
        assert!(status.power_w.abs() < f32::EPSILON);
    }

    #[test]
    fn rejects_short_status_payload() {
        assert!(PsuStatus::parse(&[0u8; 10]).is_err());
    }

    #[test]
    fn parses_model_string() {
        let payload = b"DM-850G D\x1c\x0c\x46\x64\xf0\x03\x52\x0a\x47segotep";
        let info = DeviceInfo::parse(payload).expect("payload holds a model");
        assert_eq!(info.model, "DM-850G D");
        assert_eq!(info.vendor, "Segotep");
    }

    #[test]
    fn command_frames_match_capture() {
        let cases: &[(Frame, &str)] = &[
            (set_fan_mode(FanMode::Auto), "557e0413000047ae"),
            (set_fan_mode(FanMode::Silent), "557e0413000140ae"),
            (set_fan_mode(FanMode::Overclock), "557e0413000249ae"),
            (set_fan_mode(FanMode::Custom), "557e041300034eae"),
            (set_fan_mode(FanMode::Dust), "557e041300045bae"),
            (
                set_refresh_interval(RefreshInterval::default()),
                "557e0416000087ae",
            ),
            (clear_fault(), "557e041e0000d6ae"),
            (connect(), "557e04040000f3ae"),
        ];
        for (frame, expected) in cases {
            let encoded = frame.encode().expect("captured frames fit the protocol");
            assert_eq!(hex(&encoded), *expected, "{frame:?}");
        }
    }

    #[test]
    fn custom_curve_matches_capture() {
        let points = [
            CurvePoint {
                temperature_c: 40,
                duty_percent: 20,
            },
            CurvePoint {
                temperature_c: 60,
                duty_percent: 30,
            },
            CurvePoint {
                temperature_c: 80,
                duty_percent: 70,
            },
        ];
        let frame = set_custom_curve(&points).expect("three points fit");
        assert_eq!(
            hex(&frame.encode().expect("frame fits")),
            "557e1d1b000305080a0d0f121417191c1e28323c464e555d6428143c1e5046a8ae"
        );
    }

    #[test]
    fn rejects_a_curve_that_exceeds_the_length_field() {
        let points = [CurvePoint {
            temperature_c: 40,
            duty_percent: 20,
        }; 200];
        assert!(matches!(
            set_custom_curve(&points),
            Err(FrameError::PayloadTooLong(_))
        ));
    }

    #[test]
    fn init_sequence_sends_the_vendor_order() {
        let cmds: Vec<u8> = init_sequence(RefreshInterval::default(), FanMode::Auto)
            .iter()
            .map(Frame::cmd)
            .collect();
        assert_eq!(
            cmds,
            vec![CMD_CLEAR_FAULT, CMD_FAN_MODE, CMD_REFRESH, CMD_CONNECT]
        );
    }

    #[test]
    fn intervalo_validation() {
        assert!(RefreshInterval::try_new(1000).is_ok());
        assert!(RefreshInterval::try_new(1500).is_ok());
        assert!(RefreshInterval::try_new(1200).is_err());
        assert!(RefreshInterval::try_new(500).is_err());
        assert_eq!(RefreshInterval::try_new(3000).expect("valid").code(), 4);
    }
}
