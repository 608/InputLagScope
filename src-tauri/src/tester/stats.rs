//! Latency statistics of tester trials.
//!
//! latency = (t1_irq - c1) - (t0_tick + c0), in TIM9 ticks of 10 ns.
//! The statistics are P50/P95/P99, mean, standard deviation (n - 1),
//! minimum and maximum (no trimming, unlike `stats.rs`), always accompanied
//! by the success rate and the count of every fail kind.
//!
//! Which trials are used (`select_press_latencies`, never mixed):
//! * if at least one trial has latency_valid = 1, only those trials, with
//!   the calibrated latency;
//! * otherwise (typically no c0 calibration yet) every successful trial
//!   (fail_kind 0, t1_irq set) with the uncorrected difference
//!   t1_irq - t0_tick, and the run is marked `uncalibrated`.
//!
//! The release side is selected the same way, independently.

use serde::{Deserialize, Serialize};

use crate::models::Summary;
use crate::stats::percentile_sorted;

use super::protocol::{RunDoneEvent, TrialResultEvent, FAIL_KIND_COUNT};

/// TIM9 runs at 100 MHz: 1 tick = 10 ns.
pub const TICK_NS: u64 = 10;
pub const TICKS_PER_US: f64 = 100.0;
pub const TICKS_PER_MS: f64 = 100_000.0;

/// `(t1_irq - c1) - (t0_tick + c0)` in ticks (signed; the 64-bit
/// difference is taken modulo 2^64 first so that ordering of the raw
/// counters does not matter).
pub fn latency_ticks(t1_irq: u64, c1: i32, t0_tick: u64, c0: i32) -> i64 {
    (t1_irq.wrapping_sub(t0_tick) as i64)
        .wrapping_sub(i64::from(c1))
        .wrapping_sub(i64::from(c0))
}

pub fn ticks_to_ms(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_MS
}

pub fn ticks_to_us(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_US
}

/// Press latency of a trial in ticks, when latency_valid is set (and the
/// conditions of 6.6 hold: fail_kind 0, t1_irq non-zero).
pub fn trial_latency_ticks(trial: &TrialResultEvent) -> Option<i64> {
    (trial.latency_valid() && trial.fail_kind == 0 && trial.t1_irq != 0)
        .then(|| latency_ticks(trial.t1_irq, trial.c1, trial.t0_tick, trial.c0))
}

/// Release latency of a trial in ticks, when release_latency_valid is set.
pub fn trial_release_latency_ticks(trial: &TrialResultEvent) -> Option<i64> {
    (trial.release_latency_valid() && trial.release_fail_kind == 0 && trial.release_t1_irq != 0)
        .then(|| {
            latency_ticks(
                trial.release_t1_irq,
                trial.c1,
                trial.release_t0_tick,
                trial.c0,
            )
        })
}

/// Uncorrected press latency `t1_irq - t0_tick` of a successful trial
/// (fail_kind 0, t1_irq set), whatever latency_valid says.
pub fn trial_uncorrected_ticks(trial: &TrialResultEvent) -> Option<i64> {
    (trial.fail_kind == 0 && trial.t1_irq != 0)
        .then(|| trial.t1_irq.wrapping_sub(trial.t0_tick) as i64)
}

/// Uncorrected release latency `release_t1_irq - release_t0_tick` of a
/// trial whose press and release both succeeded.
pub fn trial_uncorrected_release_ticks(trial: &TrialResultEvent) -> Option<i64> {
    (trial.fail_kind == 0
        && trial.release_fail_kind == 0
        && trial.release_t1_irq != 0
        && trial.release_t0_tick != 0)
        .then(|| trial.release_t1_irq.wrapping_sub(trial.release_t0_tick) as i64)
}

/// Latencies used by the statistics, one entry per trial (`None` = not
/// used), and whether they are the uncorrected values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LatencySelection {
    pub ticks: Vec<Option<i64>>,
    pub uncalibrated: bool,
}

impl LatencySelection {
    pub fn values_ms(&self) -> Vec<f64> {
        self.ticks
            .iter()
            .flatten()
            .map(|ticks| ticks_to_ms(*ticks))
            .collect()
    }
}

fn select(
    trials: &[TrialResultEvent],
    calibrated: fn(&TrialResultEvent) -> Option<i64>,
    uncorrected: fn(&TrialResultEvent) -> Option<i64>,
) -> LatencySelection {
    let ticks: Vec<Option<i64>> = trials.iter().map(calibrated).collect();
    if ticks.iter().any(Option::is_some) {
        return LatencySelection {
            ticks,
            uncalibrated: false,
        };
    }
    let ticks: Vec<Option<i64>> = trials.iter().map(uncorrected).collect();
    let uncalibrated = ticks.iter().any(Option::is_some);
    LatencySelection {
        ticks,
        uncalibrated,
    }
}

/// Press latencies used by the statistics (see the module comment).
pub fn select_press_latencies(trials: &[TrialResultEvent]) -> LatencySelection {
    select(trials, trial_latency_ticks, trial_uncorrected_ticks)
}

/// Release latencies used by the statistics (see the module comment).
pub fn select_release_latencies(trials: &[TrialResultEvent]) -> LatencySelection {
    select(
        trials,
        trial_release_latency_ticks,
        trial_uncorrected_release_ticks,
    )
}

/// Latency of the confirmation report (not part of the statistics).
pub fn trial_confirm_latency_ticks(trial: &TrialResultEvent) -> Option<i64> {
    (trial.latency_valid() && trial.confirm_t1_irq != 0)
        .then(|| latency_ticks(trial.confirm_t1_irq, trial.c1, trial.t0_tick, trial.c0))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct LatencyDistribution {
    pub count: usize,
    pub mean_ms: Option<f64>,
    /// Sample standard deviation (divisor n - 1); `None` below 2 samples.
    pub std_dev_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub p05_ms: Option<f64>,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

/// Percentiles use linear interpolation between closest ranks, the same
/// definition as `stats.rs::percentile_sorted`.
pub fn distribution(values_ms: &[f64]) -> LatencyDistribution {
    if values_ms.is_empty() {
        return LatencyDistribution::default();
    }
    let mut sorted = values_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    let count = sorted.len();
    let mean = sorted.iter().sum::<f64>() / count as f64;
    let std_dev = (count >= 2).then(|| {
        let variance = sorted
            .iter()
            .map(|value| {
                let delta = value - mean;
                delta * delta
            })
            .sum::<f64>()
            / (count as f64 - 1.0);
        variance.sqrt()
    });
    LatencyDistribution {
        count,
        mean_ms: Some(mean),
        std_dev_ms: std_dev,
        min_ms: sorted.first().copied(),
        p05_ms: Some(percentile_sorted(&sorted, 0.05)),
        p50_ms: Some(percentile_sorted(&sorted, 0.50)),
        p95_ms: Some(percentile_sorted(&sorted, 0.95)),
        p99_ms: Some(percentile_sorted(&sorted, 0.99)),
        max_ms: sorted.last().copied(),
    }
}

/// Maps the tester distribution onto the existing `Summary` shape used by
/// the GUI (average = untrimmed mean, jitter = standard deviation,
/// median = P50).
pub fn summary_from_distribution(distribution: &LatencyDistribution) -> Summary {
    Summary {
        count: distribution.count,
        average_ms: distribution.mean_ms,
        jitter_ms: distribution
            .std_dev_ms
            .or((distribution.count == 1).then_some(0.0)),
        min_ms: distribution.min_ms,
        p05_ms: distribution.p05_ms,
        median_ms: distribution.p50_ms,
        p95_ms: distribution.p95_ms,
        max_ms: distribution.max_ms,
    }
}

/// Count of every fail kind (TRIAL_RESULT fail_kind 0..=6).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailCounts {
    pub success: usize,
    pub timeout: usize,
    pub not_released: usize,
    pub confirm_reverted: usize,
    pub event_queue_full: usize,
    pub disconnected: usize,
    pub aborted: usize,
    /// fail_kind values outside 0..=6.
    #[serde(default)]
    pub unknown: usize,
}

impl FailCounts {
    pub fn from_array(counts: &[u16; FAIL_KIND_COUNT]) -> Self {
        Self {
            success: usize::from(counts[0]),
            timeout: usize::from(counts[1]),
            not_released: usize::from(counts[2]),
            confirm_reverted: usize::from(counts[3]),
            event_queue_full: usize::from(counts[4]),
            disconnected: usize::from(counts[5]),
            aborted: usize::from(counts[6]),
            unknown: 0,
        }
    }

    pub fn add(&mut self, kind: u8) {
        match kind {
            0 => self.success += 1,
            1 => self.timeout += 1,
            2 => self.not_released += 1,
            3 => self.confirm_reverted += 1,
            4 => self.event_queue_full += 1,
            5 => self.disconnected += 1,
            6 => self.aborted += 1,
            _ => self.unknown += 1,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TesterLatencyStats {
    pub requested: usize,
    pub completed: usize,
    /// Trials whose input was detected and confirmed (fail_kind 0).
    pub succeeded: usize,
    pub valid_latency_count: usize,
    /// succeeded / completed
    pub success_rate: Option<f64>,
    /// valid_latency_count / completed
    pub valid_latency_rate: Option<f64>,
    pub fail_counts: FailCounts,
    /// "run_done" when the counts come from RUN_DONE, "trial_results" when
    /// they were counted from the received TRIAL_RESULT events.
    pub counts_source: String,
    pub trial_results_received: usize,
    /// True when `latency` (or `release_latency`) was computed from the
    /// uncorrected t1_irq - t0_tick because no trial had a valid latency.
    #[serde(default)]
    pub uncalibrated: bool,
    pub latency: LatencyDistribution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_latency: Option<LatencyDistribution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_fail_counts: Option<FailCounts>,
}

pub fn compute_tester_stats(
    trials: &[TrialResultEvent],
    run_done: Option<&RunDoneEvent>,
    requested: usize,
    measure_release: bool,
) -> TesterLatencyStats {
    let press = select_press_latencies(trials);
    let valid_latency_count = trials
        .iter()
        .filter(|trial| trial_latency_ticks(trial).is_some())
        .count();
    let mut counted = FailCounts::default();
    for trial in trials {
        counted.add(trial.fail_kind);
    }

    let (requested, completed, succeeded, fail_counts, source) = match run_done {
        Some(done) => (
            usize::from(done.requested),
            usize::from(done.completed),
            usize::from(done.succeeded),
            FailCounts::from_array(&done.fail_counts),
            "run_done",
        ),
        None => (
            requested,
            trials.len(),
            counted.success,
            counted,
            "trial_results",
        ),
    };
    let rate = |count: usize| (completed > 0).then(|| count as f64 / completed as f64);

    let mut release_uncalibrated = false;
    let (release_latency, release_fail_counts) = if measure_release {
        let release = select_release_latencies(trials);
        release_uncalibrated = release.uncalibrated;
        let mut counts = FailCounts::default();
        for trial in trials.iter().filter(|trial| trial.fail_kind == 0) {
            counts.add(trial.release_fail_kind);
        }
        (Some(distribution(&release.values_ms())), Some(counts))
    } else {
        (None, None)
    };

    TesterLatencyStats {
        requested,
        completed,
        succeeded,
        valid_latency_count,
        success_rate: rate(succeeded),
        valid_latency_rate: rate(valid_latency_count),
        fail_counts,
        counts_source: source.to_string(),
        trial_results_received: trials.len(),
        uncalibrated: press.uncalibrated || release_uncalibrated,
        latency: distribution(&press.values_ms()),
        release_latency,
        release_fail_counts,
    }
}
