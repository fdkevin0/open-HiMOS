//! Desktop operator panel for the DM-850G.
//!
//! Four pages (overview, details, advanced, history) on top of one serial
//! worker thread; the port is detected asynchronously and re-scanned while
//! missing, so neither enumeration nor I/O ever blocks a frame.
//!
//! Command line: `--smoke <seconds>` prints a summary and exits, `--screenshot
//! <png>` renders one frame and exits, `--level overview|details|advanced|history`
//! selects the initial page, `--port <name>` overrides autodetection, and
//! `--help` prints usage. `--smoke` exits with status 1 when no frame arrived.

mod health;
mod history;
mod panels;
mod sys;
mod theme;
mod widgets;
mod window;
mod worker;

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui;
use himos_core::Frame;
use himos_core::dm850g::{
    AuxTelemetry, CMD_AUX, CMD_INFO, CMD_SERIAL, CMD_STATUS, CurvePoint, DeviceInfo, FanMode,
    PsuStatus, RefreshInterval, SerialNumber,
};
use himos_core::transport::SerialLink;

use history::{History, HistoryWindow, Metric};
use theme::{Accent, Appearance, ThemeMode, palette};
use window::WindowState;
use worker::{FromWorker, ToWorker};

/// How often the UI looks for an unplugged PSU.
const SCAN_INTERVAL: Duration = Duration::from_secs(2);
/// Telemetry older than this is shown as stale; longer than the slowest
/// reporting interval (3 s) so an idle link is not flagged by accident.
const STALE_AFTER: Duration = Duration::from_secs(5);

/// Which part of the PSU surface the window reveals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    /// Headline numbers.
    Overview,
    /// Every measured value.
    Details,
    /// Write commands and the event log.
    Advanced,
    /// Recorded telemetry and the trend chart.
    History,
}

/// One event-log line.
#[derive(Debug)]
struct LogEntry {
    /// Uptime stamp such as `T+00:12`.
    time: String,
    /// Message text.
    text: String,
}

/// Result of one asynchronous port scan.
#[derive(Debug)]
enum Detection {
    /// A PSU was found on this port.
    Found(String),
    /// Enumeration worked but no CH340 port is present.
    Missing(String),
    /// Enumeration itself failed.
    Failed(String),
}

/// Latest decoded device state, kept separate from the widgets so it can be
/// unit-tested without a window.
#[derive(Debug, Default)]
struct PsuState {
    status: Option<PsuStatus>,
    aux: Option<AuxTelemetry>,
    info: Option<DeviceInfo>,
    serial: Option<String>,
    last_status: Option<Instant>,
}

impl PsuState {
    /// Applies one frame; returns a log line for frames the UI does not render.
    fn apply(&mut self, frame: &Frame) -> Option<String> {
        match frame.cmd() {
            CMD_STATUS => {
                if let Ok(status) = PsuStatus::parse(frame.payload()) {
                    self.status = Some(status);
                    self.last_status = Some(Instant::now());
                }
                None
            }
            CMD_AUX => {
                if let Ok(aux) = AuxTelemetry::parse(frame.payload()) {
                    self.aux = Some(aux);
                }
                None
            }
            CMD_INFO => {
                if let Ok(info) = DeviceInfo::parse(frame.payload()) {
                    self.info = Some(info);
                }
                None
            }
            CMD_SERIAL => {
                self.serial = Some(SerialNumber::parse(frame.payload()).sn);
                None
            }
            other => Some(format!(
                "frame cmd {other:#04x} len {}",
                frame.payload().len()
            )),
        }
    }

    /// Whether the last status sample is still inside the freshness window.
    fn is_fresh(&self) -> bool {
        self.last_status
            .is_some_and(|at| at.elapsed() <= STALE_AFTER)
    }

    /// Fresh status, or `None` when telemetry is missing or stale.
    fn fresh_status(&self) -> Option<&PsuStatus> {
        self.is_fresh().then_some(self.status.as_ref()).flatten()
    }

    /// Fresh extended telemetry, or `None` when it is missing or stale.
    fn fresh_aux(&self) -> Option<&AuxTelemetry> {
        self.is_fresh().then_some(self.aux.as_ref()).flatten()
    }
}

/// Parsed command line.
#[derive(Debug, Default)]
struct Options {
    smoke_secs: Option<f64>,
    port: Option<String>,
    screenshot: Option<String>,
    level: Option<Level>,
    theme: Option<ThemeMode>,
    accent: Option<Accent>,
    settle_secs: f64,
    history_span: Option<HistoryWindow>,
    data_dir: Option<String>,
    help: bool,
}

impl Options {
    fn validate_durations(&self) -> Result<(), std::io::Error> {
        for (name, seconds) in [
            ("--settle", Some(self.settle_secs)),
            ("--smoke", self.smoke_secs),
        ] {
            if let Some(seconds) = seconds {
                Duration::try_from_secs_f64(seconds).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("invalid {name} duration: {err}"),
                    )
                })?;
            }
        }
        Ok(())
    }
}

/// GUI application state.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent UI flags (link, autoconnect, probe, screenshot) are not modes"
)]
struct App {
    level: Level,
    port: Option<String>,
    port_override: Option<String>,
    tx: Option<Sender<ToWorker>>,
    rx: Option<Receiver<FromWorker>>,
    worker: Option<JoinHandle<()>>,
    retired: Vec<JoinHandle<()>>,
    detect_rx: Option<Receiver<Detection>>,
    connected: bool,
    appearance: Appearance,
    os_theme: Option<egui::Theme>,
    os_accent: Option<egui::Color32>,
    accent_rx: Option<Receiver<Option<egui::Color32>>>,
    accent_attempted: bool,
    theme_applied: Option<(egui::Theme, egui::Color32)>,
    window: WindowState,
    fullscreen: bool,
    geometry_restored: bool,
    settle_secs: f64,
    archive_enabled: bool,
    history: History,
    recorder: Option<history::Recorder>,
    recorder_error: Option<String>,
    history_paused: bool,
    history_metric: Metric,
    history_window: HistoryWindow,
    history_note: Option<String>,
    title: String,
    state: PsuState,
    fan_mode: Option<FanMode>,
    interval: Option<RefreshInterval>,
    curve: [CurvePoint; 3],
    log: VecDeque<LogEntry>,
    error: Option<String>,
    smoke_secs: Option<f64>,
    screenshot: Option<String>,
    shot_requested: bool,
    started: Instant,
    frames_seen: u64,
    uptime: String,
    uptime_secs: u64,
    /// Whether the UI should keep looking for the PSU and connect on its own.
    auto_connect: bool,
    /// Next autodetection attempt.
    next_scan: Instant,
}

impl App {
    fn new(options: Options, appearance: Appearance, window: WindowState) -> Self {
        let archive_enabled = options.screenshot.is_none() && options.smoke_secs.is_none();
        let history_window = options.history_span.unwrap_or(HistoryWindow::OneHour);
        Self {
            level: options.level.unwrap_or(Level::Overview),
            port_override: options.port.clone(),
            port: options.port,
            tx: None,
            rx: None,
            worker: None,
            retired: Vec::new(),
            detect_rx: None,
            connected: false,
            appearance,
            os_theme: None,
            os_accent: None,
            accent_rx: None,
            accent_attempted: false,
            theme_applied: None,
            window,
            fullscreen: false,
            geometry_restored: false,
            settle_secs: options.settle_secs,
            archive_enabled,
            history: History::default(),
            recorder: None,
            recorder_error: None,
            history_paused: false,
            history_metric: Metric::Power,
            history_window,
            history_note: None,
            title: String::new(),
            state: PsuState::default(),
            fan_mode: None,
            interval: None,
            curve: [
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
            ],
            log: VecDeque::new(),
            error: None,
            smoke_secs: options.smoke_secs,
            screenshot: options.screenshot,
            shot_requested: false,
            started: Instant::now(),
            frames_seen: 0,
            uptime: stamp(Duration::ZERO),
            uptime_secs: 0,
            auto_connect: true,
            next_scan: Instant::now(),
        }
    }

    /// Re-resolves and applies the palette when the OS or the user changed it.
    fn refresh_theme(&mut self, ctx: &egui::Context) {
        let os_theme = ctx.input(|input| input.raw.system_theme);
        if os_theme != self.os_theme {
            self.os_theme = os_theme;
            self.theme_applied = None;
        }
        if self.appearance.accent == Accent::Auto && !self.accent_attempted {
            self.start_accent_probe();
        }
        let (palette, theme) = theme::resolve(self.appearance, self.os_theme, self.os_accent);
        if self.theme_applied != Some((theme, palette.accent)) {
            theme::apply(ctx, palette, theme);
            self.theme_applied = Some((theme, palette.accent));
            ctx.request_repaint();
        }
    }

    /// Records one history sample per second from fresh telemetry.
    fn sample_history(&mut self) {
        if self.history_paused {
            return;
        }
        let now = Instant::now();
        if !self.history.should_sample(now) {
            return;
        }
        let Some(status) = self.state.fresh_status().copied() else {
            return;
        };
        let elapsed_s = self.started.elapsed().as_secs_f64();
        let sample = history::Sample::from_status(&status, elapsed_s, history::unix_now());
        self.history.push(sample);
        self.history.mark_sampled(now);
        self.append_archive(&sample);
    }

    /// Appends one sample to the rotating CSV archive (opened lazily).
    fn append_archive(&mut self, sample: &history::Sample) {
        if !self.archive_enabled {
            return;
        }
        if self.recorder.is_none() && self.recorder_error.is_none() {
            match history::default_path() {
                Some(path) => match history::Recorder::open(&path) {
                    Ok(recorder) => {
                        self.push_log(format!("history archive: {}", path.display()));
                        self.recorder = Some(recorder);
                    }
                    Err(err) => self.recorder_error = Some(format!("history archive: {err}")),
                },
                None => {
                    self.recorder_error =
                        Some("no writable data directory for the history archive".to_owned());
                }
            }
        }
        let Some(recorder) = &mut self.recorder else {
            return;
        };
        if let Err(err) = recorder.append(sample) {
            self.recorder_error = Some(format!("history archive: {err}"));
            self.recorder = None;
        }
    }

    /// Writes the in-memory buffer to a timestamped CSV export.
    fn export_history(&mut self) {
        let Some(path) = history::export_path() else {
            self.history_note = Some("no config directory for the export".to_owned());
            return;
        };
        match history::export(&self.history, &path) {
            Ok(count) => {
                self.history_note = Some(format!("exported {count} samples to {}", path.display()));
            }
            Err(err) => self.history_note = Some(format!("export failed: {err}")),
        }
    }

    /// Keeps the native title bar in sync with the device/link state.
    fn sync_title(&mut self, ctx: &egui::Context) {
        let title = if self.connected {
            let port = self.port.as_deref().unwrap_or("—");
            format!("open-HiMOS · DM-850G · {port}")
        } else if self.state.status.is_some() {
            "open-HiMOS · DM-850G · link lost".to_owned()
        } else {
            "open-HiMOS · DM-850G · searching".to_owned()
        };
        if self.title != title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Keeps the native window state in sync and restores the saved geometry.
    fn handle_window(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.key_pressed(egui::Key::F11)) {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if self.fullscreen && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }

        let (monitor, fullscreen) = ctx.input(|input| {
            let viewport = input.viewport();
            (viewport.monitor_size, viewport.fullscreen)
        });
        if let Some(fullscreen) = fullscreen {
            self.fullscreen = fullscreen;
        }

        // Once the native window is up, pull an off-screen rectangle back and
        // apply the restored size through the windowing API.
        if let Some(monitor) = monitor
            && !self.geometry_restored
        {
            self.geometry_restored = true;
            if !self.window.maximized {
                let clamped = self.window.clamped_to(monitor);
                if clamped != self.window {
                    if let Some(position) = clamped.position {
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        clamped.width,
                        clamped.height,
                    )));
                }
                self.window = clamped;
            }
        }

        if self.geometry_restored && !self.fullscreen {
            let info = ctx.input(|input| input.viewport().clone());
            self.window.capture(&info);
        }
    }

    /// Starts an asynchronous read of the OS accent colour.
    fn start_accent_probe(&mut self) {
        if self.accent_rx.is_some() {
            return;
        }
        self.accent_attempted = true;
        let (sender, receiver) = mpsc::channel();
        self.accent_rx = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(sys::system_accent());
        });
    }

    /// Picks up a finished accent probe.
    fn poll_accent(&mut self) {
        let Some(receiver) = &self.accent_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(color) => {
                self.accent_rx = None;
                if color != self.os_accent {
                    self.os_accent = color;
                    self.theme_applied = None;
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.accent_rx = None,
        }
    }

    /// Applies a user choice and persists it.
    fn set_appearance(&mut self, appearance: Appearance) {
        if appearance == self.appearance {
            return;
        }
        self.appearance = appearance;
        sys::save_config("appearance.conf", &appearance.to_config());
        self.accent_attempted = false;
        self.theme_applied = None;
    }

    /// Starts an asynchronous port scan (no-op while one is in flight).
    fn start_detect(&mut self) {
        if self.detect_rx.is_some() || self.tx.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.detect_rx = Some(receiver);
        let port_override = self.port_override.clone();
        std::thread::spawn(move || {
            if let Some(port) = port_override {
                let _ = sender.send(Detection::Found(port));
                return;
            }
            let outcome = match SerialLink::detect() {
                Ok(Some(info)) => Detection::Found(info.port_name),
                Ok(None) => {
                    Detection::Missing("No PSU detected; plug it in or press Rescan".to_owned())
                }
                Err(err) => Detection::Failed(format!("serial port enumeration failed: {err}")),
            };
            let _ = sender.send(outcome);
        });
    }

    /// Applies a finished scan.
    fn poll_detection(&mut self) {
        let Some(receiver) = &self.detect_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(outcome) => {
                self.detect_rx = None;
                self.apply_detection(outcome);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.detect_rx = None,
        }
    }

    fn apply_detection(&mut self, outcome: Detection) {
        match outcome {
            Detection::Found(port) => {
                self.port = Some(port);
                self.error = None;
                if self.tx.is_none() {
                    self.connect();
                }
            }
            Detection::Missing(message) => {
                self.port = None;
                self.connected = false;
                self.error = Some(message);
            }
            Detection::Failed(message) => self.error = Some(message),
        }
    }

    /// Requests a fresh scan, e.g. from the Rescan button.
    fn request_rescan(&mut self) {
        self.auto_connect = true;
        self.next_scan = Instant::now();
        self.start_detect();
    }

    fn connect(&mut self) {
        let Some(port) = self.port.clone() else {
            self.error = Some("No PSU port detected".to_owned());
            return;
        };
        self.disconnect();
        let (to_worker, from_ui) = mpsc::channel();
        let (to_ui, from_worker) = mpsc::channel();

        let handle = std::thread::spawn(move || {
            if let Err(err) = worker::run_worker(&port, &from_ui, &to_ui) {
                let _ = to_ui.send(FromWorker::Error(err));
            }
            let _ = to_ui.send(FromWorker::Closed);
        });

        self.tx = Some(to_worker);
        self.rx = Some(from_worker);
        self.worker = Some(handle);
        self.connected = false;
        self.error = None;
        self.log.clear();
    }

    /// Asks the worker to stop without waiting for it on the UI thread.
    fn disconnect(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(ToWorker::Shutdown);
        }
        if let Some(handle) = self.worker.take() {
            self.retired.push(handle);
        }
        self.rx = None;
        self.connected = false;
    }

    /// Joins workers that have already finished; never blocks on a live one.
    fn reap_workers(&mut self) {
        let mut still_running = Vec::new();
        for handle in self.retired.drain(..) {
            if handle.is_finished() {
                let _ = handle.join();
            } else {
                still_running.push(handle);
            }
        }
        self.retired = still_running;
    }

    /// Stops the worker and waits for it; only used while shutting down.
    fn shutdown_blocking(&mut self) {
        self.disconnect();
        for handle in self.retired.drain(..) {
            let _ = handle.join();
        }
    }

    /// Sends a frame; returns `false` and reports why when no link is usable.
    fn send(&mut self, frame: Frame, label: &str) -> bool {
        if !self.connected {
            self.error = Some("link offline; command not sent".to_owned());
            return false;
        }
        let Some(tx) = self.tx.clone() else {
            self.error = Some("worker stopped; command not sent".to_owned());
            return false;
        };
        if tx.send(ToWorker::Send(frame)).is_ok() {
            self.push_log(format!("sent {label}"));
            true
        } else {
            self.error = Some("worker stopped; command not sent".to_owned());
            false
        }
    }

    fn push_log(&mut self, line: String) {
        let entry = LogEntry {
            time: self.uptime.clone(),
            text: line,
        };
        self.log.push_back(entry);
        while self.log.len() > 200 {
            self.log.pop_front();
        }
    }

    fn refresh_uptime(&mut self) {
        let secs = self.started.elapsed().as_secs();
        if secs != self.uptime_secs {
            self.uptime_secs = secs;
            self.uptime = stamp(Duration::from_secs(secs));
        }
    }

    fn drain_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                FromWorker::Frame(frame) => {
                    self.frames_seen += 1;
                    if let Some(line) = self.state.apply(&frame) {
                        self.push_log(line);
                    }
                }
                FromWorker::Connected(port) => {
                    self.connected = true;
                    self.push_log(format!("connected to {port}"));
                }
                FromWorker::Error(err) => {
                    self.connected = false;
                    self.push_log(format!("error: {err}"));
                    self.error = Some(err);
                }
                FromWorker::Closed => {
                    self.connected = false;
                    self.tx = None;
                    if let Some(handle) = self.worker.take() {
                        self.retired.push(handle);
                    }
                    self.push_log("link closed; searching for the PSU".to_owned());
                }
            }
        }
    }

    /// Requests one frame, saves it as PNG, then exits for documentation capture.
    fn maybe_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        let settled = self.started.elapsed().as_secs_f64() >= self.settle_secs;
        if !self.shot_requested
            && settled
            && (self.frames_seen >= 5 || self.started.elapsed() > Duration::from_secs(8))
        {
            ctx.request_screenshot(move |frame_image| match save_png(&path, &frame_image) {
                Ok(()) => {
                    println!("screenshot saved to {path}");
                    std::process::exit(0);
                }
                Err(err) => {
                    eprintln!("screenshot failed: {err}");
                    std::process::exit(1);
                }
            });
            self.shot_requested = true;
        }
        if !self.shot_requested {
            return;
        }
        if self.started.elapsed().as_secs_f64() > 20.0 + self.settle_secs {
            eprintln!("screenshot timed out");
            std::process::exit(1);
        }
    }

    /// Brand bar, status strip, tabs, and footer.
    fn draw_chrome(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("masthead")
            .exact_size(48.0)
            .frame(
                egui::Frame::NONE
                    .fill(palette().bar)
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    self.masthead(ui);
                });
            });

        egui::Panel::top("status")
            .exact_size(38.0)
            .frame(
                egui::Frame::NONE
                    .fill(palette().bg)
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    self.status_strip(ui);
                });
            });

        egui::Panel::top("tabs")
            .exact_size(42.0)
            .frame(
                egui::Frame::NONE
                    .fill(palette().bg)
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    self.tabs(ui);
                });
            });

        egui::Panel::bottom("footer")
            .exact_size(26.0)
            .frame(
                egui::Frame::NONE
                    .fill(palette().bar)
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    self.footer(ui);
                });
            });
    }

    /// Central panel with the selected disclosure level.
    fn draw_content(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(palette().bg)
                    .inner_margin(egui::Margin::symmetric(16, 14)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.level {
                        Level::Overview => self.overview(ui),
                        Level::Details => self.details(ui),
                        Level::Advanced => self.advanced(ui),
                        Level::History => self.history_panel(ui),
                    });
            });
    }

    fn smoke_finished(&self) -> bool {
        self.smoke_secs
            .is_some_and(|seconds| self.started.elapsed().as_secs_f64() >= seconds)
    }
}

/// Saves a captured frame as PNG.
fn save_png(path: &str, frame_image: &egui::ColorImage) -> Result<(), String> {
    let width = u32::try_from(frame_image.size[0]).map_err(|_| "image too wide".to_owned())?;
    let height = u32::try_from(frame_image.size[1]).map_err(|_| "image too tall".to_owned())?;
    let mut rgba = Vec::with_capacity(frame_image.pixels.len() * 4);
    for pixel in &frame_image.pixels {
        let [red, green, blue, alpha] = pixel.to_array();
        rgba.extend_from_slice(&[red, green, blue, alpha]);
    }
    image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8)
        .map_err(|err| err.to_string())
}

/// `T+MM:SS` uptime stamp.
fn stamp(elapsed: Duration) -> String {
    let total = elapsed.as_secs();
    format!("T+{:02}:{:02}", total / 60, total % 60)
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_uptime();
        self.reap_workers();
        self.poll_detection();
        self.poll_accent();
        self.refresh_theme(ctx);
        // Autodetection loop: look for the PSU, connect, retry after a drop.
        if self.auto_connect
            && self.tx.is_none()
            && self.detect_rx.is_none()
            && Instant::now() >= self.next_scan
        {
            self.next_scan = Instant::now() + SCAN_INTERVAL;
            self.start_detect();
        }

        if self.smoke_secs.is_some() && self.smoke_finished() && self.screenshot.is_none() {
            let power = self.state.status.map_or(f32::NAN, |status| status.power_w);
            let model = self
                .state
                .info
                .as_ref()
                .map_or_else(String::new, |info| info.model.clone());
            let port = self.port.clone().unwrap_or_else(|| "none".to_owned());
            println!(
                "gui smoke: frames={} connected={} port={port} power={power:.1} model={model:?}",
                self.frames_seen, self.connected
            );
            std::process::exit(i32::from(self.frames_seen == 0));
        }

        self.drain_events();
        self.sample_history();
        self.sync_title(ctx);
        self.maybe_screenshot(ctx);
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx();
        self.handle_window(ctx);

        let requested = ctx.input(|input| {
            if input.key_pressed(egui::Key::Num1) {
                Some(Level::Overview)
            } else if input.key_pressed(egui::Key::Num2) {
                Some(Level::Details)
            } else if input.key_pressed(egui::Key::Num3) {
                Some(Level::Advanced)
            } else if input.key_pressed(egui::Key::Num4) {
                Some(Level::History)
            } else {
                None
            }
        });
        if let Some(level) = requested {
            self.level = level;
        }

        self.draw_chrome(ui);
        self.draw_content(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        sys::save_config("window.conf", &self.window.to_config());
        self.shutdown_blocking();
    }
}

/// Parses the process arguments.
fn parse_args() -> Options {
    parse_args_from(std::env::args().skip(1))
}

/// Parses `--smoke`, `--screenshot`, `--level`, `--port` and `--help`.
fn parse_args_from<I: IntoIterator<Item = String>>(args: I) -> Options {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => {
                options.smoke_secs = Some(
                    args.next()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(f64::NAN),
                );
            }
            "--port" => options.port = args.next(),
            "--screenshot" => options.screenshot = args.next(),
            "--level" => options.level = args.next().and_then(|value| parse_level(&value)),
            "--theme" => options.theme = args.next().and_then(|value| parse_theme_mode(&value)),
            "--accent" => options.accent = args.next().and_then(|value| Accent::from_key(&value)),
            "--settle" => {
                options.settle_secs = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(f64::NAN);
            }
            "--history-span" => {
                options.history_span = args
                    .next()
                    .and_then(|value| HistoryWindow::from_key(&value));
            }
            "--data-dir" => options.data_dir = args.next(),
            "--help" | "-h" => options.help = true,
            _ => {}
        }
    }
    options
}

/// Maps `system`/`dark`/`light` to a theme mode.
fn parse_theme_mode(value: &str) -> Option<ThemeMode> {
    ThemeMode::from_key(value)
}

/// Maps `overview`/`details`/`advanced` to a disclosure level.
fn parse_level(value: &str) -> Option<Level> {
    match value.to_ascii_lowercase().as_str() {
        "overview" => Some(Level::Overview),
        "details" => Some(Level::Details),
        "advanced" => Some(Level::Advanced),
        "history" => Some(Level::History),
        _ => None,
    }
}

fn print_usage() {
    println!(
        "open-HiMOS desktop panel\n\n\
         Usage: himos-gui [OPTIONS]\n\n\
         Options:\n\
         \x20 --port <NAME>        serial port override (default: autodetect CH340)\n\
         \x20 --level <PAGE>       overview | details | advanced | history\n\
         \x20 --theme <MODE>       system | dark | light (default: follow OS)\n\
         \x20 --accent <NAME>      auto | cyan | blue | green | amber | violet | rose\n\
         \x20 --smoke <SECONDS>    connect, print frame stats, exit (1 when no frame)\n\
         \x20 --screenshot <PATH>  render one frame to PNG, then exit\n\
         \x20 --settle <SECONDS>   wait before the screenshot (default: as soon as possible)\n\
         \x20 --history-span <SPAN>  1m | 5m | 15m | 1h | all (fixed x axis, default: 1h)\n\
         \x20 --data-dir <PATH>    config/history directory (default: per-user, writable fallback)\n\
         \x20 -h, --help           show this help\n\n\
         Keys: 1/2/3/4 pages · F11 fullscreen · Esc exit fullscreen"
    );
}

fn main() -> eframe::Result {
    let options = parse_args();
    if options.help {
        print_usage();
        return Ok(());
    }
    options
        .validate_durations()
        .map_err(|err| eframe::Error::AppCreation(Box::new(err)))?;
    if let Some(dir) = &options.data_dir {
        sys::set_data_dir(Some(std::path::PathBuf::from(dir)));
    }
    let mut appearance = sys::load_config("appearance.conf")
        .map(|text| Appearance::from_config(&text))
        .unwrap_or_default();
    if let Some(mode) = options.theme {
        appearance.mode = mode;
    }
    if let Some(accent) = options.accent {
        appearance.accent = accent;
    }
    let deterministic = options.screenshot.is_some() || options.smoke_secs.is_some();
    let saved_window = sys::load_config("window.conf")
        .map(|text| WindowState::from_config(&text))
        .unwrap_or_default();
    let window = if deterministic {
        WindowState::default()
    } else {
        saved_window
    };
    let native_options = eframe::NativeOptions {
        viewport: window.viewport(),
        ..Default::default()
    };
    eframe::run_native(
        "open-HiMOS",
        native_options,
        Box::new(move |creation| {
            let mut app = App::new(options, appearance, window);
            app.refresh_theme(&creation.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_status_frame() {
        let mut state = PsuState::default();
        let frame = Frame::new(CMD_STATUS, decoded_status_payload());
        assert!(state.apply(&frame).is_none());
        assert!(state.is_fresh());
        let status = state.fresh_status().expect("fresh status");
        assert!((status.power_w - 151.3).abs() < 0.05);
    }

    #[test]
    fn logs_unknown_frames() {
        let mut state = PsuState::default();
        let line = state.apply(&Frame::new(0x77, [1, 2, 3]));
        assert_eq!(line.as_deref(), Some("frame cmd 0x77 len 3"));
    }

    #[test]
    fn ignores_malformed_payloads() {
        let mut state = PsuState::default();
        assert!(state.apply(&Frame::new(CMD_STATUS, [0u8; 3])).is_none());
        assert!(state.status.is_none());
        assert!(!state.is_fresh());
    }

    #[test]
    fn stale_status_is_hidden() {
        let stale_since = Instant::now()
            .checked_sub(STALE_AFTER + Duration::from_secs(1))
            .expect("test clock is past the staleness window");
        let state = PsuState {
            status: Some(PsuStatus::parse(&decoded_status_payload()).expect("parses")),
            last_status: Some(stale_since),
            ..PsuState::default()
        };
        assert!(!state.is_fresh());
        assert!(state.fresh_status().is_none());
        assert!(state.fresh_aux().is_none());
    }

    #[test]
    fn explicit_port_is_used_for_startup_and_reconnect() {
        let options = parse_args_from(["--port", "manual-port"].map(str::to_owned));
        let mut app = App::new(options, Appearance::default(), WindowState::default());
        for _ in 0..2 {
            app.start_detect();
            let event = app
                .detect_rx
                .take()
                .expect("detection started")
                .recv_timeout(Duration::from_secs(2))
                .expect("override resolved");
            assert!(matches!(event, Detection::Found(port) if port == "manual-port"));
            app.port = None;
        }
    }

    #[test]
    fn rejects_invalid_gui_durations() {
        for flag in ["--settle", "--smoke"] {
            for value in ["-1", "NaN", "inf", "1e300", "abc"] {
                let options = parse_args_from([flag, value].map(str::to_owned));
                assert!(
                    options.validate_durations().is_err(),
                    "accepted {flag} {value}"
                );
            }
            assert!(
                parse_args_from([flag].map(str::to_owned))
                    .validate_durations()
                    .is_err()
            );
            assert!(
                parse_args_from([flag, "0"].map(str::to_owned))
                    .validate_durations()
                    .is_ok()
            );
        }
    }

    #[test]
    fn hidden_logic_does_not_replay_keyboard_input() {
        use eframe::App as _;
        let ctx = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        let mut app = App::new(
            Options::default(),
            Appearance::default(),
            WindowState::default(),
        );
        app.auto_connect = false;
        app.accent_attempted = true;
        let mut input = egui::RawInput::default();
        for key in [egui::Key::F11, egui::Key::Num4] {
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        let mut output = ctx.run_ui(input, |ui| {
            app.logic(ui.ctx(), &mut frame);
            app.ui(ui, &mut frame);
        });
        output.textures_delta.clear();
        assert!(app.fullscreen, "visible F11 enters fullscreen");
        assert_eq!(app.level, Level::History);
        app.level = Level::Overview;
        for _ in 0..2 {
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
            assert!(app.fullscreen, "hidden logic must not replay F11");
            assert_eq!(
                app.level,
                Level::Overview,
                "hidden logic must not replay navigation"
            );
        }
    }

    #[test]
    fn link_error_disables_controls_until_reconnected() {
        let mut app = App::new(
            Options::default(),
            Appearance::default(),
            WindowState::default(),
        );
        let (events, receiver) = mpsc::channel();
        let (commands, receiver_commands) = mpsc::channel();
        app.rx = Some(receiver);
        app.tx = Some(commands);
        events
            .send(FromWorker::Connected("test".into()))
            .expect("event receiver alive");
        app.drain_events();
        assert!(app.connected);
        events
            .send(FromWorker::Error("link lost".into()))
            .expect("event receiver alive");
        app.drain_events();
        assert!(!app.send(Frame::new(CMD_STATUS, []), "test"));
        assert!(receiver_commands.try_recv().is_err());
        events
            .send(FromWorker::Connected("test".into()))
            .expect("event receiver alive");
        app.drain_events();
        assert!(app.send(Frame::new(CMD_STATUS, []), "test"));
    }

    #[test]
    fn commands_are_refused_while_offline() {
        let mut app = App::new(
            Options::default(),
            Appearance::default(),
            WindowState::default(),
        );
        assert!(!app.send(Frame::new(CMD_STATUS, []), "test"));
        assert!(app.error.is_some());
        assert!(app.log.is_empty());
    }

    #[test]
    fn log_is_capped() {
        let mut app = App::new(
            Options::default(),
            Appearance::default(),
            WindowState::default(),
        );
        for index in 0..250 {
            app.push_log(format!("line {index}"));
        }
        assert_eq!(app.log.len(), 200);
        assert_eq!(app.log.back().expect("log is not empty").text, "line 249");
    }

    #[test]
    fn health_reports_stale_telemetry() {
        let mut app = App::new(
            Options::default(),
            Appearance::default(),
            WindowState::default(),
        );
        app.state.status = Some(PsuStatus::parse(&decoded_status_payload()).expect("parses"));
        app.state.last_status = Some(
            Instant::now()
                .checked_sub(STALE_AFTER + Duration::from_secs(1))
                .expect("test clock"),
        );
        assert_eq!(app.health().label(), "STALE");
    }

    #[test]
    fn uptime_stamp_is_formatted() {
        assert_eq!(stamp(Duration::ZERO), "T+00:00");
        assert_eq!(stamp(Duration::from_secs(125)), "T+02:05");
    }

    #[test]
    fn parses_supported_arguments() {
        let options = parse_args_from(
            [
                "--smoke",
                "9",
                "--port",
                "COM3",
                "--level",
                "details",
                "--screenshot",
                "out.png",
            ]
            .map(str::to_owned),
        );
        assert_eq!(options.smoke_secs, Some(9.0));
        assert_eq!(options.port.as_deref(), Some("COM3"));
        assert_eq!(options.level, Some(Level::Details));
        assert_eq!(options.screenshot.as_deref(), Some("out.png"));
        assert!(!options.help);
    }

    #[test]
    fn help_flag_and_unknown_arguments() {
        let options = parse_args_from(["--help".to_owned(), "--bogus".to_owned()]);
        assert!(options.help);
        assert!(options.port.is_none());
    }

    fn decoded_status_payload() -> Vec<u8> {
        // Payload of a captured 0x02 frame (power 151.3 W, +12V 12.089 V).
        vec![
            0xf1, 0x0c, 0xaf, 0x13, 0x39, 0x2f, 0xc7, 0x13, 0x00, 0x00, 0x15, 0x0b, 0xd2, 0x0b,
            0xf9, 0x01, 0x0d, 0x34, 0x9d, 0x43, 0xb1, 0x21, 0xcc, 0x08, 0x00, 0x00, 0x00, 0x00,
        ]
    }
}
