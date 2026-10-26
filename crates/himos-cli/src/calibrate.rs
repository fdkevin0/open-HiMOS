//! Load-phase calibration used to identify the extended telemetry fields.
//!
//! Runs the PSU through idle, CPU-load, and tail phases, records every
//! `0x02`/`0x04` frame, then reports how each extended field moves with power.

use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Status and aux frames must arrive within this window to be paired.
const AUX_PAIR_WINDOW: Duration = Duration::from_millis(100);

use anyhow::{Context, Result};
use himos_core::Session;
use himos_core::dm850g::{
    AuxTelemetry, CMD_AUX, CMD_STATUS, FanMode, PsuStatus, RefreshInterval, connect, init_sequence,
};
use himos_core::transport::SerialLink;
/// Where a sample was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Before any synthetic load.
    Idle,
    /// While CPU load threads run.
    Load,
    /// After the load stopped.
    Tail,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Load => "load",
            Self::Tail => "tail",
        }
    }
}

/// One paired status/aux observation.
#[derive(Debug, Clone)]
struct Sample {
    phase: Phase,
    power_w: f32,
    aux: Vec<u16>,
}

/// Calibration options from the CLI.
#[derive(Debug, Clone)]
pub(crate) struct Options {
    /// Serial port.
    pub port: String,
    /// Idle seconds before load.
    pub idle_s: f64,
    /// Load seconds.
    pub load_s: f64,
    /// Tail seconds after load.
    pub tail_s: f64,
    /// Optional CSV output path.
    pub csv: Option<String>,
}

/// Runs the calibration and prints a field ranking.
///
/// # Errors
///
/// Returns an error when the port cannot be opened or the protocol fails.
pub(crate) fn run(options: &Options) -> Result<()> {
    let link = SerialLink::open_retry(&options.port)
        .with_context(|| format!("opening {}", options.port))?;
    let mut session = Session::new(link);
    session.send(&connect()).context("handshake")?;
    std::thread::sleep(Duration::from_millis(300));
    for frame in init_sequence(RefreshInterval::default(), FanMode::Auto) {
        session.send(&frame).context("init")?;
        std::thread::sleep(Duration::from_millis(50));
    }

    let stop = Arc::new(AtomicBool::new(false));
    let start = Instant::now();
    let load_start = start + Duration::from_secs_f64(options.idle_s);
    let load_end = load_start + Duration::from_secs_f64(options.load_s);
    let finish = load_end + Duration::from_secs_f64(options.tail_s);

    let mut loaders = Vec::new();
    let mut samples: Vec<Sample> = Vec::new();
    let mut latest_aux: Option<(Instant, Vec<u16>)> = None;
    let mut buf = [0u8; 256];

    while Instant::now() < finish {
        let now = Instant::now();
        let phase = if now < load_start {
            Phase::Idle
        } else if now < load_end {
            Phase::Load
        } else {
            Phase::Tail
        };
        if phase == Phase::Load && loaders.is_empty() {
            println!("starting CPU load for {} s", options.load_s);
            for _ in 0..std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
            {
                let stop = Arc::clone(&stop);
                loaders.push(std::thread::spawn(move || burn(&stop)));
            }
        }
        if now >= load_end && !loaders.is_empty() {
            stop.store(true, Ordering::Relaxed);
            for handle in loaders.drain(..) {
                let _ = handle.join();
            }
        }

        for frame in session.poll(&mut buf).context("reading")? {
            match frame.cmd() {
                CMD_AUX => {
                    if let Ok(aux) = AuxTelemetry::parse(frame.payload()) {
                        latest_aux = Some((Instant::now(), aux.raw_words));
                    }
                }
                CMD_STATUS => {
                    if let Ok(status) = PsuStatus::parse(frame.payload()) {
                        if let Some((at, aux)) = latest_aux.take() {
                            // Only pair frames that arrived close together; a
                            // stale aux frame would silently skew the stats.
                            if at.elapsed() <= AUX_PAIR_WINDOW {
                                samples.push(Sample {
                                    phase,
                                    power_w: status.power_w,
                                    aux,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    report(&samples, options.csv.as_deref())
}

fn burn(stop: &AtomicBool) {
    let mut acc: u64 = 1;
    while !stop.load(Ordering::Relaxed) {
        acc = acc.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        std::hint::black_box(acc);
    }
}

fn report(samples: &[Sample], csv: Option<&str>) -> Result<()> {
    if samples.is_empty() {
        anyhow::bail!("no paired samples captured");
    }
    let power: Vec<f32> = samples.iter().map(|s| s.power_w).collect();
    println!("samples: {}", samples.len());
    for phase in [Phase::Idle, Phase::Load, Phase::Tail] {
        let values: Vec<f32> = samples
            .iter()
            .filter(|s| s.phase == phase)
            .map(|s| s.power_w)
            .collect();
        if !values.is_empty() {
            let mean = mean(&values);
            println!(
                "  {:5} n={:4} mean={mean:7.1} W",
                phase.label(),
                values.len()
            );
        }
    }

    let width = samples.iter().map(|s| s.aux.len()).min().unwrap_or(0);
    let mut ranked: Vec<(f64, usize)> = (0..width)
        .map(|index| {
            let xs: Vec<f32> = samples.iter().map(|s| f32::from(s.aux[index])).collect();
            (pearson(&xs, &power).abs(), index)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));

    println!("\nfield  corr(power)   idle_mean   load_mean");
    for (corr, index) in ranked.iter().take(6) {
        let idle = mean_of(samples, Phase::Idle, *index);
        let load = mean_of(samples, Phase::Load, *index);
        println!("f{index:02}   {corr:8.3}   {idle:9.1}   {load:9.1}");
    }

    if let Some(path) = csv {
        let mut file = std::fs::File::create(path).with_context(|| format!("creating {path}"))?;
        writeln!(file, "phase,power_w,aux")?;
        for sample in samples {
            let aux = sample
                .aux
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            writeln!(file, "{},{},{}", sample.phase.label(), sample.power_w, aux)?;
        }
        println!("\ncsv written to {path}");
    }
    Ok(())
}

fn mean_of(samples: &[Sample], phase: Phase, index: usize) -> f64 {
    let values: Vec<f32> = samples
        .iter()
        .filter(|s| s.phase == phase)
        .map(|s| f32::from(s.aux[index]))
        .collect();
    mean(&values)
}

fn mean(values: &[f32]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let sum: f64 = values.iter().map(|value| f64::from(*value)).sum();
    sum / count(values.len())
}

fn count(n: usize) -> f64 {
    f64::from(u32::try_from(n).expect("sample count fits in u32"))
}

fn pearson(xs: &[f32], ys: &[f32]) -> f64 {
    if xs.len() < 3 {
        return 0.0;
    }
    let n = count(xs.len());
    let mean_x = xs.iter().map(|v| f64::from(*v)).sum::<f64>() / n;
    let mean_y = ys.iter().map(|v| f64::from(*v)).sum::<f64>() / n;
    let mut covariance = 0.0;
    let mut variance_x = 0.0;
    let mut variance_y = 0.0;
    for (x, y) in xs.iter().zip(ys) {
        let deviation_x = f64::from(*x) - mean_x;
        let deviation_y = f64::from(*y) - mean_y;
        covariance += deviation_x * deviation_y;
        variance_x += deviation_x * deviation_x;
        variance_y += deviation_y * deviation_y;
    }
    if variance_x == 0.0 || variance_y == 0.0 {
        0.0
    } else {
        covariance / (variance_x.sqrt() * variance_y.sqrt())
    }
}
