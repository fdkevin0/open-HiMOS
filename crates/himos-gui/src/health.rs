//! Health classification: one source of truth for the warning and alarm limits.
//!
//! The numeric limits live here and are used by the status strip, the gauges,
//! the rail table and the alarm logic, so a threshold can never drift between
//! what is drawn and what is classified.

use eframe::egui::Color32;
use himos_core::dm850g::PsuStatus;

use crate::theme::palette;

/// Temperature at which the panel switches to amber.
pub(crate) const TEMP_WARN_C: f32 = 75.0;
/// Temperature at which the panel switches to red.
pub(crate) const TEMP_ALARM_C: f32 = 90.0;
/// Relative rail deviation that turns a reading amber (5%).
pub(crate) const DEV_WARN: f32 = 0.05;
/// Relative rail deviation that turns a reading red (10%).
pub(crate) const DEV_ALARM: f32 = 0.10;

/// Severity of one measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    /// Within limits.
    Ok,
    /// Outside the amber limit.
    Warning,
    /// Outside the red limit.
    Alarm,
}

impl Severity {
    /// Short label for tables.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warning => "WARN",
            Self::Alarm => "ALARM",
        }
    }

    /// Indicator colour.
    pub(crate) fn color(self) -> Color32 {
        match self {
            Self::Ok => palette().good,
            Self::Warning => palette().warn,
            Self::Alarm => palette().alarm,
        }
    }
}

/// Overall health shown in the status strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    /// No telemetry has arrived yet.
    Standby,
    /// Telemetry arrived but is older than the freshness window.
    Stale,
    /// All rails and temperatures within limits.
    Normal,
    /// A rail is off by more than 5% or a temperature is above 75 °C.
    Warning,
    /// A rail is off by more than 10% or a temperature is above 90 °C.
    Alarm,
}

impl Health {
    /// Short label for the status strip.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Standby => "STANDBY",
            Self::Stale => "STALE",
            Self::Normal => "NORMAL",
            Self::Warning => "WARNING",
            Self::Alarm => "ALARM",
        }
    }

    /// Indicator colour.
    pub(crate) fn color(self) -> Color32 {
        match self {
            Self::Standby => palette().muted,
            Self::Stale | Self::Warning => palette().warn,
            Self::Normal => palette().good,
            Self::Alarm => palette().alarm,
        }
    }
}

/// Nominal and measured values of the four rails.
pub(crate) fn rails(status: &PsuStatus) -> [(&'static str, f32, f32); 4] {
    [
        ("+12 V", 12.0, status.voltage_12v),
        ("+5 V", 5.0, status.voltage_5v),
        ("+3.3 V", 3.3, status.voltage_3v3),
        ("+5 VSB", 5.0, status.voltage_5vsb),
    ]
}

/// Relative deviation of `measured` from `nominal`.
pub(crate) fn deviation(nominal: f32, measured: f32) -> f32 {
    if nominal <= 0.0 {
        0.0
    } else {
        (measured - nominal) / nominal
    }
}

/// Classifies a relative rail deviation.
pub(crate) fn severity_of_deviation(deviation: f32) -> Severity {
    let magnitude = deviation.abs();
    if magnitude > DEV_ALARM {
        Severity::Alarm
    } else if magnitude > DEV_WARN {
        Severity::Warning
    } else {
        Severity::Ok
    }
}

/// Classifies a temperature in degrees Celsius.
pub(crate) fn severity_of_temperature(celsius: f32) -> Severity {
    if celsius >= TEMP_ALARM_C {
        Severity::Alarm
    } else if celsius >= TEMP_WARN_C {
        Severity::Warning
    } else {
        Severity::Ok
    }
}

/// Derives the overall health from a fresh status sample.
pub(crate) fn health_of(status: &PsuStatus) -> Health {
    let mut health = Health::Normal;
    for (_, nominal, measured) in rails(status) {
        match severity_of_deviation(deviation(nominal, measured)) {
            Severity::Alarm => return Health::Alarm,
            Severity::Warning => health = Health::Warning,
            Severity::Ok => {}
        }
    }
    for temperature in [status.temp_high_c, status.temp_low_c] {
        match severity_of_temperature(temperature) {
            Severity::Alarm => return Health::Alarm,
            Severity::Warning => health = Health::Warning,
            Severity::Ok => {}
        }
    }
    health
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a 28-byte status payload with the given 12 V and high-side temp.
    fn payload(voltage_12v_mv: u16, temp_high_q8: u16) -> Vec<u8> {
        let mut payload = vec![0u8; 28];
        payload[0..2].copy_from_slice(&3300u16.to_le_bytes());
        payload[2..4].copy_from_slice(&5000u16.to_le_bytes());
        payload[4..6].copy_from_slice(&voltage_12v_mv.to_le_bytes());
        payload[6..8].copy_from_slice(&5000u16.to_le_bytes());
        payload[10..12].copy_from_slice(&0x15u16.to_le_bytes());
        payload[12..14].copy_from_slice(&2000u16.to_le_bytes());
        payload[14..16].copy_from_slice(&500u16.to_le_bytes());
        payload[16..18].copy_from_slice(&temp_high_q8.to_le_bytes());
        payload[18..20].copy_from_slice(&(40u16 * 256).to_le_bytes());
        payload[20..22].copy_from_slice(&(30u16 * 256).to_le_bytes());
        payload[22..24].copy_from_slice(&2300u16.to_le_bytes());
        payload
    }

    fn status(voltage_12v_mv: u16, temp_high_q8: u16) -> PsuStatus {
        PsuStatus::parse(&payload(voltage_12v_mv, temp_high_q8)).expect("28-byte payload parses")
    }

    #[test]
    fn nominal_readings_are_normal() {
        assert_eq!(health_of(&status(12_000, 12_800)), Health::Normal);
    }

    #[test]
    fn voltage_deviation_escalates() {
        assert_eq!(health_of(&status(12_500, 12_800)), Health::Normal);
        assert_eq!(health_of(&status(12_900, 12_800)), Health::Warning);
        assert_eq!(health_of(&status(13_500, 12_800)), Health::Alarm);
    }

    #[test]
    fn temperature_escalates() {
        assert_eq!(health_of(&status(12_000, 76 * 256)), Health::Warning);
        assert_eq!(health_of(&status(12_000, 91 * 256)), Health::Alarm);
    }

    #[test]
    fn severity_uses_the_shared_limits() {
        assert_eq!(severity_of_deviation(DEV_WARN + 0.001), Severity::Warning);
        assert_eq!(severity_of_deviation(DEV_ALARM + 0.001), Severity::Alarm);
        assert_eq!(severity_of_temperature(TEMP_WARN_C), Severity::Warning);
        assert_eq!(severity_of_temperature(TEMP_ALARM_C), Severity::Alarm);
        assert_eq!(Severity::Warning.label(), "WARN");
    }
}
