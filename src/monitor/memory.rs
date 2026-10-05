//! Physical memory usage via `GlobalMemoryStatusEx`.

use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

pub struct MemSample {
    pub percent: f32,
    pub used: u64,
    pub total: u64,
}

/// Reads current physical memory usage.
pub fn sample() -> Option<MemSample> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe {
        GlobalMemoryStatusEx(&mut status).ok()?;
    }
    let total = status.ullTotalPhys;
    let available = status.ullAvailPhys;
    let used = total.saturating_sub(available);
    Some(MemSample {
        percent: percent_used(total, available),
        used,
        total,
    })
}

/// Percentage used given total and available bytes.
pub fn percent_used(total: u64, available: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    let used = total.saturating_sub(available);
    ((used as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentage_is_computed_from_available() {
        let p = percent_used(16 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024);
        assert!((p - 50.0).abs() < 0.01);
    }

    #[test]
    fn zero_total_is_safe() {
        assert_eq!(percent_used(0, 0), 0.0);
    }

    #[test]
    fn live_sample_is_in_range() {
        let s = sample().expect("memory sample");
        assert!(s.total > 0);
        assert!(s.used <= s.total);
        assert!(
            (0.0..=100.0).contains(&s.percent),
            "mem out of range: {}",
            s.percent
        );
    }
}
