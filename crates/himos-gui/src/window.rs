//! Window geometry: restore the previous size/position and track fullscreen.
//!
//! The panel keeps the operating system's native decorations (minimize,
//! maximize, close, Snap Layouts, Alt+F4), so this module only stores the
//! restored window rectangle and applies it through egui's viewport API.

use std::fmt::Write as _;

use eframe::egui::{self, Pos2, Vec2};

/// Size used when no geometry has been saved yet.
pub(crate) const DEFAULT_SIZE: Vec2 = egui::vec2(1060.0, 800.0);
/// Smallest usable window (matches the layouts in `panels`).
pub(crate) const MIN_SIZE: Vec2 = egui::vec2(940.0, 640.0);
/// How much of the window must stay on screen when a saved position is restored.
const MIN_VISIBLE: f32 = 120.0;

/// Persisted window rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WindowState {
    /// Restored (non-maximized) width in points.
    pub width: f32,
    /// Restored (non-maximized) height in points.
    pub height: f32,
    /// Outer top-left in monitor space, when known.
    pub position: Option<Pos2>,
    /// Whether the window was maximized.
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: DEFAULT_SIZE.x,
            height: DEFAULT_SIZE.y,
            position: None,
            maximized: false,
        }
    }
}

impl WindowState {
    /// Parses `key=value` lines; unknown or malformed keys keep the defaults.
    pub(crate) fn from_config(text: &str) -> Self {
        let mut state = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "width" => {
                    if let Ok(width) = value.trim().parse::<f32>() {
                        state.width = width;
                    }
                }
                "height" => {
                    if let Ok(height) = value.trim().parse::<f32>() {
                        state.height = height;
                    }
                }
                "maximized" => state.maximized = value.trim() == "true",
                _ => {}
            }
        }
        let x = parse_coordinate(text, "x");
        let y = parse_coordinate(text, "y");
        state.position = x.zip(y).map(|(x, y)| Pos2::new(x, y));
        state
    }

    /// Serialises the rectangle as `key=value` lines.
    pub(crate) fn to_config(self) -> String {
        let mut text = format!(
            "width={:.1}\nheight={:.1}\nmaximized={}\n",
            self.width, self.height, self.maximized
        );
        if let Some(position) = self.position {
            let _ = write!(text, "x={:.1}\ny={:.1}\n", position.x, position.y);
        }
        text
    }

    /// Fits a restored rectangle onto a monitor of the given size.
    pub(crate) fn clamped_to(self, monitor: Vec2) -> Self {
        let width = self.width.clamp(MIN_SIZE.x, monitor.x.max(MIN_SIZE.x));
        let height = self.height.clamp(MIN_SIZE.y, monitor.y.max(MIN_SIZE.y));
        let position = self.position.map(|position| {
            // Negative x is legitimate on multi-monitor setups with a screen to
            // the left; keep the title bar reachable in both axes.
            let x = position
                .x
                .clamp(-(width - MIN_VISIBLE), (monitor.x - MIN_VISIBLE).max(0.0));
            let y = position
                .y
                .clamp(0.0, (monitor.y - MIN_VISIBLE / 2.0).max(0.0));
            Pos2::new(x, y)
        });
        Self {
            width,
            height,
            position,
            maximized: self.maximized,
        }
    }

    /// Records the current native geometry, keeping the restored size while maximized.
    pub(crate) fn capture(&mut self, info: &egui::ViewportInfo) {
        if info.fullscreen == Some(true) {
            self.maximized = false;
            return;
        }
        if info.maximized == Some(true) {
            self.maximized = true;
            return;
        }
        self.maximized = false;
        if let Some(inner) = info.inner_rect {
            self.width = inner.width();
            self.height = inner.height();
        }
        if let Some(outer) = info.outer_rect {
            self.position = Some(outer.min);
        }
    }

    /// Viewport builder that applies the saved rectangle at startup.
    pub(crate) fn viewport(self) -> egui::ViewportBuilder {
        let state = self;
        let mut builder = egui::ViewportBuilder::default()
            .with_title("open-HiMOS · DM-850G")
            .with_inner_size([state.width, state.height])
            .with_min_inner_size([MIN_SIZE.x, MIN_SIZE.y])
            .with_resizable(true)
            .with_decorations(true)
            .with_maximized(state.maximized);
        if let Some(position) = state.position {
            builder = builder.with_position(position);
        }
        builder
    }
}

/// Reads one optional `f32` coordinate from the config text.
fn parse_coordinate(text: &str, key: &str) -> Option<f32> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key)
            .then(|| value.trim().parse::<f32>().ok())
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_round_trips() {
        let state = WindowState {
            width: 1280.0,
            height: 720.0,
            position: Some(Pos2::new(-40.0, 60.0)),
            maximized: true,
        };
        assert_eq!(WindowState::from_config(&state.to_config()), state);
    }

    #[test]
    fn malformed_config_keeps_defaults() {
        let state = WindowState::from_config("width=nope\nmaximized=sure\nx=1\n");
        let default = WindowState::default();
        assert!((state.width - default.width).abs() < f32::EPSILON);
        assert!(!state.maximized);
        assert!(state.position.is_none(), "x without y is ignored");
    }

    #[test]
    fn restored_geometry_is_clamped_to_the_monitor() {
        let state = WindowState {
            width: 4000.0,
            height: 3000.0,
            position: Some(Pos2::new(-3900.0, -20.0)),
            maximized: false,
        };
        let clamped = state.clamped_to(egui::vec2(1920.0, 1080.0));
        assert_eq!(clamped.width, 1920.0);
        assert_eq!(clamped.height, 1080.0);
        let position = clamped.position.expect("position kept");
        assert!(position.x >= -(1920.0 - MIN_VISIBLE));
        assert_eq!(position.y, 0.0);
    }

    #[test]
    fn maximized_keeps_the_restored_size() {
        let mut state = WindowState::default();
        let info = egui::ViewportInfo {
            maximized: Some(true),
            ..Default::default()
        };
        state.capture(&info);
        assert!(state.maximized);
        assert!((state.width - DEFAULT_SIZE.x).abs() < f32::EPSILON);
    }

    #[test]
    fn viewport_builder_applies_saved_geometry() {
        let state = WindowState {
            width: 1920.0,
            height: 1080.0,
            position: Some(Pos2::new(10.0, 10.0)),
            maximized: true,
        };
        let builder = state.viewport();
        assert_eq!(builder.inner_size, Some(egui::vec2(1920.0, 1080.0)));
        assert_eq!(builder.maximized, Some(true));
        assert_eq!(builder.position, Some(Pos2::new(10.0, 10.0)));

        let default = WindowState::default().viewport();
        assert_eq!(default.inner_size, Some(DEFAULT_SIZE));
        assert_eq!(default.maximized, Some(false));
        assert_eq!(default.position, None);
    }
}
