//! `himos` command line tool: read telemetry and send control frames.
//!
//! The GUI counterpart lives in `himos-gui`; this binary is the scriptable
//! surface and the reference for the protocol.

use std::io::Write as _;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use himos_core::dm850g::{
    AuxTelemetry, CMD_AUX, CMD_INFO, CMD_SERIAL, CMD_STATUS, CurvePoint, DeviceInfo, FanMode,
    PsuStatus, RefreshInterval, SerialNumber, connect, init_sequence, set_custom_curve,
    set_fan_mode, set_refresh_interval,
};
use himos_core::transport::SerialLink;

mod calibrate;

#[derive(Debug, Parser)]
#[command(name = "himos", version, about = "Segotep DM-series PSU monitor")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Stream telemetry from the PSU (read-only unless a control flag is given).
    Monitor {
        /// Serial port (for example COM3 or /dev/ttyUSB0); autodetected when omitted.
        #[arg(long)]
        port: Option<String>,
        /// Seconds to keep reading after the handshake.
        #[arg(long, default_value_t = 10.0)]
        seconds: f64,
        /// Reporting interval in milliseconds (1000..=3000, step 500).
        #[arg(long)]
        interval: Option<u16>,
        /// Fan mode to apply after the handshake.
        #[arg(long)]
        fan_mode: Option<FanMode>,
        /// Clear the stored fault code.
        #[arg(long)]
        clear_fault: bool,
        /// Custom curve points as `TEMP,DUTY` (repeatable, applied in order).
        #[arg(long = "curve", value_name = "TEMP,DUTY")]
        curve: Vec<String>,
        /// Skip the vendor initialisation sequence.
        #[arg(long)]
        no_init: bool,
    },
    /// List serial ports that could host the PSU.
    Ports,
    /// Run an idle/load/tail calibration of the extended telemetry fields.
    Calibrate {
        /// Serial port (for example COM3 or /dev/ttyUSB0); autodetected when omitted.
        #[arg(long)]
        port: Option<String>,
        /// Seconds to sample before the load starts.
        #[arg(long, default_value_t = 8.0)]
        idle: f64,
        /// Seconds of CPU load.
        #[arg(long, default_value_t = 30.0)]
        load: f64,
        /// Seconds to sample after the load stops.
        #[arg(long, default_value_t = 10.0)]
        tail: f64,
        /// Optional CSV output path.
        #[arg(long)]
        csv: Option<String>,
    },
    /// Replay captured frames through the codec (no hardware needed).
    Selftest,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Monitor {
            port,
            seconds,
            interval,
            fan_mode,
            clear_fault,
            curve,
            no_init,
        } => monitor(&Options {
            port,
            seconds,
            interval,
            fan_mode,
            clear_fault,
            curve,
            no_init,
        }),
        Command::Ports => {
            let detected = SerialLink::detect().unwrap_or(None);
            let ports = SerialLink::available().context("enumerating serial ports")?;
            for info in ports {
                let mark = if detected
                    .as_ref()
                    .is_some_and(|psu| psu.port_name == info.port_name)
                {
                    "\t<- PSU"
                } else {
                    ""
                };
                println!("{}\t{:?}{mark}", info.port_name, info.port_type);
            }
            Ok(())
        }
        Command::Calibrate {
            port,
            idle,
            load,
            tail,
            csv,
        } => {
            let port = resolve_port(port.as_deref())?;
            calibrate::run(&calibrate::Options {
                port,
                idle_s: idle,
                load_s: load,
                tail_s: tail,
                csv,
            })
        }
        Command::Selftest => selftest(),
    }
}

/// Uses the requested port, or the autodetected PSU port when none was given.
fn resolve_port(port: Option<&str>) -> Result<String> {
    if let Some(name) = port {
        return Ok(name.to_owned());
    }
    let info = SerialLink::detect()
        .context("detecting the PSU port")?
        .context("no CH340 serial port found; connect the PSU or pass --port")?;
    println!("detected PSU on {}", info.port_name);
    Ok(info.port_name)
}

struct Options {
    port: Option<String>,
    seconds: f64,
    interval: Option<u16>,
    fan_mode: Option<FanMode>,
    clear_fault: bool,
    curve: Vec<String>,
    no_init: bool,
}

fn monitor(options: &Options) -> Result<()> {
    let port = resolve_port(options.port.as_deref())?;
    let link = SerialLink::open_retry(&port).with_context(|| format!("opening {port}"))?;
    let mut session = himos_core::Session::new(link);

    session.send(&connect()).context("sending connect")?;
    std::thread::sleep(Duration::from_millis(300));

    let interval = match options.interval {
        Some(ms) => RefreshInterval::try_new(ms).map_err(|err| anyhow::anyhow!(err.to_string()))?,
        None => RefreshInterval::default(),
    };
    if !options.no_init {
        for frame in init_sequence(interval, options.fan_mode.unwrap_or(FanMode::Auto)) {
            session.send(&frame).context("sending init frame")?;
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    if options.clear_fault {
        session
            .send(&himos_core::dm850g::clear_fault())
            .context("sending clear-fault")?;
        println!("sent: clear fault");
    }
    if let Some(mode) = options.fan_mode {
        session
            .send(&set_fan_mode(mode))
            .context("sending fan mode")?;
        println!("sent: fan mode {mode}");
    }
    if options.interval.is_some() {
        session
            .send(&set_refresh_interval(interval))
            .context("sending interval")?;
        println!("sent: interval {} ms", interval.millis());
    }
    if !options.curve.is_empty() {
        let points = parse_points(&options.curve)?;
        let frame = set_custom_curve(&points).context("building custom curve")?;
        session.send(&frame).context("sending custom curve")?;
        println!("sent: custom curve with {} points", points.len());
    }

    let deadline = Instant::now() + Duration::from_secs_f64(options.seconds);
    let mut buf = [0u8; 256];
    while Instant::now() < deadline {
        for frame in session.poll(&mut buf).context("reading frames")? {
            print_frame(&frame);
        }
    }
    Ok(())
}

fn print_frame(frame: &himos_core::Frame) {
    match frame.cmd() {
        CMD_STATUS => match PsuStatus::parse(frame.payload()) {
            Ok(status) => println!(
                "{:7.1}W  12V {:.3}V  5V {:.3}V  3.3V {:.3}V  5VSB {:.3}V  \
                 {:.1}V/{:.1}Hz  T {:.1}/{:.1}/{:.1}C",
                status.power_w,
                status.voltage_12v,
                status.voltage_5v,
                status.voltage_3v3,
                status.voltage_5vsb,
                status.input_voltage_v,
                status.input_frequency_hz,
                status.temp_high_c,
                status.temp_low_c,
                status.temp_ambient_c
            ),
            Err(err) => println!("status payload invalid: {err}"),
        },
        CMD_AUX => match AuxTelemetry::parse(frame.payload()) {
            Ok(aux) => println!(
                "        12V rail≈ {:6.1}W  low temp {:.1}C",
                aux.power_12v_w, aux.temp_low_c
            ),
            Err(err) => println!("aux payload invalid: {err}"),
        },
        CMD_INFO => match DeviceInfo::parse(frame.payload()) {
            Ok(info) => println!("device: {} / {}", info.model, info.vendor),
            Err(err) => println!("model payload invalid: {err}"),
        },
        CMD_SERIAL => println!("serial: {}", SerialNumber::parse(frame.payload()).sn),
        other => println!("cmd {other:#04x}: {}", hex(frame.payload())),
    }
    let _ = std::io::stdout().flush();
}

fn parse_points(raw: &[String]) -> Result<Vec<CurvePoint>> {
    let mut points = Vec::with_capacity(raw.len());
    for item in raw {
        let (temp, duty) = item
            .split_once(',')
            .with_context(|| format!("expected TEMP,DUTY, got {item:?}"))?;
        points.push(CurvePoint {
            temperature_c: temp.trim().parse().context("temperature")?,
            duty_percent: duty.trim().parse().context("duty")?,
        });
    }
    if points.is_empty() {
        bail!("at least one curve point is required");
    }
    Ok(points)
}

fn selftest() -> Result<()> {
    // Captured from HiMOS 1.6.1 on a DM-850G: model, serial, and one status frame.
    let stream = hex_to_bytes(
        "557e1b03444d2d3835304720441c0c4664f003520a477365676f746570a4ae\
         557e12055342525936423530363030363133352049ae\
         557e1e02f10caf13392fc7130000150bd20bf9010d349d43b121cc0800000000d9ae",
    );
    let mut reader = himos_core::FrameReader::new();
    let frames = reader.feed(&stream);
    if frames.len() != 3 {
        bail!("expected 3 frames, decoded {}", frames.len());
    }
    let status = PsuStatus::parse(frames[2].payload()).context("status payload")?;
    println!(
        "decoded 3 frames: power {:.1} W, +12V {:.3} V, temp high {:.1} C",
        status.power_w, status.voltage_12v, status.temp_high_c
    );
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn hex_to_bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_curve_points() {
        let raw = ["40,20".to_owned(), " 60 , 30 ".to_owned()];
        let points = parse_points(&raw).expect("both points are valid");
        assert_eq!(points.len(), 2);
        assert_eq!(
            points[1],
            CurvePoint {
                temperature_c: 60,
                duty_percent: 30
            }
        );
    }

    #[test]
    fn rejects_malformed_curve_points() {
        assert!(parse_points(&["40".to_owned()]).is_err());
        assert!(parse_points(&["40,abc".to_owned()]).is_err());
        assert!(parse_points(&["300,20".to_owned()]).is_err());
    }

    #[test]
    fn rejects_an_empty_curve() {
        assert!(parse_points(&[]).is_err());
    }
}
