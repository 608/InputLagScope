//! Polling-rate test through the tester.
//!
//! The tester aggregates the intervals between IN transactions that
//! returned data; the PC rebuilds the statistics of `polling_rate.rs` from
//! the histogram (1 us bins) and the list of long intervals, on intervals
//! quantized to floor(ticks / 100) us.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use crossbeam_channel::RecvTimeoutError;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::models::{
    AttachedController, PollStall, PollTestConfig, PollTestResult, PollTestSummary, PollTimingBin,
    TesterInfo,
};
use crate::polling_rate::{percentile_sorted, save_result, timing_bins};

use super::controller::select_interface;
use super::model::{HistogramBin, TesterPollSection, TesterPollStatus, TesterPollTestConfig};
use super::objects::read_object;
use super::protocol::{
    completion_reason_name, PollResultEvent, PollTestStart, TesterEvent, OBJ_POLL_HISTOGRAM,
    OBJ_POLL_LONG, OBJ_POLL_RAW,
};
use super::runctl::{send_stop, unconfirmed_end, RunKind, StopOutcome};
use super::stats::TICK_NS;
use super::transport::TesterClient;

/// 50 ms in ticks: intervals at or above it are outside the statistics.
pub const BELOW_50MS_TICKS: u64 = 5_000_000;
/// Width of one histogram bin (1 us).
pub const BIN_WIDTH_TICKS: u64 = 100;
pub const MIN_DURATION_MS: u32 = 1_000;
pub const MAX_DURATION_MS: u32 = 120_000;
const STATUS_UPDATE: Duration = Duration::from_millis(200);

/// Entry of the long-interval list (GET_OBJECT kind 9).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongInterval {
    pub end_time: u64,
    pub interval: u64,
}

pub fn decode_histogram(data: &[u8], bin_count: u32) -> Result<Vec<u32>> {
    if data.len() != bin_count as usize * 4 {
        bail!(
            "histogram object has {} bytes, expected {} (bin_count {bin_count})",
            data.len(),
            bin_count as usize * 4
        );
    }
    Ok(data
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

pub fn decode_long_intervals(data: &[u8], long_count: u32) -> Result<Vec<LongInterval>> {
    if data.len() != long_count as usize * 16 {
        bail!(
            "long-interval object has {} bytes, expected {} (long_count {long_count})",
            data.len(),
            long_count as usize * 16
        );
    }
    Ok(data
        .chunks_exact(16)
        .map(|b| LongInterval {
            end_time: u64::from_le_bytes(b[0..8].try_into().unwrap()),
            interval: u64::from_le_bytes(b[8..16].try_into().unwrap()),
        })
        .collect())
}

pub fn decode_raw_intervals(data: &[u8], raw_count: u32) -> Result<Vec<u32>> {
    if data.len() != raw_count as usize * 4 {
        bail!(
            "raw-interval object has {} bytes, expected {} (raw_count {raw_count})",
            data.len(),
            raw_count as usize * 4
        );
    }
    Ok(data
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TesterPollStats {
    /// Intervals below 50 ms in the rebuilt set (histogram + long intervals
    /// beyond the histogram range).
    pub sample_count: usize,
    pub interval_count: u32,
    pub data_count: u32,
    pub nak_count: u32,
    pub error_count: u32,
    pub duration_ms: u32,
    /// From POLL_RESULT (0 when interval_count is 0), not quantized.
    pub min_interval_us: f64,
    pub max_interval_us: f64,
    pub median_interval_us: Option<f64>,
    pub p95_interval_us: Option<f64>,
    /// Largest interval below 50 ms (as `polling_rate.rs` max_interval_us).
    pub max_below_50ms_us: Option<f64>,
    /// max(4 x median, 1000 us)
    pub stall_threshold_us: Option<f64>,
    pub stall_count: Option<usize>,
    pub stall_total_ms: Option<f64>,
    pub stalls: Option<Vec<PollStall>>,
    pub steady_interval_count: Option<u64>,
    /// (sum_below_50ms - stall sum) / (count_below_50ms - stall count),
    /// not quantized.
    pub steady_mean_interval_us: Option<f64>,
    pub poll_rate_hz: Option<f64>,
    pub long_threshold_us: f64,
    pub poll_period_us: Option<f64>,
    pub first_data_time: u64,
    pub incomplete: bool,
    pub percentiles_incomplete: bool,
    pub stall_values_incomplete: bool,
    pub steady_mean_incomplete: bool,
    pub incomplete_reasons: Vec<String>,
}

pub struct PollReconstruction {
    pub stats: TesterPollStats,
    pub summary: PollTestSummary,
    pub bins: Vec<PollTimingBin>,
    pub stalls: Vec<PollStall>,
}

/// Rebuilds the `polling_rate.rs` statistics:
/// * quantization: floor(ticks / 100) us
/// * interval set: every histogram bin plus the long intervals whose value
///   is >= bin_count x 100 ticks (entries inside the histogram range are
///   not counted twice); only values below 50 ms are used
/// * median / P95: k = round((N - 1) x p / 100) with the same expression as
///   `polling_rate.rs::percentile_sorted`
/// * stall threshold max(4 x median, 1000 us) (only when the median is
///   above 0, as in `polling_rate.rs`), stalls taken from the long-interval
///   list among intervals below 50 ms, time from first_data_time
/// * steady mean from the exact sums (not quantized)
pub fn reconstruct_poll_stats(
    result: &PollResultEvent,
    histogram: &[u32],
    long: &[LongInterval],
    poll_period_us: Option<f64>,
    device_name: Option<String>,
    vid_pid: Option<String>,
) -> PollReconstruction {
    let bin_count = histogram.len() as u64;
    let range_ticks = bin_count * BIN_WIDTH_TICKS;
    let below_50ms_us = BELOW_50MS_TICKS / BIN_WIDTH_TICKS;

    let total: u64 = histogram.iter().map(|count| u64::from(*count)).sum();
    let mut values = Vec::<f64>::with_capacity(total as usize + long.len());
    for (index, count) in histogram.iter().enumerate() {
        let us = index as u64;
        if us < below_50ms_us {
            values.extend(std::iter::repeat(us as f64).take(*count as usize));
        }
    }
    let mut beyond: Vec<u64> = long
        .iter()
        .filter(|entry| entry.interval >= range_ticks && entry.interval < BELOW_50MS_TICKS)
        .map(|entry| entry.interval / BIN_WIDTH_TICKS)
        .collect();
    beyond.sort_unstable();
    values.extend(beyond.iter().map(|us| *us as f64));
    // Already ascending: every value beyond the range is >= bin_count us.
    debug_assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));

    let n = values.len();
    let median = percentile_sorted(&values, 50.0);
    let p95 = percentile_sorted(&values, 95.0);
    // Undefined (null) without intervals and, as in polling_rate.rs,
    // when the median is 0.
    let stall_threshold =
        (result.interval_count > 0 && n > 0 && median > 0.0).then_some((median * 4.0).max(1000.0));

    let mut stall_entries: Vec<&LongInterval> = Vec::new();
    if let Some(threshold) = stall_threshold {
        stall_entries = long
            .iter()
            .filter(|entry| entry.interval < BELOW_50MS_TICKS)
            .filter(|entry| (entry.interval / BIN_WIDTH_TICKS) as f64 >= threshold)
            .collect();
        stall_entries.sort_by_key(|entry| entry.end_time);
    }
    let stalls: Vec<PollStall> = stall_entries
        .iter()
        .map(|entry| PollStall {
            interval_us: (entry.interval / BIN_WIDTH_TICKS) as f64,
            at_ms: entry.end_time.saturating_sub(result.first_data_time) as f64 / 100_000.0,
        })
        .collect();
    let stall_ms = stalls.iter().map(|stall| stall.interval_us).sum::<f64>() / 1000.0;

    let (steady_count, steady_mean) = if stall_threshold.is_some() {
        let stall_ticks: u64 = stall_entries.iter().map(|entry| entry.interval).sum();
        let count = u64::from(result.count_below_50ms).saturating_sub(stall_entries.len() as u64);
        let sum = result.sum_below_50ms.saturating_sub(stall_ticks);
        let mean = (count > 0).then(|| (sum as f64 / BIN_WIDTH_TICKS as f64) / count as f64);
        (Some(count), mean)
    } else {
        (None, None)
    };
    let poll_rate = steady_mean.and_then(|mean| (mean > 0.0).then(|| 1_000_000.0 / mean));

    // Incomplete flags
    let mut reasons = Vec::new();
    let mut percentiles_incomplete = false;
    let mut stall_incomplete = false;
    let mut mean_incomplete = false;
    let no_intervals = result.interval_count == 0 || n == 0;
    if no_intervals {
        reasons.push("no intervals below 50 ms".to_string());
        percentiles_incomplete = true;
        stall_incomplete = true;
        mean_incomplete = true;
    }
    if result.long_truncated != 0 {
        reasons.push("long_truncated: the long-interval list exceeded 4096 entries".to_string());
        percentiles_incomplete = true;
        stall_incomplete = true;
        mean_incomplete = true;
    }
    if !no_intervals {
        if let Some(period) = poll_period_us {
            if median < period / 2.0 {
                reasons.push(format!(
                    "median {median} us is below half of the polling period {period} us"
                ));
                stall_incomplete = true;
                mean_incomplete = true;
            }
        }
        match stall_threshold {
            Some(threshold)
                if threshold * (BIN_WIDTH_TICKS as f64) < f64::from(result.long_threshold) =>
            {
                reasons.push(format!(
                    "stall threshold {threshold} us is below long_threshold {} us",
                    f64::from(result.long_threshold) / BIN_WIDTH_TICKS as f64
                ));
                stall_incomplete = true;
                mean_incomplete = true;
            }
            None => {
                reasons.push("median interval is 0 us".to_string());
                stall_incomplete = true;
                mean_incomplete = true;
            }
            _ => {}
        }
    }
    if steady_mean.is_none() && !mean_incomplete {
        reasons.push("no steady intervals".to_string());
        mean_incomplete = true;
    }
    if steady_mean.is_none() {
        mean_incomplete = true;
    }
    let incomplete = percentiles_incomplete || stall_incomplete || mean_incomplete;

    let stats = TesterPollStats {
        sample_count: n,
        interval_count: result.interval_count,
        data_count: result.data_count,
        nak_count: result.nak_count,
        error_count: result.error_count,
        duration_ms: result.duration_ms,
        min_interval_us: if result.interval_count == 0 {
            0.0
        } else {
            result.min_interval as f64 / BIN_WIDTH_TICKS as f64
        },
        max_interval_us: if result.interval_count == 0 {
            0.0
        } else {
            result.max_interval as f64 / BIN_WIDTH_TICKS as f64
        },
        median_interval_us: (n > 0).then_some(median),
        p95_interval_us: (n > 0).then_some(p95),
        max_below_50ms_us: values.last().copied(),
        stall_threshold_us: stall_threshold,
        stall_count: stall_threshold.map(|_| stalls.len()),
        stall_total_ms: stall_threshold.map(|_| stall_ms),
        stalls: stall_threshold.map(|_| stalls.clone()),
        steady_interval_count: steady_count,
        steady_mean_interval_us: steady_mean,
        poll_rate_hz: poll_rate,
        long_threshold_us: f64::from(result.long_threshold) / BIN_WIDTH_TICKS as f64,
        poll_period_us,
        first_data_time: result.first_data_time,
        incomplete,
        percentiles_incomplete,
        stall_values_incomplete: stall_incomplete,
        steady_mean_incomplete: mean_incomplete,
        incomplete_reasons: reasons,
    };

    let summary = PollTestSummary {
        selected_device_name: device_name,
        selected_vid_pid: vid_pid,
        sample_count: n,
        poll_rate_hz: poll_rate,
        average_interval_us: steady_mean,
        median_interval_us: (n > 0).then_some(median),
        p95_interval_us: (n > 0).then_some(p95),
        max_interval_us: values.last().copied(),
        stall_count: stall_threshold.map(|_| stalls.len()),
        stall_ms: stall_threshold.map(|_| stall_ms),
        stall_threshold_us: stall_threshold,
        capture_duration_ms: Some(f64::from(result.duration_ms)),
    };
    let bins = timing_bins(&values);

    PollReconstruction {
        stats,
        summary,
        bins,
        stalls,
    }
}

pub fn sparse_histogram(histogram: &[u32]) -> Vec<HistogramBin> {
    histogram
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .map(|(index, count)| HistogramBin {
            interval_us: index as u32,
            count: *count,
        })
        .collect()
}

/// Polling period estimate from long_threshold = max(1000 us, 2 x period)
/// when no descriptor information is available.
pub fn period_from_long_threshold(long_threshold: u32) -> Option<f64> {
    (u64::from(long_threshold) > 1000 * BIN_WIDTH_TICKS)
        .then(|| f64::from(long_threshold) / 2.0 / BIN_WIDTH_TICKS as f64)
}

/// Runs a polling test and writes the result JSON (existing
/// `PollTestResult` format plus the "tester" section). Returns the path of
/// the result file.
pub fn run_tester_poll_test(
    client: &Arc<TesterClient>,
    tester: &TesterInfo,
    controller: &AttachedController,
    config: &TesterPollTestConfig,
    status: &Arc<Mutex<TesterPollStatus>>,
    stop: &Arc<AtomicBool>,
) -> Result<PathBuf> {
    let duration_ms = config
        .duration_seconds
        .checked_mul(1000)
        .filter(|ms| (u64::from(MIN_DURATION_MS)..=u64::from(MAX_DURATION_MS)).contains(ms))
        .ok_or_else(|| anyhow!("duration must be 1 to 120 seconds"))? as u32;
    let selected = select_interface(controller, config.interface_number)?;
    let host_profile = client.get_host_profile().ok();
    let interval_mode = host_profile
        .as_ref()
        .map_or(0, |profile| profile.interval_mode);
    let raw_max = if config.keep_raw {
        let requested = config.raw_max.unwrap_or(tester.max_poll_raw_count);
        if requested > tester.max_poll_raw_count {
            bail!(
                "raw_max {requested} exceeds the tester's max_poll_raw_count {}",
                tester.max_poll_raw_count
            );
        }
        requested
    } else {
        0
    };

    let run_id = client.next_run_id()?;
    {
        let mut status = status.lock();
        status.phase = "polling".to_string();
        status.run_id = Some(run_id);
        status.base.requested_duration_seconds = config.duration_seconds;
        status.push_message(format!(
            "polling interface {} endpoint 0x{:02X} for {} s (run_id {run_id})",
            selected.iface.iface_number, selected.iface.ep_in, config.duration_seconds
        ));
    }

    // The run counts as possibly active from the moment POLL_TEST_START is
    // sent; see `runctl`.
    let events = client.subscribe();
    let start_error = match client.poll_test_start(&PollTestStart {
        run_id,
        iface: selected.iface.iface_number,
        ep_in: selected.iface.ep_in,
        keep_raw: u8::from(config.keep_raw),
        duration_ms,
        raw_max,
    }) {
        Ok(()) => None,
        // The tester answered with a status: the run was not started.
        Err(error) if !error.outcome_unknown() => return Err(error.into()),
        Err(error) => {
            status.lock().push_message(format!(
                "POLL_TEST_START was not confirmed ({error}); stopping run_id {run_id}"
            ));
            Some(error)
        }
    };

    let started = Instant::now();
    let watchdog = Duration::from_millis(u64::from(duration_ms)) + Duration::from_secs(5);
    let mut stop_sent: Option<(Instant, Duration)> = None;
    let mut last_status = Instant::now();
    let result = loop {
        if stop_sent.is_none()
            && (start_error.is_some()
                || stop.load(Ordering::Relaxed)
                || started.elapsed() > watchdog)
        {
            if start_error.is_none() && !stop.load(Ordering::Relaxed) {
                status
                    .lock()
                    .push_message("POLL_RESULT is late; sending POLL_TEST_STOP");
            }
            let outcome = send_stop(client, RunKind::Poll, run_id);
            if let StopOutcome::Unconfirmed(error) = &outcome {
                status.lock().push_message(format!(
                    "POLL_TEST_STOP was not confirmed ({error}); waiting for POLL_RESULT"
                ));
            }
            stop_sent = Some((Instant::now(), outcome.wait()));
        }
        if let Some((sent, wait)) = stop_sent {
            if sent.elapsed() > wait {
                let what = match &start_error {
                    Some(error) => format!(
                        "POLL_TEST_START was not confirmed ({error}) and POLL_RESULT did not arrive"
                    ),
                    None => "POLL_RESULT did not arrive after POLL_TEST_STOP".to_string(),
                };
                return Err(unconfirmed_end(client, run_id, &what));
            }
        }
        if last_status.elapsed() >= STATUS_UPDATE {
            status.lock().base.elapsed_ms = started.elapsed().as_millis() as u64;
            last_status = Instant::now();
        }
        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(TesterEvent::PollResult(result)) if result.run_id == run_id => {
                if let Some(error) = start_error {
                    bail!(
                        "POLL_TEST_START was not confirmed ({error}); run {run_id} was stopped                          (POLL_RESULT: {})",
                        completion_reason_name(result.completion_reason)
                    );
                }
                break result;
            }
            Ok(TesterEvent::DeviceDetached(event)) => status.lock().push_message(format!(
                "controller detached (generation {})",
                event.device_generation
            )),
            Ok(TesterEvent::Error(event)) => status.lock().push_message(format!(
                "tester error {} ({}) detail {}",
                event.code,
                super::protocol::error_code_name(event.code),
                event.detail
            )),
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => bail!(
                "tester connection lost: {}",
                client.close_reason().unwrap_or_default()
            ),
        }
    };

    if result.bin_width != BIN_WIDTH_TICKS as u32 {
        bail!("unsupported histogram bin_width {}", result.bin_width);
    }
    status.lock().phase = "reading".to_string();
    let histogram = if result.bin_count > 0 {
        decode_histogram(
            &read_object(client, OBJ_POLL_HISTOGRAM, 0, run_id)?,
            result.bin_count,
        )?
    } else {
        Vec::new()
    };
    let long = if result.long_count > 0 {
        decode_long_intervals(
            &read_object(client, OBJ_POLL_LONG, 0, run_id)?,
            result.long_count,
        )?
    } else {
        Vec::new()
    };
    let raw = if config.keep_raw && result.raw_count > 0 {
        Some(decode_raw_intervals(
            &read_object(client, OBJ_POLL_RAW, 0, run_id)?,
            result.raw_count,
        )?)
    } else {
        None
    };

    let period = super::controller::poll_period_us(
        controller.speed,
        selected.iface.in_interval,
        interval_mode,
    )
    .or_else(|| period_from_long_threshold(result.long_threshold));
    let rebuilt = reconstruct_poll_stats(
        &result,
        &histogram,
        &long,
        period,
        Some(controller.display_name()),
        Some(format!(
            "{:04X}:{:04X}",
            controller.vendor_id, controller.product_id
        )),
    );

    let section = TesterPollSection {
        run_id,
        completion_reason: result.completion_reason,
        completion_reason_name: completion_reason_name(result.completion_reason).to_string(),
        tester_uuid: tester.uuid.clone(),
        firmware_version: tester.firmware_version.clone(),
        tester: tester.clone(),
        host_profile: host_profile.clone(),
        controller: controller.clone(),
        interface_number: selected.iface.iface_number,
        ep_in: selected.iface.ep_in,
        keep_raw: config.keep_raw,
        raw_max,
        tick_ns: TICK_NS,
        result: result.clone(),
        stats: rebuilt.stats.clone(),
        histogram: sparse_histogram(&histogram),
        long_intervals: long,
        raw_intervals_ticks: raw.clone(),
    };
    let intervals_us = raw
        .as_ref()
        .map(|raw| {
            raw.iter()
                .map(|ticks| *ticks as f64 / BIN_WIDTH_TICKS as f64)
                .collect()
        })
        .unwrap_or_default();
    let output_path = save_result(PollTestResult {
        config: PollTestConfig {
            device_id: format!("tester:{}:{}", tester.uuid, selected.iface.iface_number),
            duration_seconds: config.duration_seconds,
            output_dir: config.output_dir.clone(),
        },
        device: None,
        summary: rebuilt.summary.clone(),
        capture_dropped_samples: 0,
        bins: rebuilt.bins.clone(),
        intervals_us,
        stalls: rebuilt.stalls.clone(),
        tester: Some(section),
    })?;

    {
        let mut status = status.lock();
        status.base.running = false;
        status.base.elapsed_ms = u64::from(result.duration_ms);
        status.base.summary = Some(rebuilt.summary.clone());
        status.base.bins = rebuilt.bins.clone();
        status.base.output_path = Some(output_path.display().to_string());
        status.stats = Some(rebuilt.stats.clone());
        status.incomplete = rebuilt.stats.incomplete;
        status.completion_reason =
            Some(completion_reason_name(result.completion_reason).to_string());
        status.phase = "finished".to_string();
        status.push_message("polling rate test finished");
        status.push_message(format!("output_path  : {}", output_path.display()));
        if let Some(rate) = rebuilt.stats.poll_rate_hz {
            status.push_message(format!("poll_rate_hz : {rate:.2}"));
        }
        status.push_message(format!("samples      : {}", rebuilt.stats.sample_count));
        if rebuilt.stats.incomplete {
            status.push_message(format!(
                "incomplete   : {}",
                rebuilt.stats.incomplete_reasons.join("; ")
            ));
        }
    }

    Ok(output_path)
}

/// Writes a failure into the poll status (used by the worker thread).
pub fn record_poll_error(status: &Arc<Mutex<TesterPollStatus>>, error: &anyhow::Error) {
    let mut status = status.lock();
    status.base.running = false;
    status.phase = "error".to_string();
    status.push_message(format!("error: {error:#}"));
}
