//! Rolling telemetry history, CSV archive and trend statistics.
//!
//! The operator panel keeps up to two hours of samples in memory (fixed ring
//! buffer, one sample per second) and appends every sample to a rotating CSV
//! file. The archive survives restarts; the chart starts empty each run.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use himos_core::dm850g::PsuStatus;

/// One recorded telemetry sample.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sample {
    /// Seconds since the application started.
    pub elapsed_s: f64,
    /// Unix time in seconds.
    pub unix_s: f64,
    /// Total output power in watts.
    pub power_w: f32,
    /// +12 V rail in volts.
    pub voltage_12v: f32,
    /// +5 V rail in volts.
    pub voltage_5v: f32,
    /// +3.3 V rail in volts.
    pub voltage_3v3: f32,
    /// +5 VSB rail in volts.
    pub voltage_5vsb: f32,
    /// High-side temperature in °C.
    pub temp_high_c: f32,
    /// Low-side temperature in °C.
    pub temp_low_c: f32,
    /// Ambient temperature in °C.
    pub temp_ambient_c: f32,
    /// AC input voltage in volts.
    pub input_voltage_v: f32,
    /// AC input frequency in hertz.
    pub input_frequency_hz: f32,
}

impl Sample {
    /// Turns one fresh status frame into a sample.
    pub(crate) fn from_status(status: &PsuStatus, elapsed_s: f64, unix_s: f64) -> Self {
        Self {
            elapsed_s,
            unix_s,
            power_w: status.power_w,
            voltage_12v: status.voltage_12v,
            voltage_5v: status.voltage_5v,
            voltage_3v3: status.voltage_3v3,
            voltage_5vsb: status.voltage_5vsb,
            temp_high_c: status.temp_high_c,
            temp_low_c: status.temp_low_c,
            temp_ambient_c: status.temp_ambient_c,
            input_voltage_v: status.input_voltage_v,
            input_frequency_hz: status.input_frequency_hz,
        }
    }
}

/// In-memory ring buffer size (two hours at one sample per second).
pub(crate) const MAX_SAMPLES: usize = 7_200;
/// Minimum spacing between recorded samples.
pub(crate) const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
/// The archive is rotated once it grows past this size.
const MAX_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024;

/// Which measurement the trend chart shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Metric {
    /// Total output power.
    Power,
    /// +12 V rail voltage.
    Rail12,
    /// High-side temperature.
    TempHigh,
    /// AC input voltage.
    InputVoltage,
}

impl Metric {
    /// Every metric, in selector order.
    pub(crate) const ALL: [Self; 4] = [
        Self::Power,
        Self::Rail12,
        Self::TempHigh,
        Self::InputVoltage,
    ];

    /// Short uppercase label.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Power => "POWER",
            Self::Rail12 => "+12V",
            Self::TempHigh => "TEMP",
            Self::InputVoltage => "AC",
        }
    }

    /// Unit shown next to the value.
    pub(crate) fn unit(self) -> &'static str {
        match self {
            Self::Power => "W",
            Self::Rail12 | Self::InputVoltage => "V",
            Self::TempHigh => "°C",
        }
    }

    /// Digits after the decimal point.
    pub(crate) fn decimals(self) -> usize {
        match self {
            Self::Power | Self::TempHigh => 1,
            Self::Rail12 | Self::InputVoltage => 2,
        }
    }

    /// Reads this metric from a sample.
    pub(crate) fn value(self, sample: &Sample) -> f32 {
        match self {
            Self::Power => sample.power_w,
            Self::Rail12 => sample.voltage_12v,
            Self::TempHigh => sample.temp_high_c,
            Self::InputVoltage => sample.input_voltage_v,
        }
    }
}

/// Time span shown by the chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryWindow {
    /// Last minute.
    OneMinute,
    /// Last five minutes.
    FiveMinutes,
    /// Last fifteen minutes.
    FifteenMinutes,
    /// Last hour.
    OneHour,
    /// Everything in the ring buffer.
    All,
}

impl HistoryWindow {
    /// Every span, in selector order.
    pub(crate) const ALL: [Self; 5] = [
        Self::OneMinute,
        Self::FiveMinutes,
        Self::FifteenMinutes,
        Self::OneHour,
        Self::All,
    ];

    /// Short label.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::OneMinute => "1 M",
            Self::FiveMinutes => "5 M",
            Self::FifteenMinutes => "15 M",
            Self::OneHour => "1 H",
            Self::All => "ALL",
        }
    }

    /// Parses `1m`/`5m`/`15m`/`1h`/`all`.
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "1m" | "1" => Some(Self::OneMinute),
            "5m" | "5" => Some(Self::FiveMinutes),
            "15m" | "15" => Some(Self::FifteenMinutes),
            "1h" | "60" => Some(Self::OneHour),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    /// Fixed width of the time axis for this span; `All` uses the buffer size.
    pub(crate) fn span_seconds(self) -> f64 {
        self.seconds()
            .unwrap_or_else(|| f64::from(u32::try_from(MAX_SAMPLES).unwrap_or(u32::MAX)))
    }

    /// Window length in seconds, or `None` for everything.
    pub(crate) fn seconds(self) -> Option<f64> {
        match self {
            Self::OneMinute => Some(60.0),
            Self::FiveMinutes => Some(300.0),
            Self::FifteenMinutes => Some(900.0),
            Self::OneHour => Some(3_600.0),
            Self::All => None,
        }
    }
}

/// Aggregates for one metric over the selected window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stats {
    /// Number of samples in the window.
    pub count: usize,
    /// Smallest value.
    pub min: f32,
    /// Largest value.
    pub max: f32,
    /// Arithmetic mean.
    pub avg: f32,
    /// Most recent value.
    pub last: f32,
}

/// Ring buffer of telemetry samples.
#[derive(Debug, Default)]
pub(crate) struct History {
    samples: VecDeque<Sample>,
    last_sampled: Option<Instant>,
}

impl History {
    /// Whether enough time has passed to record another sample.
    pub(crate) fn should_sample(&self, now: Instant) -> bool {
        self.last_sampled
            .is_none_or(|last| now.duration_since(last) >= SAMPLE_INTERVAL)
    }

    /// Marks the sampling clock.
    pub(crate) fn mark_sampled(&mut self, now: Instant) {
        self.last_sampled = Some(now);
    }

    /// Appends a sample, dropping the oldest when full.
    pub(crate) fn push(&mut self, sample: Sample) {
        if self.samples.len() == MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Removes every sample.
    pub(crate) fn clear(&mut self) {
        self.samples.clear();
    }

    /// Number of buffered samples.
    pub(crate) fn len(&self) -> usize {
        self.samples.len()
    }

    /// All buffered samples, oldest first.
    pub(crate) fn samples(&self) -> impl Iterator<Item = &Sample> {
        self.samples.iter()
    }

    /// Samples inside the window ending at `now_s`.
    pub(crate) fn window(
        &self,
        window: HistoryWindow,
        now_s: f64,
    ) -> impl Iterator<Item = &Sample> {
        let start = window.seconds().map_or(f64::NEG_INFINITY, |s| now_s - s);
        self.samples
            .iter()
            .filter(move |sample| sample.elapsed_s >= start)
    }

    /// Aggregates one metric over the window.
    pub(crate) fn stats(&self, window: HistoryWindow, now_s: f64, metric: Metric) -> Option<Stats> {
        let mut count = 0usize;
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        let mut sum = 0.0_f32;
        let mut last = 0.0_f32;
        for sample in self.window(window, now_s) {
            let value = metric.value(sample);
            count += 1;
            min = min.min(value);
            max = max.max(value);
            sum += value;
            last = value;
        }
        if count == 0 {
            return None;
        }
        let count_f = f32::from(u16::try_from(count).unwrap_or(u16::MAX));
        Some(Stats {
            count,
            min,
            max,
            avg: sum / count_f,
            last,
        })
    }
}

/// CSV column names.
const CSV_HEADER: &str =
    "unix_s,elapsed_s,power_w,v12,v5,v3v3,v5vsb,temp_high_c,temp_low_c,temp_ambient_c,ac_v,ac_hz";

/// Formats one sample as a CSV line (without the trailing newline).
pub(crate) fn csv_line(sample: &Sample) -> String {
    format!(
        "{:.3},{:.3},{:.2},{:.3},{:.3},{:.3},{:.3},{:.2},{:.2},{:.2},{:.2},{:.2}",
        sample.unix_s,
        sample.elapsed_s,
        sample.power_w,
        sample.voltage_12v,
        sample.voltage_5v,
        sample.voltage_3v3,
        sample.voltage_5vsb,
        sample.temp_high_c,
        sample.temp_low_c,
        sample.temp_ambient_c,
        sample.input_voltage_v,
        sample.input_frequency_hz,
    )
}

/// Appends samples to the rotating CSV archive.
#[derive(Debug)]
pub(crate) struct Recorder {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
}

impl Recorder {
    /// Opens (or creates) the archive and writes the header when needed.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        rotate_if_large(path)?;
        let needs_header = std::fs::metadata(path).map_or(true, |meta| meta.len() == 0);
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let mut writer = BufWriter::new(file);
        if needs_header {
            writeln!(writer, "{CSV_HEADER}")?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            writer: Some(writer),
        })
    }

    /// Appends one sample.
    pub(crate) fn append(&mut self, sample: &Sample) -> io::Result<()> {
        if let Some(writer) = &mut self.writer {
            writer.flush()?;
            if writer.get_ref().metadata()?.len() > MAX_ARCHIVE_BYTES {
                // Close before renaming so rotation also works on Windows.
                self.writer = None;
            }
        }
        if self.writer.is_none() {
            *self = Self::open(&self.path)?;
        }
        let writer = self
            .writer
            .as_mut()
            .expect("archive reopened before appending");
        writeln!(writer, "{}", csv_line(sample))?;
        writer.flush()
    }

    /// Where the archive lives.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

/// Default archive location next to the appearance settings.
pub(crate) fn default_path() -> Option<PathBuf> {
    crate::sys::config_path("history.csv")
}

/// Default export location, one file per export.
pub(crate) fn export_path() -> Option<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    crate::sys::config_path(&format!("history-export-{stamp}.csv"))
}

/// Writes every buffered sample to `path`; returns the row count.
pub(crate) fn export(samples: &History, path: &Path) -> io::Result<usize> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "{CSV_HEADER}")?;
    let mut count = 0;
    for sample in samples.samples() {
        writeln!(writer, "{}", csv_line(sample))?;
        count += 1;
    }
    writer.flush()?;
    Ok(count)
}

/// Renames an oversized archive so a single file never grows without bound.
fn rotate_if_large(path: &Path) -> io::Result<()> {
    let Ok(meta) = std::fs::metadata(path) else {
        return Ok(());
    };
    if meta.len() <= MAX_ARCHIVE_BYTES {
        return Ok(());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let stem = path.file_stem().map_or_else(
        || "history".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let rotated = path.with_file_name(format!("{stem}-{stamp}.csv"));
    std::fs::rename(path, rotated)
}

/// Current Unix time in seconds.
pub(crate) fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(elapsed_s: f64, power: f32, rail: f32) -> Sample {
        Sample {
            elapsed_s,
            unix_s: 1_700_000_000.0 + elapsed_s,
            power_w: power,
            voltage_12v: rail,
            voltage_5v: 5.0,
            voltage_3v3: 3.3,
            voltage_5vsb: 5.0,
            temp_high_c: 50.0,
            temp_low_c: 60.0,
            temp_ambient_c: 30.0,
            input_voltage_v: 230.0,
            input_frequency_hz: 50.0,
        }
    }

    #[test]
    fn ring_buffer_keeps_the_newest_samples() {
        let mut history = History::default();
        for index in 0..(MAX_SAMPLES + 10) {
            let value = f32::from(u16::try_from(index).expect("sample index fits u16"));
            history.push(sample(f64::from(value), value, 12.0));
        }
        assert_eq!(history.len(), MAX_SAMPLES);
        assert_eq!(history.samples().next().expect("not empty").elapsed_s, 10.0);
    }

    #[test]
    fn stats_use_only_the_window() {
        let mut history = History::default();
        history.push(sample(0.0, 100.0, 12.0));
        history.push(sample(10.0, 200.0, 12.1));
        history.push(sample(20.0, 300.0, 12.2));
        let stats = history
            .stats(HistoryWindow::OneMinute, 20.0, Metric::Power)
            .expect("samples in window");
        assert_eq!(stats.count, 3);
        assert!((stats.min - 100.0).abs() < f32::EPSILON);
        assert!((stats.max - 300.0).abs() < f32::EPSILON);
        assert!((stats.avg - 200.0).abs() < 0.01);
        assert!((stats.last - 300.0).abs() < f32::EPSILON);
        assert!(
            history
                .stats(HistoryWindow::OneMinute, 120.0, Metric::Power)
                .is_none()
        );
    }

    #[test]
    fn span_keys_parse() {
        assert_eq!(HistoryWindow::from_key("1h"), Some(HistoryWindow::OneHour));
        assert_eq!(HistoryWindow::from_key("ALL"), Some(HistoryWindow::All));
        assert_eq!(HistoryWindow::from_key("2h"), None);
    }

    #[test]
    fn span_seconds_are_fixed() {
        assert!((HistoryWindow::OneMinute.span_seconds() - 60.0).abs() < f64::EPSILON);
        assert!((HistoryWindow::OneHour.span_seconds() - 3_600.0).abs() < f64::EPSILON);
        assert!((HistoryWindow::All.span_seconds() - 7_200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn metric_reads_the_right_field() {
        let sample = sample(1.0, 151.3, 12.089);
        assert!((Metric::Power.value(&sample) - 151.3).abs() < f32::EPSILON);
        assert!((Metric::Rail12.value(&sample) - 12.089).abs() < f32::EPSILON);
        assert_eq!(Metric::TempHigh.unit(), "°C");
        assert_eq!(Metric::InputVoltage.decimals(), 2);
    }

    #[test]
    fn csv_line_has_every_column() {
        let line = csv_line(&sample(5.0, 151.3, 12.089));
        let columns = line.split(',').count();
        assert_eq!(columns, CSV_HEADER.split(',').count());
        assert!(line.starts_with("1700000005.000,5.000,151.30,12.089"));
    }

    #[test]
    fn recorder_rotates_without_restarting() {
        let dir = std::env::temp_dir().join(format!("himos-rotation-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test directory");
        let path = dir.join("history.csv");
        let mut recorder = Recorder::open(&path).expect("open archive");
        recorder
            .append(&sample(0.0, 100.0, 12.0))
            .expect("first sample");
        OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open sparse archive")
            .set_len(MAX_ARCHIVE_BYTES + 1)
            .expect("grow archive");
        recorder
            .append(&sample(1.0, 110.0, 12.1))
            .expect("rotate and append");
        assert!(std::fs::metadata(&path).expect("archive metadata").len() < MAX_ARCHIVE_BYTES);
        let text = std::fs::read_to_string(&path).expect("new archive");
        assert_eq!(text.lines().count(), 2, "header and new sample");
        assert!(text.contains("110.00"));
        assert_eq!(std::fs::read_dir(&dir).expect("list archives").count(), 2);
        drop(recorder);
        std::fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn recorder_appends_and_exports() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        // The system temp directory is not writable in every sandbox; the
        // workspace is, and the test cleans up after itself.
        let dir = std::env::current_dir()
            .expect("test working directory")
            .join("target")
            .join("history-tests");
        std::fs::create_dir_all(&dir).expect("test directory is writable");
        let unique = format!("himos-history-test-{}-{stamp}.csv", std::process::id());
        let path = dir.join(unique);
        let mut recorder = Recorder::open(&path).expect("archive opens");
        recorder.append(&sample(0.0, 100.0, 12.0)).expect("append");
        recorder.append(&sample(1.0, 110.0, 12.1)).expect("append");
        let text = std::fs::read_to_string(&path).expect("archive readable");
        assert_eq!(text.lines().count(), 3, "header plus two rows");

        let mut history = History::default();
        history.push(sample(0.0, 100.0, 12.0));
        let export_file = dir.join(format!("himos-history-export-{}.csv", std::process::id()));
        assert_eq!(export(&history, &export_file).expect("export"), 1);
        let text = std::fs::read_to_string(&export_file).expect("export readable");
        assert_eq!(text.lines().count(), 2);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&export_file);
    }
}
