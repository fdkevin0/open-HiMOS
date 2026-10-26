//! Panel layout: masthead, status strip, tabs and the three disclosure levels.

use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke, Ui};
use egui_plot::{Line, Plot, PlotPoints};

use himos_core::dm850g::{
    CurvePoint, FanMode, PsuStatus, RefreshInterval, clear_fault, set_custom_curve, set_fan_mode,
    set_refresh_interval,
};

use crate::health::{
    Health, deviation, health_of, rails, severity_of_deviation, severity_of_temperature,
};
use crate::history::{History, HistoryWindow, Metric, Stats};
use crate::theme::{Accent, Appearance, Mark, ThemeMode, palette};
use crate::widgets::{
    accent_swatch, bar, card, chip, curve_preview, deviation_bar, hero_metric, kv, led, metric,
    temp_bar,
};
use crate::{App, Level};

/// Rated output power used for the load gauge.
const RATED_W: f32 = 850.0;
/// AC input window used for the input gauge.
const AC_MIN_V: f32 = 180.0;
/// AC input window upper bound.
const AC_MAX_V: f32 = 260.0;

impl App {
    /// Health derived from fresh telemetry, or stale/standby when there is none.
    pub(crate) fn health(&self) -> Health {
        match self.state.fresh_status() {
            Some(status) => health_of(status),
            None if self.state.status.is_some() => Health::Stale,
            None => Health::Standby,
        }
    }

    /// Brand line with link state and the detected port.
    pub(crate) fn masthead(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("open-HiMOS")
                    .size(18.0)
                    .strong()
                    .color(palette().text),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new("DIGITAL PSU CONTROL")
                    .size(8.5)
                    .color(palette().muted)
                    .strong(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (color, label) = if self.connected {
                    (palette().good, "ONLINE")
                } else {
                    (palette().warn, "SEARCHING")
                };
                ui.label(RichText::new(label).size(10.0).strong().color(color));
                led(ui, color);
                ui.add_space(12.0);
                let port = self.port.as_deref().unwrap_or("—");
                ui.label(
                    RichText::new(format!("{port}  ·  115200 8N1"))
                        .size(10.5)
                        .color(palette().muted)
                        .monospace(),
                );
                ui.label(
                    RichText::new("PORT")
                        .size(8.5)
                        .color(palette().muted)
                        .strong(),
                );
            });
        });
    }

    /// One-line telemetry summary with the alarm state.
    pub(crate) fn status_strip(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if let Some(status) = self.state.fresh_status() {
                chip(
                    ui,
                    "POWER",
                    &format!("{:.1} W", status.power_w),
                    palette().accent,
                );
                chip(
                    ui,
                    "+12V",
                    &format!("{:.3} V", status.voltage_12v),
                    severity_of_deviation(deviation(12.0, status.voltage_12v)).color(),
                );
                chip(
                    ui,
                    "TEMP",
                    &format!("{:.1} °C", status.temp_high_c),
                    crate::health::severity_of_temperature(status.temp_high_c).color(),
                );
                chip(
                    ui,
                    "AC",
                    &format!(
                        "{:.1} V / {:.1} Hz",
                        status.input_voltage_v, status.input_frequency_hz
                    ),
                    palette().text,
                );
            } else if self.state.status.is_some() {
                chip(ui, "TELEMETRY", "STALE", palette().warn);
            } else {
                chip(ui, "STATUS", "waiting for telemetry", palette().muted);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let health = self.health();
                chip(ui, "SYSTEM", health.label(), health.color());
                chip(ui, "FRAMES", &self.frames_seen.to_string(), palette().muted);
            });
        });
    }

    /// Disclosure tabs.
    pub(crate) fn tabs(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 20.0;
            for (level, label) in [
                (Level::Overview, "OVERVIEW"),
                (Level::Details, "DETAILS"),
                (Level::Advanced, "ADVANCED"),
                (Level::History, "HISTORY"),
            ] {
                let selected = self.level == level;
                let color = if selected {
                    palette().accent
                } else {
                    palette().muted
                };
                let response = ui.add(
                    egui::Button::new(RichText::new(label).size(11.0).strong().color(color))
                        .frame(false),
                );
                if selected {
                    let underline = egui::Rect::from_min_size(
                        egui::pos2(response.rect.left(), response.rect.bottom() + 1.0),
                        egui::vec2(response.rect.width(), 2.0),
                    );
                    ui.painter().rect_filled(
                        underline,
                        egui::CornerRadius::same(1),
                        palette().accent,
                    );
                }
                if response.clicked() {
                    self.level = level;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let position = match self.level {
                    Level::Overview => 1,
                    Level::Details => 2,
                    Level::Advanced => 3,
                    Level::History => 4,
                };
                ui.label(
                    RichText::new(format!("{position} / 4"))
                        .size(8.5)
                        .color(palette().muted),
                );
            });
        });
    }

    /// Footer with build and shortcut hints.
    pub(crate) fn footer(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("open-HiMOS · Segotep DM-850G")
                    .size(9.0)
                    .color(palette().muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new("1–4 pages · F11 fullscreen")
                        .size(9.0)
                        .color(palette().muted),
                );
                if let Some(error) = &self.error {
                    ui.label(RichText::new(error).size(9.0).color(palette().warn));
                }
            });
        });
    }

    /// Level 1: headline numbers.
    pub(crate) fn overview(&self, ui: &mut Ui) {
        let Some(status) = self.state.fresh_status() else {
            self.waiting(ui);
            return;
        };
        ui.columns(2, |columns| {
            power_card(&mut columns[0], status, self.state.fresh_aux());
            identity_card(&mut columns[1], self);
        });
        ui.add_space(8.0);
        let values = rails(status);
        ui.columns(4, |columns| {
            for (index, (label, nominal, measured)) in values.into_iter().enumerate() {
                rail_card(&mut columns[index], label, nominal, measured);
            }
        });
        ui.add_space(8.0);
        thermal_card(ui, status, self);
        ui.add_space(8.0);
        system_card(ui, self);
    }

    /// Level 2: every measured value.
    pub(crate) fn details(&self, ui: &mut Ui) {
        let Some(status) = self.state.fresh_status() else {
            self.waiting(ui);
            return;
        };
        card(ui, "Rail telemetry", |ui| rail_table(ui, status));
        ui.add_space(10.0);
        ui.columns(2, |columns| {
            card(&mut columns[0], "AC input", |ui| ac_panel(ui, status));
            card(&mut columns[1], "Registers", |ui| {
                register_panel(ui, self, status);
            });
        });
        ui.add_space(10.0);
        card(ui, "Thermal detail", |ui| {
            for (label, temperature) in [
                ("HIGH SIDE", status.temp_high_c),
                ("LOW SIDE", status.temp_low_c),
                ("AMBIENT", status.temp_ambient_c),
            ] {
                thermal_row(ui, label, temperature);
            }
            if let Some(aux) = self.state.fresh_aux() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "extended frame: 12 V rail ≈ {:.1} W, low-side {:.1} °C (inferred)",
                        aux.power_12v_w, aux.temp_low_c
                    ))
                    .size(9.5)
                    .color(palette().muted),
                );
            }
        });
    }

    /// Level 3: write controls and the event log.
    pub(crate) fn advanced(&mut self, ui: &mut Ui) {
        ui.columns(2, |columns| {
            self.fan_card(&mut columns[0]);
            self.interval_card(&mut columns[1]);
        });
        ui.add_space(10.0);
        self.curve_card(ui);
        ui.add_space(10.0);
        self.maintenance_card(ui);
        ui.add_space(10.0);
        self.appearance_card(ui);
        ui.add_space(10.0);
        self.log_card(ui);
    }

    fn waiting(&self, ui: &mut Ui) {
        card(ui, "Link", |ui| {
            let message = if self.state.status.is_some() {
                "Telemetry stale — waiting for the link"
            } else {
                "Waiting for telemetry…"
            };
            ui.label(RichText::new(message).size(13.0).color(palette().muted));
            ui.add_space(4.0);
            let port = self.port.as_deref().unwrap_or("—");
            ui.label(
                RichText::new(format!("port {port} · 115200 8N1 · autodetect every 2 s"))
                    .size(9.5)
                    .color(palette().muted)
                    .monospace(),
            );
        });
    }

    fn fan_card(&mut self, ui: &mut Ui) {
        let current = self.fan_mode;
        card(ui, "Fan control", |ui| {
            ui.horizontal_wrapped(|ui| {
                for (label, mode) in [
                    ("AUTO", FanMode::Auto),
                    ("SILENT", FanMode::Silent),
                    ("OVERCLOCK", FanMode::Overclock),
                    ("CUSTOM", FanMode::Custom),
                    ("DUST", FanMode::Dust),
                ] {
                    let mark = if current == Some(mode) {
                        Mark::On
                    } else {
                        Mark::Off
                    };
                    if mode_button(ui, mark, label).clicked()
                        && self.send(set_fan_mode(mode), &format!("fan mode {mode}"))
                    {
                        self.fan_mode = Some(mode);
                    }
                }
            });
            ui.add_space(6.0);
            let note = match current {
                Some(mode) => format!("device set to {}", mode.label().to_uppercase()),
                None => "mode unknown — pick one to write it".to_owned(),
            };
            ui.label(RichText::new(note).size(9.0).color(palette().muted));
        });
    }

    fn interval_card(&mut self, ui: &mut Ui) {
        let current = self.interval;
        card(ui, "Reporting interval", |ui| {
            ui.horizontal_wrapped(|ui| {
                for millis in [1000u16, 1500, 2000, 2500, 3000] {
                    let selected = current.is_some_and(|value| value.millis() == millis);
                    let label = format!("{:.1} S", f32::from(millis) / 1000.0);
                    let mark = if selected { Mark::On } else { Mark::Off };
                    if mode_button(ui, mark, &label).clicked()
                        && let Ok(next) = RefreshInterval::try_new(millis)
                        && self.send(set_refresh_interval(next), "interval")
                    {
                        self.interval = Some(next);
                    }
                }
            });
            ui.add_space(6.0);
            let note = current.map_or_else(
                || "interval unknown — pick one to write it".to_owned(),
                |value| format!("device reports every {} ms", value.millis()),
            );
            ui.label(RichText::new(note).size(9.0).color(palette().muted));
        });
    }

    fn curve_card(&mut self, ui: &mut Ui) {
        let points = self.curve;
        let mut draft = points;
        card(ui, "Custom fan curve", |ui| {
            ui.columns(2, |columns| {
                for (index, point) in draft.iter_mut().enumerate() {
                    curve_row(&mut columns[0], index, point);
                }
                curve_preview(&mut columns[1], &draft);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("APPLY CURVE")
                                .size(10.0)
                                .strong()
                                .color(palette().bg),
                        )
                        .fill(palette().accent),
                    )
                    .clicked()
                {
                    match set_custom_curve(&draft) {
                        Ok(frame) => {
                            if self.send(frame, "custom curve") {
                                self.fan_mode = Some(FanMode::Custom);
                            }
                        }
                        Err(err) => self.error = Some(format!("curve rejected: {err}")),
                    }
                }
                ui.label(
                    RichText::new("device implies (0 °C, 0 %) and (100 °C, 100 %)")
                        .size(9.0)
                        .color(palette().muted),
                );
            });
        });
        self.curve = draft;
    }

    fn maintenance_card(&mut self, ui: &mut Ui) {
        let connected = self.connected;
        card(ui, "Maintenance", |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("CLEAR STORED FAULT")
                                .size(10.0)
                                .strong()
                                .color(palette().alarm),
                        )
                        .fill(palette().surface_alt)
                        .stroke(Stroke::new(1.0_f32, palette().alarm)),
                    )
                    .clicked()
                {
                    self.send(clear_fault(), "clear fault");
                }
                if ui.button(RichText::new("RESCAN PORT").size(10.0)).clicked() {
                    self.request_rescan();
                }
                if connected && ui.button(RichText::new("DISCONNECT").size(10.0)).clicked() {
                    self.auto_connect = false;
                    self.disconnect();
                }
            });
        });
    }

    fn appearance_card(&mut self, ui: &mut Ui) {
        let appearance = self.appearance;
        let os_theme = match self.os_theme {
            Some(egui::Theme::Dark) => "dark",
            Some(egui::Theme::Light) => "light",
            None => "unknown",
        };
        let os_accent = self.os_accent.map_or_else(
            || "n/a".to_owned(),
            |color| format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b()),
        );
        card(ui, "Appearance", |ui| {
            ui.horizontal(|ui| {
                for mode in ThemeMode::ALL {
                    let mark = if appearance.mode == mode {
                        Mark::On
                    } else {
                        Mark::Off
                    };
                    if mode_button(ui, mark, mode.label()).clicked() {
                        self.set_appearance(Appearance { mode, ..appearance });
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("ACCENT")
                        .size(9.0)
                        .color(palette().muted)
                        .strong(),
                );
                for accent in Accent::ALL {
                    let mark = if appearance.accent == accent {
                        Mark::On
                    } else {
                        Mark::Off
                    };
                    if accent_swatch(ui, accent.color(), accent.label(), mark) {
                        self.set_appearance(Appearance {
                            accent,
                            ..appearance
                        });
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("SYNC WITH OS").size(10.0))
                    .clicked()
                {
                    self.set_appearance(Appearance::default());
                }
                ui.label(
                    RichText::new(format!("OS theme {os_theme} · accent {os_accent}"))
                        .size(9.0)
                        .color(palette().muted),
                );
            });
        });
    }

    /// Recorded telemetry: metric/window selection, statistics and the trend chart.
    pub(crate) fn history_panel(&mut self, ui: &mut Ui) {
        let now_s = self.started.elapsed().as_secs_f64();
        let paused = self.history_paused;
        let metric = self.history_metric;
        let window = self.history_window;
        let aggregates = self.history.stats(window, now_s, metric);
        card(ui, "Trend", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("METRIC")
                        .size(9.0)
                        .color(palette().muted)
                        .strong(),
                );
                for option in Metric::ALL {
                    let mark = if metric == option {
                        Mark::On
                    } else {
                        Mark::Off
                    };
                    if mode_button(ui, mark, option.label()).clicked() {
                        self.history_metric = option;
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new("SPAN")
                        .size(9.0)
                        .color(palette().muted)
                        .strong(),
                );
                for option in HistoryWindow::ALL {
                    let mark = if window == option {
                        Mark::On
                    } else {
                        Mark::Off
                    };
                    if mode_button(ui, mark, option.label()).clicked() {
                        self.history_window = option;
                    }
                }
                ui.add_space(10.0);
                let label = if paused { "RESUME" } else { "PAUSE" };
                if ui.button(RichText::new(label).size(10.0)).clicked() {
                    self.history_paused = !paused;
                }
                if ui.button(RichText::new("CLEAR").size(10.0)).clicked() {
                    self.history.clear();
                    self.history_note = None;
                }
                if ui.button(RichText::new("EXPORT CSV").size(10.0)).clicked() {
                    self.export_history();
                }
            });
            ui.add_space(6.0);
            let archive = if self.archive_enabled {
                self.recorder.as_ref().map_or_else(
                    || "archive starting".to_owned(),
                    |recorder| format!("archive {}", recorder.path().display()),
                )
            } else {
                "archive off in deterministic mode".to_owned()
            };
            let status = format!(
                "{} samples · {} · {archive}",
                self.history.len(),
                if paused { "PAUSED" } else { "REC" }
            );
            ui.label(RichText::new(status).size(9.0).color(palette().muted));
            if let Some(error) = &self.recorder_error {
                ui.label(RichText::new(error).size(9.0).color(palette().warn));
            }
            if let Some(note) = &self.history_note {
                ui.label(RichText::new(note).size(9.0).color(palette().accent));
            }
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            history_stats(ui, aggregates, metric);
        });
        ui.add_space(10.0);

        card(ui, "Trend chart", |ui| {
            trend_plot(ui, &self.history, metric, window, now_s);
        });
    }

    fn log_card(&self, ui: &mut Ui) {
        card(ui, "Event log", |ui| {
            ui.set_min_height(64.0);
            if self.log.is_empty() {
                ui.label(
                    RichText::new("no events yet")
                        .size(10.0)
                        .color(palette().muted),
                );
                return;
            }
            egui::ScrollArea::vertical()
                .max_height(168.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for entry in &self.log {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&entry.time)
                                    .size(9.5)
                                    .color(palette().muted)
                                    .monospace(),
                            );
                            ui.label(
                                RichText::new(&entry.text)
                                    .size(10.5)
                                    .color(palette().text)
                                    .monospace(),
                            );
                        });
                    }
                });
        });
    }
}

fn mode_button(ui: &mut Ui, mark: Mark, label: &str) -> egui::Response {
    let selected = mark == Mark::On;
    let color = if selected {
        palette().bg
    } else {
        palette().text
    };
    let fill = if selected {
        palette().accent
    } else {
        palette().surface_alt
    };
    ui.add(
        egui::Button::new(RichText::new(label).size(10.0).strong().color(color))
            .fill(fill)
            .min_size(egui::vec2(66.0, 24.0)),
    )
}

/// Minimum/average/maximum/last row for the selected window.
fn history_stats(ui: &mut Ui, stats: Option<Stats>, metric: Metric) {
    ui.columns(4, |columns| {
        let digits = metric.decimals();
        let unit = metric.unit();
        let Some(stats) = stats else {
            for column in columns.iter_mut() {
                column.label(RichText::new("--").size(12.0).color(palette().muted));
            }
            return;
        };
        stat(
            &mut columns[0],
            "MIN",
            &format!("{:.*} {unit}", digits, stats.min),
            palette().good,
        );
        stat(
            &mut columns[1],
            "AVG",
            &format!("{:.*} {unit}", digits, stats.avg),
            palette().text,
        );
        stat(
            &mut columns[2],
            "MAX",
            &format!("{:.*} {unit}", digits, stats.max),
            palette().warn,
        );
        stat(
            &mut columns[3],
            "LAST",
            &format!("{:.*} {unit}", digits, stats.last),
            palette().accent,
        );
    });
}

/// Draws the selected metric over the selected span.
fn trend_plot(ui: &mut Ui, history: &History, metric: Metric, window: HistoryWindow, now_s: f64) {
    let points: Vec<[f64; 2]> = history
        .window(window, now_s)
        .map(|sample| [sample.elapsed_s - now_s, f64::from(metric.value(sample))])
        .collect();
    if points.len() < 2 {
        ui.label(
            RichText::new("Collecting samples — the chart needs at least two one-second samples")
                .size(10.0)
                .color(palette().muted),
        );
        return;
    }
    let digits = metric.decimals();
    let unit = metric.unit();
    // Fixed time scale: the x axis always spans the selected window, so the
    // chart does not rescale while the buffer fills up. The y axis stays auto.
    let span = window.span_seconds();
    let covered = points
        .first()
        .zip(points.last())
        .map_or(0.0, |(first, last)| (last[0] - first[0]).max(0.0));
    ui.label(
        RichText::new(format!(
            "axis fixed to {} · {} samples · {covered:.0}s recorded",
            window.label(),
            points.len()
        ))
        .size(9.0)
        .color(palette().muted),
    );
    ui.add_space(4.0);
    Plot::new("himos-history-trend")
        .height(340.0)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .show_background(false)
        .show_grid(true)
        .default_x_bounds(-span, 0.0)
        .x_axis_formatter(|mark, _range| format_age(mark.value))
        .y_axis_formatter(move |mark, _range| format!("{:.*} {unit}", digits, mark.value))
        .show(ui, |plot_ui| {
            plot_ui.line(
                Line::new(metric.label(), PlotPoints::from(points))
                    .color(palette().accent)
                    .width(1.8_f32),
            );
        });
}

/// Formats a relative x position (`-300.0`) as `-5m` or `-45s`.
fn format_age(seconds: f64) -> String {
    if seconds.abs() < 90.0 {
        format!("{seconds:.0}s")
    } else {
        format!("{:.0}m", seconds / 60.0)
    }
}

fn stat(ui: &mut Ui, label: &str, value: &str, color: Color32) {
    ui.label(
        RichText::new(label)
            .size(8.5)
            .color(palette().muted)
            .strong(),
    );
    ui.label(RichText::new(value).size(12.5).color(color).monospace());
}

fn power_card(ui: &mut Ui, status: &PsuStatus, aux: Option<&himos_core::dm850g::AuxTelemetry>) {
    card(ui, "Output power", |ui| {
        hero_metric(ui, &format!("{:.0}", status.power_w), "W");
        ui.add_space(2.0);
        bar(ui, status.power_w / RATED_W, palette().accent, 8.0);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("LOAD {:.1} %", status.power_w / RATED_W * 100.0))
                    .size(9.0)
                    .color(palette().muted)
                    .strong(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:.0} / {RATED_W:.0} W", status.power_w))
                        .size(9.0)
                        .color(palette().muted)
                        .monospace(),
                );
            });
        });
        ui.add_space(10.0);
        ui.columns(3, |columns| {
            let rail =
                aux.map_or_else(|| "—".to_owned(), |aux| format!("{:.1} W", aux.power_12v_w));
            stat(&mut columns[0], "12 V RAIL ≈", &rail, palette().muted);
            stat(
                &mut columns[1],
                "AC INPUT",
                &format!("{:.1} V", status.input_voltage_v),
                palette().text,
            );
            stat(
                &mut columns[2],
                "AMBIENT",
                &format!("{:.1} °C", status.temp_ambient_c),
                severity_of_temperature(status.temp_ambient_c).color(),
            );
        });
    });
}

fn system_card(ui: &mut Ui, app: &App) {
    card(ui, "System", |ui| {
        ui.columns(4, |columns| {
            let link = if app.connected {
                ("ONLINE", palette().good)
            } else {
                ("SEARCHING", palette().warn)
            };
            stat(&mut columns[0], "LINK", link.0, link.1);
            stat(&mut columns[1], "UPTIME", &app.uptime, palette().text);
            stat(
                &mut columns[2],
                "FRAMES",
                &app.frames_seen.to_string(),
                palette().text,
            );
            let health = app.health();
            stat(&mut columns[3], "SYSTEM", health.label(), health.color());
        });
    });
}

fn identity_card(ui: &mut Ui, app: &App) {
    card(ui, "Device", |ui| {
        let model = app
            .state
            .info
            .as_ref()
            .map_or("—", |info| info.model.as_str());
        let vendor = app
            .state
            .info
            .as_ref()
            .map_or("—", |info| info.vendor.as_str());
        let serial = app.state.serial.as_deref().unwrap_or("—");
        info_row(ui, "MODEL", model);
        info_row(ui, "VENDOR", vendor);
        info_row(ui, "SERIAL", serial);
        // The vendor status word is shown raw until its bits are decoded.
        info_row(
            ui,
            "STATE",
            &format!("0x{:04X}", app.state.status.map_or(0, |s| s.state_word)),
        );
        info_row(ui, "FRAMES", &app.frames_seen.to_string());
    });
}

fn info_row(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).size(9.0).color(palette().muted).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .size(11.5)
                    .color(palette().text)
                    .monospace(),
            );
        });
    });
}

fn rail_card(ui: &mut Ui, label: &str, nominal: f32, measured: f32) {
    let deviation = deviation(nominal, measured);
    let severity = severity_of_deviation(deviation);
    card(ui, label, |ui| {
        metric(
            ui,
            "MEASURED",
            &format!("{measured:.3}"),
            "V",
            severity.color(),
        );
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("NOM {nominal:.1} V"))
                .size(9.0)
                .color(palette().muted),
        );
        deviation_bar(ui, deviation);
        ui.label(
            RichText::new(format!("{:+.2} %", deviation * 100.0))
                .size(9.0)
                .color(severity.color())
                .monospace(),
        );
    });
}

fn thermal_card(ui: &mut Ui, status: &PsuStatus, app: &App) {
    card(ui, "Thermal", |ui| {
        ui.columns(3, |columns| {
            for (index, (label, temperature)) in [
                ("HIGH SIDE", status.temp_high_c),
                ("LOW SIDE", status.temp_low_c),
                ("AMBIENT", status.temp_ambient_c),
            ]
            .into_iter()
            .enumerate()
            {
                let ui = &mut columns[index];
                metric(
                    ui,
                    label,
                    &format!("{temperature:.1}"),
                    "°C",
                    severity_of_temperature(temperature).color(),
                );
                temp_bar(ui, temperature);
                ui.label(
                    RichText::new("warn 75 · alarm 90 °C")
                        .size(8.5)
                        .color(palette().muted),
                );
            }
        });
        if let Some(aux) = app.state.fresh_aux() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "extended: 12 V rail ≈ {:.1} W · low-side {:.1} °C (inferred)",
                    aux.power_12v_w, aux.temp_low_c
                ))
                .size(9.0)
                .color(palette().muted),
            );
        }
    });
}

fn rail_table(ui: &mut Ui, status: &PsuStatus) {
    let headings = ["RAIL", "MEASURED", "NOMINAL", "DEVIATION", "STATE"];
    let widths = [90.0_f32, 150.0, 130.0, 140.0, 100.0];
    ui.horizontal(|ui| {
        for (index, heading) in headings.iter().enumerate() {
            ui.add_sized(
                [widths[index], 16.0],
                egui::Label::new(
                    RichText::new(*heading)
                        .size(8.5)
                        .color(palette().muted)
                        .strong(),
                ),
            );
        }
    });
    ui.add_space(2.0);
    ui.separator();
    let rows = rails(status);
    for (index, (label, nominal, measured)) in rows.into_iter().enumerate() {
        let deviation = deviation(nominal, measured);
        let severity = severity_of_deviation(deviation);
        let cells: [(String, Color32, bool); 5] = [
            (label.to_owned(), palette().text, true),
            (format!("{measured:.3} V"), palette().text, false),
            (format!("{nominal:.1} V"), palette().muted, false),
            (
                format!("{:+.2} %", deviation * 100.0),
                severity.color(),
                false,
            ),
            (severity.label().to_owned(), severity.color(), true),
        ];
        ui.horizontal(|ui| {
            for (column, (text, text_color, bold)) in cells.iter().enumerate() {
                let mut rich = RichText::new(text)
                    .size(10.5)
                    .color(*text_color)
                    .monospace();
                if *bold {
                    rich = rich.strong();
                }
                ui.add_sized([widths[column], 18.0], egui::Label::new(rich));
            }
        });
        if index + 1 < rows.len() {
            ui.add_space(2.0);
        }
    }
}
fn ac_panel(ui: &mut Ui, status: &PsuStatus) {
    metric(
        ui,
        "AC VOLTAGE",
        &format!("{:.1}", status.input_voltage_v),
        "V",
        palette().text,
    );
    let fraction = (status.input_voltage_v - AC_MIN_V) / (AC_MAX_V - AC_MIN_V);
    bar(ui, fraction, palette().accent, 6.0);
    ui.label(
        RichText::new("nominal 230 V · range 180–260 V")
            .size(8.5)
            .color(palette().muted),
    );
    ui.add_space(6.0);
    metric(
        ui,
        "AC FREQUENCY",
        &format!("{:.1}", status.input_frequency_hz),
        "Hz",
        palette().text,
    );
    bar(
        ui,
        (status.input_frequency_hz - 45.0) / 20.0,
        palette().accent,
        6.0,
    );
    ui.label(
        RichText::new("nominal 50 Hz · range 45–65 Hz")
            .size(8.5)
            .color(palette().muted),
    );
}

fn register_panel(ui: &mut Ui, app: &App, status: &PsuStatus) {
    egui::Grid::new("registers")
        .num_columns(2)
        .spacing([16.0, 5.0])
        .show(ui, |ui| {
            kv(ui, "STATUS WORD", &format!("0x{:04X}", status.state_word));
            kv(ui, "FLAGS", &format!("0x{:04X}", status.flags));
            kv(ui, "FRAMES", &app.frames_seen.to_string());
            kv(ui, "PORT", app.port.as_deref().unwrap_or("—"));
            kv(ui, "LINK", "115200 8N1");
        });
}

fn thermal_row(ui: &mut Ui, label: &str, temperature: f32) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(9.0)
                .color(palette().muted)
                .strong(),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{temperature:.1} °C"))
                    .size(11.0)
                    .color(severity_of_temperature(temperature).color())
                    .monospace(),
            );
        });
    });
    temp_bar(ui, temperature);
    ui.add_space(2.0);
}

fn curve_row(ui: &mut Ui, index: usize, point: &mut CurvePoint) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("P{}", index + 1))
                .size(9.0)
                .color(palette().muted)
                .strong(),
        );
        ui.spacing_mut().slider_width = 110.0;
        ui.add(
            egui::Slider::new(&mut point.temperature_c, 0..=100)
                .show_value(false)
                .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 }),
        );
        ui.label(
            RichText::new(format!("{:>3} °C", point.temperature_c))
                .size(10.5)
                .color(palette().text)
                .monospace(),
        );
        ui.add_space(8.0);
        ui.add(
            egui::Slider::new(&mut point.duty_percent, 0..=100)
                .show_value(false)
                .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 }),
        );
        ui.label(
            RichText::new(format!("{:>3} %", point.duty_percent))
                .size(10.5)
                .color(palette().text)
                .monospace(),
        );
    });
}
