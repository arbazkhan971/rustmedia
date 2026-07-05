//! Terminal formatting helpers: human-readable sizes/bitrates and light,
//! opt-in ANSI colour that switches itself off when stdout is not a TTY.

use std::io::IsTerminal;
use std::sync::OnceLock;

fn color_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        // Honour the informal NO_COLOR convention and require a real terminal.
        std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
    })
}

/// Wrap `text` in an ANSI SGR code when colour is enabled.
fn paint(code: &str, text: &str) -> String {
    if color_enabled() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// Bold text (used for the file name / headings).
pub(crate) fn bold(text: &str) -> String {
    paint("1", text)
}

/// Dimmed text (used for field labels).
pub(crate) fn dim(text: &str) -> String {
    paint("2", text)
}

/// Cyan text (used for stream kinds).
pub(crate) fn cyan(text: &str) -> String {
    paint("36", text)
}

/// Green text (used for codec names).
pub(crate) fn green(text: &str) -> String {
    paint("32", text)
}

/// A red "error:" prefix for diagnostics.
pub(crate) fn error_prefix() -> String {
    paint("1;31", "error:")
}

/// Format a byte count with binary units (KiB, MiB, …).
pub(crate) fn size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Format a bitrate in bits per second as `kb/s` or `Mb/s`.
pub(crate) fn bitrate(bits_per_sec: u64) -> String {
    let kbps = bits_per_sec as f64 / 1000.0;
    if kbps >= 1000.0 {
        format!("{:.1} Mb/s", kbps / 1000.0)
    } else {
        format!("{kbps:.0} kb/s")
    }
}

/// Format a sample rate in hertz as `44.1 kHz` (or `48 kHz` when integral).
pub(crate) fn sample_rate(hz: u32) -> String {
    let khz = f64::from(hz) / 1000.0;
    if (khz.fract()).abs() < f64::EPSILON {
        format!("{khz:.0} kHz")
    } else {
        format!("{khz:.1} kHz")
    }
}

/// Name a channel count: `mono`, `stereo`, `5.1`, or `N ch`.
pub(crate) fn channels(count: u16) -> String {
    match count {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        6 => "5.1".to_string(),
        8 => "7.1".to_string(),
        n => format!("{n} ch"),
    }
}

/// Format a frame rate, trimming a trailing `.0` (so `30`, not `30.0`).
pub(crate) fn fps(rate: f64) -> String {
    if (rate.fract()).abs() < 0.01 {
        format!("{rate:.0} fps")
    } else {
        format!("{rate:.2} fps")
    }
}
