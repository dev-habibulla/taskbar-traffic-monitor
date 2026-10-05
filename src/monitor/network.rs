//! Network throughput derived from interface byte counters (`GetIfTable2`).
//!
//! Counters are tracked **per interface**. Aggregating first and diffing later
//! would be wrong: the set of active interfaces (VPNs, virtual adapters, …)
//! changes between samples, which makes a summed counter non-monotonic and
//! would occasionally fold an interface's entire lifetime traffic into a single
//! sample. Tracking each interface independently keeps both the rate and the
//! session total correct.

use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

/// Sent/received byte counters for one connected interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfaceTotals {
    pub key: u64,
    pub up: u64,
    pub down: u64,
}

/// One network reading: rates plus the raw deltas used for session totals.
pub struct NetSample {
    pub upload_bps: f64,
    pub download_bps: f64,
    pub up_delta: u64,
    pub down_delta: u64,
}

/// Converts per-interface monotonic byte counters into rates.
///
/// Kept independent of the Win32 layer so it can be unit tested deterministically.
pub struct SpeedMeter {
    previous: HashMap<u64, (u64, u64)>,
}

impl SpeedMeter {
    pub fn new() -> Self {
        Self {
            previous: HashMap::new(),
        }
    }

    /// Returns `(upload_bps, download_bps, up_delta, down_delta)` for the
    /// interval since the previous call. An interface seen for the first time
    /// contributes nothing, so a counter's entire lifetime value is never
    /// mistaken for one interval's traffic.
    pub fn update(
        &mut self,
        interfaces: &[InterfaceTotals],
        elapsed: Duration,
    ) -> (f64, f64, u64, u64) {
        let mut up_delta = 0u64;
        let mut down_delta = 0u64;
        let mut current = HashMap::with_capacity(interfaces.len());

        for interface in interfaces {
            let (interface_up, interface_down) = match self.previous.get(&interface.key) {
                Some(&(prev_up, prev_down)) => (
                    counter_delta(prev_up, interface.up),
                    counter_delta(prev_down, interface.down),
                ),
                None => (0, 0),
            };
            up_delta = up_delta.saturating_add(interface_up);
            down_delta = down_delta.saturating_add(interface_down);
            current.insert(interface.key, (interface.up, interface.down));
        }

        self.previous = current;

        let seconds = elapsed.as_secs_f64();
        // Guard against a zero interval so the first sample never divides by zero.
        let seconds = if seconds <= 0.0 { 1.0 } else { seconds };
        (
            up_delta as f64 / seconds,
            down_delta as f64 / seconds,
            up_delta,
            down_delta,
        )
    }
}

impl Default for SpeedMeter {
    fn default() -> Self {
        Self::new()
    }
}

/// A counter that goes backwards on a single interface (the adapter was reset)
/// is treated as a fresh start, so the delta is the new value.
fn counter_delta(previous: u64, current: u64) -> u64 {
    if current >= previous {
        current - previous
    } else {
        current
    }
}

pub struct NetworkSampler {
    meter: SpeedMeter,
    last: Option<Instant>,
}

impl NetworkSampler {
    /// Creates a sampler and takes an initial reading as the baseline.
    pub fn new() -> Self {
        let mut sampler = Self {
            meter: SpeedMeter::new(),
            last: None,
        };
        let _ = sampler.sample();
        sampler
    }

    pub fn sample(&mut self) -> Option<NetSample> {
        let interfaces = read_interfaces()?;
        let now = Instant::now();
        let elapsed = self.last.map(|t| now - t).unwrap_or(Duration::ZERO);
        self.last = Some(now);
        let (upload_bps, download_bps, up_delta, down_delta) =
            self.meter.update(&interfaces, elapsed);
        Some(NetSample {
            upload_bps,
            download_bps,
            up_delta,
            down_delta,
        })
    }
}

impl Default for NetworkSampler {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads sent/received byte counters for every connected, non-loopback interface.
pub fn read_interfaces() -> Option<Vec<InterfaceTotals>> {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        let result = GetIfTable2(&mut table);
        if result.0 != 0 || table.is_null() {
            return None;
        }

        let count = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), count);
        let mut interfaces = Vec::with_capacity(count);
        for row in rows {
            if row.OperStatus == IfOperStatusUp && row.Type != IF_TYPE_SOFTWARE_LOOPBACK {
                interfaces.push(InterfaceTotals {
                    key: row.InterfaceIndex as u64,
                    up: row.OutOctets,
                    down: row.InOctets,
                });
            }
        }

        FreeMibTable(table as *const c_void);
        Some(interfaces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(key: u64, up: u64, down: u64) -> InterfaceTotals {
        InterfaceTotals { key, up, down }
    }

    #[test]
    fn upload_and_download_rates_are_computed() {
        let mut meter = SpeedMeter::new();
        let first = meter.update(&[iface(1, 1_000, 2_000)], Duration::from_secs(1));
        assert_eq!(first, (0.0, 0.0, 0, 0)); // first sight contributes nothing

        let second = meter.update(&[iface(1, 2_000, 4_000)], Duration::from_secs(1));
        assert_eq!(second.2, 1_000);
        assert_eq!(second.3, 2_000);
        assert!((second.0 - 1000.0).abs() < 0.001);
        assert!((second.1 - 2000.0).abs() < 0.001);
    }

    #[test]
    fn rate_scales_with_elapsed_time() {
        let mut meter = SpeedMeter::new();
        meter.update(&[iface(1, 0, 0)], Duration::from_millis(500));
        let s = meter.update(&[iface(1, 4_096, 8_192)], Duration::from_secs(2));
        assert!((s.0 - 2048.0).abs() < 0.001);
        assert!((s.1 - 4096.0).abs() < 0.001);
    }

    #[test]
    fn counter_reset_is_treated_as_fresh_start() {
        let mut meter = SpeedMeter::new();
        meter.update(&[iface(1, 1_000_000, 1_000_000)], Duration::from_secs(1));
        let s = meter.update(&[iface(1, 500, 800)], Duration::from_secs(1));
        assert_eq!(s.2, 500);
        assert_eq!(s.3, 800);
    }

    #[test]
    fn brand_new_interface_is_not_counted_as_a_huge_delta() {
        // Regression: at startup the interface table can be empty and then
        // appear a moment later. A newly seen interface must contribute 0, not
        // its whole lifetime octet count.
        let mut meter = SpeedMeter::new();
        let first = meter.update(&[], Duration::from_secs(1));
        assert_eq!(first, (0.0, 0.0, 0, 0));

        let second = meter.update(
            &[iface(7, 50_000_000_000, 60_000_000_000)],
            Duration::from_secs(1),
        );
        assert_eq!(second.2, 0);
        assert_eq!(second.3, 0);
    }

    #[test]
    fn disappearing_interface_does_not_break_rates() {
        let mut meter = SpeedMeter::new();
        meter.update(
            &[iface(1, 1_000, 1_000), iface(2, 5_000, 5_000)],
            Duration::from_secs(1),
        );
        let s = meter.update(&[iface(1, 1_500, 1_500)], Duration::from_secs(1));
        assert_eq!(s.2, 500);
        assert_eq!(s.3, 500);
    }

    #[test]
    fn reconnected_interface_restarts_from_zero() {
        // Regression for adapter drop/reconnect (Wi-Fi/Ethernet/VPN): the
        // interface vanishes and comes back with fresh counters. The reconnect
        // must not be counted as a burst of traffic.
        let mut meter = SpeedMeter::new();
        meter.update(&[iface(1, 1_000, 2_000)], Duration::from_secs(1));
        meter.update(&[], Duration::from_secs(1)); // adapter disappears
        let reconnect = meter.update(&[iface(1, 9_000_000, 8_000_000)], Duration::from_secs(1));
        assert_eq!(reconnect.2, 0);
        assert_eq!(reconnect.3, 0);

        // Subsequent deltas are measured normally.
        let next = meter.update(&[iface(1, 9_000_500, 8_000_800)], Duration::from_secs(1));
        assert_eq!(next.2, 500);
        assert_eq!(next.3, 800);
    }

    #[test]
    fn multiple_interfaces_are_summed() {
        let mut meter = SpeedMeter::new();
        meter.update(&[iface(1, 0, 0), iface(2, 0, 0)], Duration::from_secs(1));
        let s = meter.update(
            &[iface(1, 100, 200), iface(2, 300, 400)],
            Duration::from_secs(1),
        );
        assert_eq!(s.2, 400);
        assert_eq!(s.3, 600);
    }

    #[test]
    fn zero_elapsed_does_not_divide_by_zero() {
        let mut meter = SpeedMeter::new();
        meter.update(&[iface(1, 0, 0)], Duration::ZERO);
        let s = meter.update(&[iface(1, 1_000, 1_000)], Duration::ZERO);
        assert!(s.0.is_finite() && s.1.is_finite());
    }

    #[test]
    fn live_counters_are_readable() {
        let interfaces = read_interfaces();
        assert!(
            interfaces.is_some(),
            "GetIfTable2 should succeed on Windows"
        );
    }
}
