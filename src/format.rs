//! Formatting of byte rates, byte totals and percentages.
//!
//! Network speeds pick their unit automatically, so the user never has to
//! choose between KB/s, MB/s and GB/s.

const KB: f64 = 1024.0;
const MB: f64 = 1024.0 * 1024.0;
const GB: f64 = 1024.0 * 1024.0 * 1024.0;
const TB: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0;

/// Splits a byte-per-second rate into a numeric string and a unit suffix.
pub fn split_speed(bytes_per_sec: f64) -> (String, &'static str) {
    let bps = if bytes_per_sec.is_finite() && bytes_per_sec > 0.0 {
        bytes_per_sec
    } else {
        0.0
    };
    if bps < KB {
        (format!("{bps:.0}"), "B/s")
    } else if bps < MB {
        (format!("{:.1}", bps / KB), "KB/s")
    } else if bps < GB {
        let v = bps / MB;
        if v < 10.0 {
            (format!("{v:.2}"), "MB/s")
        } else {
            (format!("{v:.1}"), "MB/s")
        }
    } else if bps < TB {
        (format!("{:.2}", bps / GB), "GB/s")
    } else {
        (format!("{:.2}", bps / TB), "TB/s")
    }
}

/// Formats a byte-per-second rate such as `835.7 KB/s`.
#[allow(dead_code)] // exercised by the unit tests; kept as a convenience API
pub fn format_speed(bytes_per_sec: f64) -> String {
    let (n, u) = split_speed(bytes_per_sec);
    format!("{n} {u}")
}

/// Splits a cumulative byte count into a numeric string and a unit suffix.
pub fn split_total(bytes: u64) -> (String, &'static str) {
    let b = bytes as f64;
    if b < KB {
        (format!("{b:.0}"), "B")
    } else if b < MB {
        (trim(format!("{:.2}", b / KB)), "KB")
    } else if b < GB {
        (trim(format!("{:.2}", b / MB)), "MB")
    } else if b < TB {
        (trim(format!("{:.2}", b / GB)), "GB")
    } else {
        (trim(format!("{:.2}", b / TB)), "TB")
    }
}

/// Formats a cumulative byte count such as `249 MB`.
pub fn format_total(bytes: u64) -> String {
    let (n, u) = split_total(bytes);
    format!("{n} {u}")
}

/// Rounds a 0..=100 percentage to a whole number and appends `%`.
pub fn format_percent(percent: f32) -> String {
    let p = if percent.is_finite() {
        percent.clamp(0.0, 100.0)
    } else {
        0.0
    };
    format!("{p:.0}%")
}

fn trim(s: String) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_units_switch_automatically() {
        assert_eq!(format_speed(41.6 * KB), "41.6 KB/s");
        assert_eq!(format_speed(835.7 * KB), "835.7 KB/s");
        assert_eq!(format_speed(1.25 * MB), "1.25 MB/s");
        assert_eq!(format_speed(12.8 * MB), "12.8 MB/s");
        assert_eq!(format_speed(1.24 * GB), "1.24 GB/s");
    }

    #[test]
    fn speed_handles_small_and_invalid_values() {
        assert_eq!(format_speed(0.0), "0 B/s");
        assert_eq!(format_speed(512.0), "512 B/s");
        assert_eq!(format_speed(-5.0), "0 B/s");
        assert_eq!(format_speed(f64::NAN), "0 B/s");
        assert_eq!(format_speed(f64::INFINITY), "0 B/s");
    }

    #[test]
    fn speed_crosses_unit_boundaries() {
        assert_eq!(split_speed(1023.0).1, "B/s");
        assert_eq!(split_speed(1024.0).1, "KB/s");
        assert_eq!(split_speed(MB).1, "MB/s");
        assert_eq!(split_speed(GB).1, "GB/s");
        assert_eq!(split_speed(TB).1, "TB/s");
    }

    #[test]
    fn total_trims_trailing_zeros() {
        assert_eq!(format_total(249 * 1024 * 1024), "249 MB");
        assert_eq!(format_total(1024), "1 KB");
        assert_eq!(format_total(0), "0 B");
        assert_eq!(format_total(1536), "1.5 KB");
    }

    #[test]
    fn percent_is_clamped_and_rounded() {
        assert_eq!(format_percent(59.4), "59%");
        assert_eq!(format_percent(48.6), "49%");
        assert_eq!(format_percent(150.0), "100%");
        assert_eq!(format_percent(-3.0), "0%");
    }
}
