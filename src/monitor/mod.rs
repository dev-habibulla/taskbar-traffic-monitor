//! System metric sampling (CPU, memory, network).

pub mod cpu;
pub mod memory;
pub mod network;

use crate::monitor::{cpu::CpuSampler, network::NetworkSampler};

/// A complete set of sampled values shown on the taskbar.
#[derive(Debug, Clone, Copy, Default)]
pub struct Metrics {
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    pub upload_bps: f64,
    pub download_bps: f64,
    /// Bytes uploaded since this application session started.
    pub total_up_bytes: u64,
    /// Bytes downloaded since this application session started.
    pub total_down_bytes: u64,
}

impl Metrics {
    pub fn total_bytes(&self) -> u64 {
        self.total_up_bytes.saturating_add(self.total_down_bytes)
    }
}

/// Owns the stateful samplers and accumulates session traffic totals.
pub struct Monitor {
    cpu: CpuSampler,
    net: NetworkSampler,
    metrics: Metrics,
}

impl Monitor {
    /// Creates a monitor and primes the samplers so the first displayed sample
    /// already has a meaningful delta.
    pub fn new() -> Self {
        Self {
            cpu: CpuSampler::new(),
            net: NetworkSampler::new(),
            metrics: Metrics::default(),
        }
    }

    /// Samples every metric, updating session totals, and returns the snapshot.
    pub fn sample(&mut self) -> Metrics {
        if let Some(cpu) = self.cpu.sample() {
            self.metrics.cpu_percent = cpu;
        }
        if let Some(mem) = memory::sample() {
            self.metrics.mem_percent = mem.percent;
            self.metrics.mem_used_bytes = mem.used;
            self.metrics.mem_total_bytes = mem.total;
        }
        if let Some(net) = self.net.sample() {
            self.metrics.upload_bps = net.upload_bps;
            self.metrics.download_bps = net.download_bps;
            self.metrics.total_up_bytes = self.metrics.total_up_bytes.saturating_add(net.up_delta);
            self.metrics.total_down_bytes =
                self.metrics.total_down_bytes.saturating_add(net.down_delta);
        }
        self.metrics
    }
}
