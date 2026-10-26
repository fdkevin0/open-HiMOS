//! Palette, theme modes and accent handling.
//!
//! The panel follows the operating-system appearance on Windows and macOS and
//! can also be pinned to dark or light. The accent colour comes from the user
//! (`Accent`), otherwise from the OS (`sys::system_accent`), otherwise from the
//! default for the active mode.
//!
//! The resolved [`Palette`] is published for the drawing helpers through
//! [`palette`], which [`apply`] sets once per frame.

use std::cell::RefCell;

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Stroke, Style, TextStyle, Theme,
    ThemePreference, Visuals,
};

/// Accent used in dark mode when nothing else is available.
pub(crate) const DEFAULT_DARK_ACCENT: Color32 = Color32::from_rgb(0x35, 0xC6, 0xF0);
/// Accent used in light mode when nothing else is available.
pub(crate) const DEFAULT_LIGHT_ACCENT: Color32 = Color32::from_rgb(0x0B, 0x6B, 0xCB);

thread_local! {
    static ACTIVE: RefCell<Palette> = RefCell::new(Palette::dark(DEFAULT_DARK_ACCENT));
}

/// How the panel chooses between the dark and the light palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeMode {
    /// Follow the operating system.
    System,
    /// Always dark.
    Dark,
    /// Always light.
    Light,
}

impl ThemeMode {
    /// Every mode, in menu order.
    pub(crate) const ALL: [Self; 3] = [Self::System, Self::Dark, Self::Light];

    /// Short uppercase label.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::System => "SYSTEM",
            Self::Dark => "DARK",
            Self::Light => "LIGHT",
        }
    }

    /// Parses the persisted key.
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "system" => Some(Self::System),
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }

    /// Persisted key.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// Whether a segmented button is the active choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    /// Currently selected.
    On,
    /// Not selected.
    Off,
}

impl Mark {
    /// Whether this mark is selected.
    pub(crate) fn is_on(self) -> bool {
        self == Self::On
    }
}

/// Where the accent colour comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Accent {
    /// Follow the operating-system accent colour.
    Auto,
    /// Cyan.
    Cyan,
    /// Blue.
    Blue,
    /// Green.
    Green,
    /// Amber.
    Amber,
    /// Violet.
    Violet,
    /// Rose.
    Rose,
}

impl Accent {
    /// Every choice, in swatch order.
    pub(crate) const ALL: [Self; 7] = [
        Self::Auto,
        Self::Cyan,
        Self::Blue,
        Self::Green,
        Self::Amber,
        Self::Violet,
        Self::Rose,
    ];

    /// Short label.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Auto => "OS",
            Self::Cyan => "CYAN",
            Self::Blue => "BLUE",
            Self::Green => "GREEN",
            Self::Amber => "AMBER",
            Self::Violet => "VIOLET",
            Self::Rose => "ROSE",
        }
    }

    /// Fixed colour, or `None` for `Auto`.
    pub(crate) fn color(self) -> Option<Color32> {
        match self {
            Self::Auto => None,
            Self::Cyan => Some(Color32::from_rgb(0x35, 0xC6, 0xF0)),
            Self::Blue => Some(Color32::from_rgb(0x3B, 0x82, 0xF6)),
            Self::Green => Some(Color32::from_rgb(0x22, 0xC5, 0x5E)),
            Self::Amber => Some(Color32::from_rgb(0xF5, 0x9E, 0x0B)),
            Self::Violet => Some(Color32::from_rgb(0xA8, 0x55, 0xF7)),
            Self::Rose => Some(Color32::from_rgb(0xF4, 0x3F, 0x5E)),
        }
    }

    /// Parses the persisted key.
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "auto" | "os" => Some(Self::Auto),
            "cyan" => Some(Self::Cyan),
            "blue" => Some(Self::Blue),
            "green" => Some(Self::Green),
            "amber" => Some(Self::Amber),
            "violet" => Some(Self::Violet),
            "rose" => Some(Self::Rose),
            _ => None,
        }
    }

    /// Persisted key.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cyan => "cyan",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Amber => "amber",
            Self::Violet => "violet",
            Self::Rose => "rose",
        }
    }
}

/// Persisted appearance selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Appearance {
    /// Dark/light policy.
    pub mode: ThemeMode,
    /// Accent source.
    pub accent: Accent,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            mode: ThemeMode::System,
            accent: Accent::Auto,
        }
    }
}

impl Appearance {
    /// Serialises the selection as `key=value` lines.
    pub(crate) fn to_config(self) -> String {
        format!("mode={}\naccent={}\n", self.mode.key(), self.accent.key())
    }

    /// Parses the persisted selection; unknown lines and values keep the defaults.
    pub(crate) fn from_config(text: &str) -> Self {
        let mut appearance = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "mode" => {
                    if let Some(mode) = ThemeMode::from_key(value) {
                        appearance.mode = mode;
                    }
                }
                "accent" => {
                    if let Some(accent) = Accent::from_key(value) {
                        appearance.accent = accent;
                    }
                }
                _ => {}
            }
        }
        appearance
    }
}

/// Colours for one resolved appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Palette {
    /// Window background.
    pub bg: Color32,
    /// Header/footer strip.
    pub bar: Color32,
    /// Card background.
    pub surface: Color32,
    /// Alternate row background.
    pub surface_alt: Color32,
    /// Widget background on hover.
    pub surface_hover: Color32,
    /// Widget background while active.
    pub surface_active: Color32,
    /// Hairline border.
    pub border: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary text and labels.
    pub muted: Color32,
    /// Live-value accent.
    pub accent: Color32,
    /// Text drawn on an accent fill.
    pub on_accent: Color32,
    /// Good/OK state.
    pub good: Color32,
    /// Warning state.
    pub warn: Color32,
    /// Alarm state.
    pub alarm: Color32,
    /// Empty part of a gauge.
    pub gauge_bg: Color32,
}

impl Palette {
    /// Industrial dark palette with the given accent.
    pub(crate) fn dark(accent: Color32) -> Self {
        let accent = brighten_for_dark(accent);
        Self {
            bg: Color32::from_rgb(0x0B, 0x10, 0x15),
            bar: Color32::from_rgb(0x0E, 0x15, 0x1D),
            surface: Color32::from_rgb(0x14, 0x1D, 0x28),
            surface_alt: Color32::from_rgb(0x17, 0x22, 0x2E),
            surface_hover: Color32::from_rgb(0x1E, 0x2C, 0x3B),
            surface_active: Color32::from_rgb(0x24, 0x36, 0x49),
            border: Color32::from_rgb(0x22, 0x30, 0x3F),
            text: Color32::from_rgb(0xDC, 0xE7, 0xF2),
            muted: Color32::from_rgb(0x7C, 0x93, 0xA8),
            accent,
            on_accent: contrasting_text(accent),
            good: Color32::from_rgb(0x3D, 0xDC, 0x97),
            warn: Color32::from_rgb(0xF5, 0xB8, 0x41),
            alarm: Color32::from_rgb(0xFF, 0x5C, 0x5C),
            gauge_bg: Color32::from_rgb(0x1B, 0x28, 0x36),
        }
    }

    /// Day palette with the given accent.
    pub(crate) fn light(accent: Color32) -> Self {
        let accent = darken_for_light(accent);
        Self {
            bg: Color32::from_rgb(0xF2, 0xF5, 0xF8),
            bar: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            surface_alt: Color32::from_rgb(0xEE, 0xF2, 0xF6),
            surface_hover: Color32::from_rgb(0xE2, 0xE9, 0xF0),
            surface_active: Color32::from_rgb(0xD5, 0xDF, 0xE9),
            border: Color32::from_rgb(0xD3, 0xDC, 0xE6),
            text: Color32::from_rgb(0x17, 0x22, 0x2E),
            muted: Color32::from_rgb(0x5B, 0x6B, 0x7A),
            accent,
            on_accent: contrasting_text(accent),
            good: Color32::from_rgb(0x12, 0x85, 0x5A),
            warn: Color32::from_rgb(0xB2, 0x6B, 0x00),
            alarm: Color32::from_rgb(0xC5, 0x30, 0x30),
            gauge_bg: Color32::from_rgb(0xDE, 0xE6, 0xEE),
        }
    }
}

/// Relative luminance of a colour, `0.0` black to `1.0` white.
fn luminance(color: Color32) -> f32 {
    let [red, green, blue, _] = color.to_array();
    (0.299 * f32::from(red) + 0.587 * f32::from(green) + 0.114 * f32::from(blue)) / 255.0
}

/// Dark text on bright accents, white text on dark ones.
fn contrasting_text(accent: Color32) -> Color32 {
    if luminance(accent) > 0.55 {
        Color32::from_rgb(0x0B, 0x10, 0x15)
    } else {
        Color32::WHITE
    }
}

/// Keeps an OS accent visible on the dark background.
fn brighten_for_dark(accent: Color32) -> Color32 {
    if luminance(accent) < 0.35 {
        accent.gamma_multiply(1.6)
    } else {
        accent
    }
}

/// Keeps an OS accent legible on the light background.
fn darken_for_light(accent: Color32) -> Color32 {
    if luminance(accent) > 0.62 {
        accent.gamma_multiply(0.72)
    } else {
        accent
    }
}

/// Resolves the palette for the current appearance and OS hints.
pub(crate) fn resolve(
    appearance: Appearance,
    os_theme: Option<Theme>,
    os_accent: Option<Color32>,
) -> (Palette, Theme) {
    let theme = match appearance.mode {
        ThemeMode::Dark => Theme::Dark,
        ThemeMode::Light => Theme::Light,
        ThemeMode::System => os_theme.unwrap_or(Theme::Dark),
    };
    let fallback = if theme == Theme::Dark {
        DEFAULT_DARK_ACCENT
    } else {
        DEFAULT_LIGHT_ACCENT
    };
    let accent = appearance.accent.color().or(os_accent).unwrap_or(fallback);
    let palette = if theme == Theme::Dark {
        Palette::dark(accent)
    } else {
        Palette::light(accent)
    };
    (palette, theme)
}

/// Publishes the palette for the drawing helpers and restyles every widget.
pub(crate) fn apply(ctx: &egui::Context, palette: Palette, theme: Theme) {
    set_palette(palette);
    let dark = theme == Theme::Dark;
    let mut style: Style = (*ctx.style_of(theme)).clone();
    style.visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    restyle(&mut style.visuals, palette);
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 22.0;
    style.text_styles = std::collections::BTreeMap::from([
        (
            TextStyle::Heading,
            FontId::new(17.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (
            TextStyle::Monospace,
            FontId::new(13.5, FontFamily::Monospace),
        ),
        (
            TextStyle::Button,
            FontId::new(12.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(10.5, FontFamily::Proportional),
        ),
    ]);

    ctx.set_theme(if dark {
        ThemePreference::Dark
    } else {
        ThemePreference::Light
    });
    let system_theme = if dark {
        egui::SystemTheme::Dark
    } else {
        egui::SystemTheme::Light
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(system_theme));
    // Both style slots get the same palette: the panel does not rely on egui's
    // built-in themes once it has resolved one.
    ctx.set_style_of(Theme::Light, style.clone());
    ctx.set_style_of(Theme::Dark, style);
}

/// Writes the resolved palette into the style.
fn restyle(visuals: &mut Visuals, palette: Palette) {
    visuals.panel_fill = palette.bg;
    visuals.window_fill = palette.bar;
    visuals.extreme_bg_color = palette.bg;
    visuals.faint_bg_color = palette.surface_alt;
    visuals.override_text_color = Some(palette.text);
    visuals.hyperlink_color = palette.accent;
    visuals.window_stroke = Stroke::new(1.0_f32, palette.border);
    visuals.selection.bg_fill = palette.accent.gamma_multiply(0.30);
    visuals.selection.stroke = Stroke::new(1.0_f32, palette.accent);

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(4);
        widget.bg_stroke = Stroke::new(1.0_f32, palette.border);
    }
    visuals.widgets.noninteractive.bg_fill = palette.surface;
    visuals.widgets.inactive.weak_bg_fill = palette.surface_alt;
    visuals.widgets.inactive.bg_fill = palette.gauge_bg;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, palette.muted);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, palette.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, palette.accent);
    visuals.widgets.hovered.bg_fill = palette.surface_hover;
    visuals.widgets.active.bg_fill = palette.surface_active;
    visuals.widgets.hovered.weak_bg_fill = palette.surface_hover;
    visuals.widgets.active.weak_bg_fill = palette.surface_active;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, palette.accent);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, palette.accent);
}

/// Publishes `palette` for the current thread (the UI thread).
fn set_palette(palette: Palette) {
    ACTIVE.with(|slot| *slot.borrow_mut() = palette);
}

/// The palette of the current frame.
pub(crate) fn palette() -> Palette {
    ACTIVE.with(|slot| *slot.borrow())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_follows_the_os_theme() {
        let appearance = Appearance::default();
        let (light_palette, theme) = resolve(appearance, Some(Theme::Light), None);
        assert_eq!(theme, Theme::Light);
        assert_eq!(
            light_palette.surface,
            Palette::light(DEFAULT_LIGHT_ACCENT).surface
        );
        assert_eq!(resolve(appearance, Some(Theme::Dark), None).1, Theme::Dark);
        assert_eq!(
            resolve(appearance, None, None).1,
            Theme::Dark,
            "unknown OS theme falls back to dark"
        );
    }

    #[test]
    fn pinned_modes_ignore_the_os() {
        let dark = Appearance {
            mode: ThemeMode::Dark,
            accent: Accent::Auto,
        };
        let light = Appearance {
            mode: ThemeMode::Light,
            accent: Accent::Auto,
        };
        assert_eq!(resolve(dark, Some(Theme::Light), None).1, Theme::Dark);
        assert_eq!(resolve(light, Some(Theme::Dark), None).1, Theme::Light);
    }

    #[test]
    fn accents_follow_user_then_os_then_default() {
        let user = Appearance {
            mode: ThemeMode::Dark,
            accent: Accent::Rose,
        };
        let os = Color32::from_rgb(0x12, 0x34, 0x56);
        let (palette, _) = resolve(user, Some(Theme::Dark), Some(os));
        assert_eq!(palette.accent, Accent::Rose.color().expect("fixed colour"));

        let auto = Appearance::default();
        let (palette, _) = resolve(auto, Some(Theme::Dark), Some(os));
        assert_eq!(palette.accent, brighten_for_dark(os));

        let (palette, _) = resolve(auto, Some(Theme::Dark), None);
        assert_eq!(palette.accent, DEFAULT_DARK_ACCENT);
    }

    #[test]
    fn light_palette_darkens_bright_accents_for_contrast() {
        let amber = Accent::Amber.color().expect("fixed colour");
        let palette = Palette::light(amber);
        assert!(luminance(palette.accent) <= luminance(amber));
        assert_eq!(palette.on_accent, Color32::WHITE);
        let bright = Palette::dark(Color32::WHITE);
        assert_eq!(bright.on_accent, Color32::from_rgb(0x0B, 0x10, 0x15));
    }

    #[test]
    fn appearance_config_round_trips() {
        let appearance = Appearance {
            mode: ThemeMode::Light,
            accent: Accent::Violet,
        };
        assert_eq!(Appearance::from_config(&appearance.to_config()), appearance);
        assert_eq!(
            Appearance::from_config("mode=weird\naccent=nope\n"),
            Appearance::default()
        );
        assert_eq!(
            Appearance::from_config("mode=dark\n"),
            Appearance {
                mode: ThemeMode::Dark,
                accent: Accent::Auto,
            }
        );
    }
}
