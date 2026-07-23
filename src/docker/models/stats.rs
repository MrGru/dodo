//! The CPU-percent computation, kept out of the service so it can be tested
//! without a daemon.
//!
//! Docker (and the Podman compatibility API) report cumulative CPU counters, so
//! a percentage is a *delta* between two samples, not a single reading. The
//! engine's own non-streaming stats call leaves the "previous" counters zeroed
//! on Podman, so [`services`](crate::docker::services) takes two frames from the
//! streaming stats endpoint and hands both to [`cpu_percent`]. That keeps this
//! correct regardless of whether the daemon populates its own `precpu` block.

/// One CPU stats sample: the container's cumulative CPU time and the host's
/// cumulative CPU time, both in the engine's arbitrary units, plus how many
/// cores were online for that sample.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CpuSample {
    /// `cpu_stats.cpu_usage.total_usage`.
    pub container_usage: u64,
    /// `cpu_stats.system_cpu_usage`.
    pub system_usage: u64,
    /// `cpu_stats.online_cpus`, defaulted to 1 by the caller when the engine
    /// omits it.
    pub online_cpus: u64,
}

/// The busy-percent of one container between two samples, using Docker's own
/// formula: the container's share of the host CPU delta, scaled by the core
/// count. `later` must be the more recent sample.
///
/// Returns `None` when the host counter did not advance between the samples
/// (division would be undefined) — the caller shows that as "no reading" rather
/// than a misleading `0.0`. The result is clamped to `0.0..=100*cores` and can
/// legitimately exceed 100 on a multi-core container.
pub fn cpu_percent(earlier: CpuSample, later: CpuSample) -> Option<f64> {
    let cpu_delta = later.container_usage.checked_sub(earlier.container_usage)? as f64;
    let system_delta = later.system_usage.checked_sub(earlier.system_usage)? as f64;
    if system_delta <= 0.0 {
        return None;
    }
    let cores = later.online_cpus.max(1) as f64;
    let percent = (cpu_delta / system_delta) * cores * 100.0;
    Some(percent.clamp(0.0, cores * 100.0))
}

#[cfg(test)]
mod tests {
    use super::{CpuSample, cpu_percent};

    fn sample(container: u64, system: u64, cpus: u64) -> CpuSample {
        CpuSample {
            container_usage: container,
            system_usage: system,
            online_cpus: cpus,
        }
    }

    #[test]
    fn half_of_one_core_reads_fifty_percent() {
        // Container advanced 50 units while the host advanced 100, on 1 core.
        let earlier = sample(100, 1_000, 1);
        let later = sample(150, 1_100, 1);
        assert_eq!(cpu_percent(earlier, later), Some(50.0));
    }

    #[test]
    fn core_count_scales_the_result() {
        // Same 50/100 share, but on 4 cores → 200%.
        let earlier = sample(0, 0, 4);
        let later = sample(50, 100, 4);
        assert_eq!(cpu_percent(earlier, later), Some(200.0));
    }

    #[test]
    fn a_stalled_host_counter_is_no_reading() {
        let earlier = sample(100, 1_000, 1);
        let later = sample(150, 1_000, 1);
        assert_eq!(cpu_percent(earlier, later), None);
    }

    #[test]
    fn counters_going_backwards_do_not_panic() {
        let earlier = sample(200, 2_000, 1);
        let later = sample(100, 1_000, 1);
        assert_eq!(cpu_percent(earlier, later), None);
    }

    #[test]
    fn an_idle_container_reads_zero() {
        let earlier = sample(500, 1_000, 2);
        let later = sample(500, 1_200, 2);
        assert_eq!(cpu_percent(earlier, later), Some(0.0));
    }
}
