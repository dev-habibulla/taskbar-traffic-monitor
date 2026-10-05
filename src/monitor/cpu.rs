//! CPU usage derived from the kernel/user/idle counters in `GetSystemTimes`.

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Threading::GetSystemTimes;

pub struct CpuSampler {
    previous: Option<(u64, u64, u64)>,
}

impl CpuSampler {
    /// Creates a sampler and takes an initial reading as the baseline.
    pub fn new() -> Self {
        let mut sampler = Self { previous: None };
        let _ = sampler.sample();
        sampler
    }

    /// Returns CPU usage in the range 0..=100 for the interval since the last call.
    pub fn sample(&mut self) -> Option<f32> {
        let current = read_times()?;
        let percent = match self.previous {
            Some(previous) => compute_usage(previous, current),
            None => 0.0,
        };
        self.previous = Some(current);
        Some(percent)
    }
}

impl Default for CpuSampler {
    fn default() -> Self {
        Self::new()
    }
}

fn read_times() -> Option<(u64, u64, u64)> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe {
        GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()?;
    }
    Some((to_u64(idle), to_u64(kernel), to_u64(user)))
}

fn to_u64(value: FILETIME) -> u64 {
    ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
}

/// `kernel` already includes `idle`, so busy time is `kernel + user - idle`.
fn compute_usage(previous: (u64, u64, u64), current: (u64, u64, u64)) -> f32 {
    let idle = current.0.wrapping_sub(previous.0);
    let kernel = current.1.wrapping_sub(previous.1);
    let user = current.2.wrapping_sub(previous.2);
    let total = kernel.wrapping_add(user);
    if total == 0 {
        return 0.0;
    }
    let busy = total.saturating_sub(idle);
    ((busy as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_fraction_matches_expected() {
        // kernel+user = 1000 ticks, idle = 400 -> 60% busy.
        let usage = compute_usage((100, 100, 100), (500, 700, 500));
        assert!((usage - 60.0).abs() < 0.01, "usage was {usage}");
    }

    #[test]
    fn fully_idle_is_zero() {
        let usage = compute_usage((0, 0, 0), (1000, 1000, 0));
        assert_eq!(usage, 0.0);
    }

    #[test]
    fn unchanged_counters_report_zero() {
        assert_eq!(compute_usage((10, 20, 30), (10, 20, 30)), 0.0);
    }

    #[test]
    fn produces_a_percentage_on_live_system() {
        let mut sampler = CpuSampler::new();
        let first = sampler.sample().expect("cpu sample");
        std::thread::sleep(std::time::Duration::from_millis(60));
        let second = sampler.sample().expect("cpu sample");
        assert!((0.0..=100.0).contains(&first));
        assert!(
            (0.0..=100.0).contains(&second),
            "cpu out of range: {second}"
        );
    }
}
