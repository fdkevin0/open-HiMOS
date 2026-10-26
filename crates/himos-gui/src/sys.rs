//! Operating-system integration: accent colour and the settings file location.
//!
//! Dark/light preference comes from the windowing layer (`egui::RawInput`), so
//! only the accent colour is read here, plus the per-user config directory.

use std::path::PathBuf;

use eframe::egui::Color32;

/// Confidence for the OS accent: `None` when the platform does not expose one.
pub(crate) fn system_accent() -> Option<Color32> {
    imp::read_accent()
}

/// Application data directory, resolved once and cached.
static DATA_DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

/// Overrides the data directory (`--data-dir`, `HIMOS_DATA_DIR`); call before
/// the first [`data_dir`] use. An unwritable override is reported by the caller.
pub(crate) fn set_data_dir(dir: Option<PathBuf>) {
    let _ = DATA_DIR.set(dir);
}

/// First writable candidate: explicit override, then the platform config dir,
/// then a temp fallback, then the working directory (portable/read-only setups).
pub(crate) fn data_dir() -> Option<PathBuf> {
    DATA_DIR.get_or_init(resolve_data_dir).clone()
}

/// Path of `file_name` inside the data directory.
pub(crate) fn config_path(file_name: &str) -> Option<PathBuf> {
    Some(data_dir()?.join(file_name))
}

/// Reads a persisted config file, if any.
pub(crate) fn load_config(file_name: &str) -> Option<String> {
    std::fs::read_to_string(config_path(file_name)?).ok()
}

/// Persists a config file; failures are non-fatal.
pub(crate) fn save_config(file_name: &str, contents: &str) {
    let Some(path) = config_path(file_name) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, contents);
}

/// Resolves a writable data directory.
fn resolve_data_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(explicit) = std::env::var_os("HIMOS_DATA_DIR") {
        candidates.push(PathBuf::from(explicit));
    }
    if cfg!(target_os = "windows") {
        for variable in ["APPDATA", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(variable) {
                candidates.push(PathBuf::from(base).join("open-HiMOS"));
            }
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join("Library/Application Support/open-HiMOS"));
        }
    } else if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
        candidates.push(PathBuf::from(base).join("open-HiMOS"));
    } else if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".config/open-HiMOS"));
    }
    candidates.push(std::env::temp_dir().join("open-HiMOS"));
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".open-HiMOS"));
    }
    candidates.into_iter().find(|dir| is_writable(dir))
}

/// Checks that a directory can be created and written.
fn is_writable(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-probe");
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

/// Parses `reg query` output for a `REG_DWORD` value.
///
/// Windows stores colorization colours as `0xAABBGGRR`, so the red channel is
/// the lowest byte.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn parse_windows_color(text: &str) -> Option<Color32> {
    let raw = text
        .split_whitespace()
        .skip_while(|word| *word != "REG_DWORD")
        .nth(1)?;
    let value =
        u32::from_str_radix(raw.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()?;
    let red = u8::try_from(value & 0xFF).ok()?;
    let green = u8::try_from((value >> 8) & 0xFF).ok()?;
    let blue = u8::try_from((value >> 16) & 0xFF).ok()?;
    let alpha = u8::try_from((value >> 24) & 0xFF)
        .ok()
        .filter(|alpha| *alpha != 0)
        .unwrap_or(0xFF);
    Some(Color32::from_rgba_unmultiplied(red, green, blue, alpha))
}

/// Maps a macOS `AppleAccentColor` index to its system colour.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn parse_apple_accent(text: &str) -> Option<Color32> {
    let index: i32 = text.trim().parse().ok()?;
    let rgb = match index {
        -1 => (0x8E, 0x8E, 0x93),
        0 => (0xFF, 0x52, 0x57),
        1 => (0xFF, 0x9F, 0x0A),
        2 => (0xFF, 0xD6, 0x0A),
        3 => (0x32, 0xD7, 0x4B),
        4 => (0x0A, 0x84, 0xFF),
        5 => (0xBF, 0x5A, 0xF2),
        6 => (0xFF, 0x37, 0x5F),
        _ => return None,
    };
    Some(Color32::from_rgb(rgb.0, rgb.1, rgb.2))
}

#[cfg(target_os = "windows")]
mod imp {
    use std::process::Command;

    use eframe::egui::Color32;

    /// Reads the DWM colorization colour, then the Explorer accent colour.
    pub(super) fn read_accent() -> Option<Color32> {
        let queries = [
            (r"HKCU\Software\Microsoft\Windows\DWM", "ColorizationColor"),
            (
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Accent",
                "AccentColorMenu",
            ),
        ];
        for (key, value) in queries {
            let Ok(output) = Command::new("reg")
                .args(["query", key, "/v", value])
                .output()
            else {
                continue;
            };
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(color) = crate::sys::parse_windows_color(&text) {
                return Some(color);
            }
        }
        None
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::Command;

    use eframe::egui::Color32;

    /// Reads the system accent colour through the global defaults domain.
    pub(super) fn read_accent() -> Option<Color32> {
        let output = Command::new("defaults")
            .args(["read", "-g", "AppleAccentColor"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        crate::sys::parse_apple_accent(&text)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod imp {
    use eframe::egui::Color32;

    /// Other platforms do not expose a system accent to the panel.
    pub(super) fn read_accent() -> Option<Color32> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_windows_dword_as_abgr() {
        let output = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\DWM\r\n    \
                      ColorizationColor    REG_DWORD    0xc40078d7\r\n";
        assert_eq!(
            parse_windows_color(output),
            Some(Color32::from_rgba_unmultiplied(0xD7, 0x78, 0x00, 0xC4))
        );
    }

    #[test]
    fn rejects_unrelated_or_malformed_output() {
        assert_eq!(parse_windows_color("no value here"), None);
        assert_eq!(parse_windows_color("x REG_DWORD 0xZZ"), None);
    }

    #[test]
    fn maps_apple_accent_indices() {
        assert_eq!(
            parse_apple_accent("4\r\n"),
            Some(Color32::from_rgb(0x0A, 0x84, 0xFF))
        );
        assert_eq!(
            parse_apple_accent("-1"),
            Some(Color32::from_rgb(0x8E, 0x8E, 0x93))
        );
        assert_eq!(parse_apple_accent("9"), None);
        assert_eq!(parse_apple_accent(""), None);
    }
}
