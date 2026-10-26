//! Small drawing primitives shared by the panels.

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Margin, Rect, RichText, Sense, Stroke, Ui,
};

use himos_core::dm850g::CurvePoint;

use crate::health::{TEMP_ALARM_C, TEMP_WARN_C, severity_of_deviation, severity_of_temperature};
use crate::theme::{Mark, palette};

/// Frame used for every panel card.
pub(crate) fn card<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(palette().surface)
        .stroke(Stroke::new(1.0_f32, palette().border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if !title.is_empty() {
                ui.label(
                    RichText::new(title.to_uppercase())
                        .size(9.5)
                        .color(palette().muted)
                        .strong(),
                );
                ui.add_space(7.0);
            }
            add(ui)
        })
        .inner
}

/// Small status pill used in the top strip.
pub(crate) fn chip(ui: &mut Ui, label: &str, value: &str, color: Color32) {
    egui::Frame::NONE
        .fill(palette().surface_alt)
        .stroke(Stroke::new(1.0_f32, palette().border))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(label)
                        .size(9.0)
                        .color(palette().muted)
                        .strong(),
                );
                ui.label(
                    RichText::new(value)
                        .size(11.5)
                        .color(color)
                        .monospace()
                        .strong(),
                );
            });
        });
}

/// Status lamp with a soft halo.
pub(crate) fn led(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 5.0, color.gamma_multiply(0.18));
    painter.circle_filled(rect.center(), 3.1, color);
}

/// Label plus monospaced value with a smaller unit.
pub(crate) fn metric(ui: &mut Ui, label: &str, value: &str, unit: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(label)
                .size(9.0)
                .color(palette().muted)
                .strong(),
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.label(
                RichText::new(value)
                    .size(23.0)
                    .color(color)
                    .monospace()
                    .strong(),
            );
            ui.label(
                RichText::new(unit)
                    .size(10.5)
                    .color(palette().muted)
                    .monospace(),
            );
        });
    });
}

/// Large hero number for the overview power card.
pub(crate) fn hero_metric(ui: &mut Ui, value: &str, unit: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new(value)
                .size(44.0)
                .color(palette().accent)
                .monospace()
                .strong(),
        );
        ui.label(
            RichText::new(unit)
                .size(14.0)
                .color(palette().muted)
                .monospace(),
        );
    });
}

/// Horizontal bar gauge, `fraction` in `0.0..=1.0`.
pub(crate) fn bar(ui: &mut Ui, fraction: f32, color: Color32, height: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(2), palette().gauge_bg);
    let filled = fraction.clamp(0.0, 1.0);
    if filled > 0.001 {
        let fill = Rect::from_min_size(rect.min, egui::vec2(rect.width() * filled, height));
        painter.rect_filled(fill, CornerRadius::same(2), color);
    }
}

/// Deviation gauge centred on the nominal value, with a subtle ±5% band.
pub(crate) fn deviation_bar(ui: &mut Ui, deviation: f32) {
    let width = ui.available_width().min(260.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(2), palette().gauge_bg);

    let band = Rect::from_center_size(rect.center(), egui::vec2(rect.width() * 0.55, 6.0));
    painter.rect_filled(
        band,
        CornerRadius::same(2),
        palette().good.gamma_multiply(0.10),
    );
    painter.line_segment(
        [
            egui::pos2(rect.center().x, rect.top()),
            egui::pos2(rect.center().x, rect.bottom()),
        ],
        Stroke::new(1.0_f32, palette().border),
    );

    let normalised = (deviation / 0.10).clamp(-1.0, 1.0);
    let x = rect.center().x + normalised * rect.width() * 0.5;
    let color = severity_of_deviation(deviation).color();
    let marker = Rect::from_center_size(
        egui::pos2(x, rect.center().y),
        egui::vec2(3.0, rect.height() + 4.0),
    );
    painter.rect_filled(marker, CornerRadius::same(1), color);
}

/// Temperature bar with warning and alarm marks.
pub(crate) fn temp_bar(ui: &mut Ui, celsius: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(420.0), 6.0),
        Sense::hover(),
    );
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(2), palette().gauge_bg);
    let fraction = (celsius / 100.0).clamp(0.0, 1.0);
    let color = severity_of_temperature(celsius).color();
    if fraction > 0.001 {
        let fill = Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, 6.0));
        painter.rect_filled(fill, CornerRadius::same(2), color);
    }
    for (mark, color) in [
        (TEMP_WARN_C, palette().warn),
        (TEMP_ALARM_C, palette().alarm),
    ] {
        let x = rect.left() + rect.width() * (mark / 100.0);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(1.0_f32, color.gamma_multiply(0.55)),
        );
    }
}

/// Fan-curve preview: temperature on X, duty on Y, with the implied endpoints.
pub(crate) fn curve_preview(ui: &mut Ui, points: &[CurvePoint]) {
    let width = ui.available_width().clamp(220.0, 320.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 132.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), palette().bg);

    let grid = Stroke::new(1.0_f32, palette().border.gamma_multiply(0.7));
    for step in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        let x = rect.left() + rect.width() * step;
        let y = rect.bottom() - rect.height() * step;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            grid,
        );
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            grid,
        );
    }

    let to_point = |temperature: f32, duty: f32| {
        egui::pos2(
            rect.left() + rect.width() * (temperature / 100.0),
            rect.bottom() - rect.height() * (duty / 100.0),
        )
    };

    let mut path = vec![to_point(0.0, 0.0)];
    path.extend(points.iter().map(|point| {
        to_point(
            f32::from(point.temperature_c),
            f32::from(point.duty_percent),
        )
    }));
    path.push(to_point(100.0, 100.0));
    for window in path.windows(2) {
        painter.line_segment(
            [window[0], window[1]],
            Stroke::new(1.6_f32, palette().accent),
        );
    }
    painter.circle_filled(path[0], 2.5, palette().muted);
    painter.circle_filled(path[path.len() - 1], 2.5, palette().muted);
    for point in &path[1..path.len() - 1] {
        painter.circle_filled(*point, 3.0, palette().accent);
    }

    let small = FontId::new(9.0, egui::FontFamily::Monospace);
    painter.text(
        egui::pos2(rect.left() + 4.0, rect.bottom() - 12.0),
        egui::Align2::LEFT_BOTTOM,
        "0 °C",
        small.clone(),
        palette().muted,
    );
    painter.text(
        egui::pos2(rect.right() - 4.0, rect.bottom() - 12.0),
        egui::Align2::RIGHT_BOTTOM,
        "100 °C",
        small.clone(),
        palette().muted,
    );
    painter.text(
        egui::pos2(rect.left() + 4.0, rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        "100 %",
        small,
        palette().muted,
    );
}

/// Two-column key/value row used inside `Grid`s.
pub(crate) fn kv(ui: &mut Ui, key: &str, value: &str) {
    ui.label(RichText::new(key).size(9.5).color(palette().muted).strong());
    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
        ui.label(
            RichText::new(value)
                .size(12.0)
                .color(palette().text)
                .monospace(),
        );
    });
    ui.end_row();
}

/// Clickable accent swatch; `None` draws the half-dark/half-light OS glyph.
pub(crate) fn accent_swatch(ui: &mut Ui, color: Option<Color32>, label: &str, mark: Mark) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::click());
    let painter = ui.painter();
    if let Some(color) = color {
        painter.circle_filled(rect.center(), 7.0, color);
    } else {
        {
            let dark =
                Rect::from_center_size(rect.center() - egui::vec2(3.5, 0.0), egui::vec2(7.0, 14.0));
            let light =
                Rect::from_center_size(rect.center() + egui::vec2(3.5, 0.0), egui::vec2(7.0, 14.0));
            painter.rect_filled(dark, CornerRadius::same(2), palette().gauge_bg);
            painter.rect_filled(light, CornerRadius::same(2), palette().text);
        }
    }
    if mark.is_on() {
        painter.circle_stroke(rect.center(), 9.0, Stroke::new(1.5_f32, palette().accent));
    }
    response.on_hover_text(label).clicked()
}
