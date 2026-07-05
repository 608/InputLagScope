use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, Receiver};
use parking_lot::Mutex;

use crate::capture::{start_capture, CaptureStats};
use crate::models::{
    DeviceInfo, PollStall, PollTestConfig, PollTestResult, PollTestStatus, PollTestSummary,
    PollTimingBin, RawSample,
};
use crate::thread_tuning::set_current_thread_high_priority;

const CHANNEL_CAPACITY: usize = 65536;
const MAX_INTERVAL_US: f64 = 50_000.0;
const STATUS_UPDATE_MS: u128 = 200;

pub fn run_poll_test(
    config: PollTestConfig,
    device: DeviceInfo,
    status: Arc<Mutex<PollTestStatus>>,
    stop: Arc<AtomicBool>,
) {
    if let Err(error) = run_poll_test_inner(config, device, status.clone(), stop.clone()) {
        let mut status = status.lock();
        status.running = false;
        status.push_message(format!("error: {error:#}"));
    }
}

fn run_poll_test_inner(
    config: PollTestConfig,
    device: DeviceInfo,
    status: Arc<Mutex<PollTestStatus>>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    set_current_thread_high_priority();

    {
        let mut status = status.lock();
        *status = PollTestStatus {
            running: true,
            requested_duration_seconds: config.duration_seconds,
            ..PollTestStatus::default()
        };
        status.push_message(format!("opening {}", device.name));
    }

    let (sample_tx, sample_rx) = bounded::<RawSample>(CHANNEL_CAPACITY);
    let capture_stats = Arc::new(CaptureStats::new());
    let capture_handle = start_capture(
        device.clone(),
        stop.clone(),
        sample_tx,
        capture_stats.clone(),
    )?;

    let duration = Duration::from_secs(config.duration_seconds.max(1));
    let started = Instant::now();
    let deadline = started + duration;
    let mut timestamps = Vec::<u64>::new();
    let mut last_status = Instant::now();

    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        collect_samples(&sample_rx, &mut timestamps, Duration::from_millis(20))?;
        if last_status.elapsed().as_millis() >= STATUS_UPDATE_MS {
            update_running_status(
                &status,
                started,
                config.duration_seconds,
                &device,
                &timestamps,
                capture_stats.dropped_samples(),
            );
            last_status = Instant::now();
        }
    }

    drain_samples(&sample_rx, &mut timestamps)?;
    stop.store(true, Ordering::Relaxed);
    let _ = capture_handle.join();

    let capture_duration_ms = started.elapsed().as_secs_f64() * 1000.0;
    let (summary, bins, intervals_us, stalls) =
        summarize_poll_samples(&device, &timestamps, Some(capture_duration_ms));
    let dropped_samples = capture_stats.dropped_samples();

    if intervals_us.is_empty() {
        return Err(anyhow!(
            "not enough input updates to calculate polling rate"
        ));
    }

    let output_path = save_result(PollTestResult {
        config,
        device: Some(device),
        summary: summary.clone(),
        capture_dropped_samples: dropped_samples,
        bins: bins.clone(),
        intervals_us,
        stalls,
    })?;

    {
        let mut status = status.lock();
        status.running = false;
        status.elapsed_ms = capture_duration_ms.round() as u64;
        status.dropped_samples = dropped_samples;
        status.summary = Some(summary.clone());
        status.bins = bins;
        status.output_path = Some(output_path.display().to_string());
        status.push_message("polling rate test finished");
        status.push_message(format!("output_path  : {}", output_path.display()));
        if let Some(rate) = summary.poll_rate_hz {
            status.push_message(format!("poll_rate_hz : {rate:.2}"));
        }
        status.push_message(format!("samples      : {}", summary.sample_count));
        status.push_message(format!("sample_drops : {dropped_samples}"));
    }

    Ok(())
}

fn collect_samples(
    sample_rx: &Receiver<RawSample>,
    timestamps: &mut Vec<u64>,
    timeout: Duration,
) -> Result<()> {
    match sample_rx.recv_timeout(timeout) {
        Ok(sample) => push_sample_timestamp(sample, timestamps),
        Err(crossbeam_channel::RecvTimeoutError::Timeout) => Ok(()),
        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
            Err(anyhow!("capture stopped unexpectedly"))
        }
    }?;
    drain_samples(sample_rx, timestamps)
}

fn drain_samples(sample_rx: &Receiver<RawSample>, timestamps: &mut Vec<u64>) -> Result<()> {
    while let Ok(sample) = sample_rx.try_recv() {
        push_sample_timestamp(sample, timestamps)?;
    }
    Ok(())
}

fn push_sample_timestamp(sample: RawSample, timestamps: &mut Vec<u64>) -> Result<()> {
    if sample.protocol == crate::models::ProtocolKind::Unknown
        && sample.raw_report.starts_with(b"capture error:")
    {
        let message = String::from_utf8_lossy(&sample.raw_report).to_string();
        return Err(anyhow!(message));
    }

    if timestamps.last().copied() != Some(sample.timestamp_ns) {
        timestamps.push(sample.timestamp_ns);
    }
    Ok(())
}

fn update_running_status(
    status: &Arc<Mutex<PollTestStatus>>,
    started: Instant,
    duration_seconds: u64,
    device: &DeviceInfo,
    timestamps: &[u64],
    dropped_samples: u64,
) {
    let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let (summary, bins, _, _) = summarize_poll_samples(device, timestamps, None);
    let mut status = status.lock();
    status.elapsed_ms = elapsed_ms;
    status.requested_duration_seconds = duration_seconds;
    status.dropped_samples = dropped_samples;
    status.summary = Some(summary);
    status.bins = bins;
}

fn summarize_poll_samples(
    device: &DeviceInfo,
    timestamps: &[u64],
    capture_duration_ms: Option<f64>,
) -> (
    PollTestSummary,
    Vec<PollTimingBin>,
    Vec<f64>,
    Vec<PollStall>,
) {
    let mut intervals_us = Vec::<f64>::new();
    for pair in timestamps.windows(2) {
        let interval_us = pair[1].saturating_sub(pair[0]) as f64 / 1000.0;
        if interval_us >= 0.0 && interval_us < MAX_INTERVAL_US {
            intervals_us.push(interval_us);
        }
    }

    let mut sorted = intervals_us.clone();
    sorted.sort_by(f64::total_cmp);

    let median = percentile_sorted(&sorted, 50.0);
    let stall_threshold_us = (median > 0.0).then_some((median * 4.0).max(1000.0));
    let mut stalls = Vec::<PollStall>::new();
    let mut steady = Vec::<f64>::new();

    if let Some(threshold) = stall_threshold_us {
        for pair in timestamps.windows(2) {
            let interval_us = pair[1].saturating_sub(pair[0]) as f64 / 1000.0;
            if interval_us >= MAX_INTERVAL_US {
                continue;
            }
            if interval_us >= threshold {
                let at_ms = pair[1].saturating_sub(timestamps[0]) as f64 / 1_000_000.0;
                stalls.push(PollStall { interval_us, at_ms });
            } else {
                steady.push(interval_us);
            }
        }
    }

    let average_interval_us = (!steady.is_empty()).then(|| average(&steady));
    let poll_rate_hz = average_interval_us.and_then(|value| {
        if value > 0.0 {
            Some(1_000_000.0 / value)
        } else {
            None
        }
    });

    let summary = PollTestSummary {
        selected_device_name: Some(device.name.clone()),
        selected_vid_pid: vid_pid(device),
        sample_count: intervals_us.len(),
        poll_rate_hz,
        average_interval_us,
        median_interval_us: (!sorted.is_empty()).then(|| percentile_sorted(&sorted, 50.0)),
        p95_interval_us: (!sorted.is_empty()).then(|| percentile_sorted(&sorted, 95.0)),
        max_interval_us: sorted.last().copied(),
        stall_count: stalls.len(),
        stall_ms: stalls.iter().map(|stall| stall.interval_us).sum::<f64>() / 1000.0,
        stall_threshold_us,
        capture_duration_ms,
    };

    let bins = timing_bins(&intervals_us);
    (summary, bins, intervals_us, stalls)
}

fn timing_bins(intervals: &[f64]) -> Vec<PollTimingBin> {
    if intervals.is_empty() {
        return Vec::new();
    }

    let mut sorted = intervals.to_vec();
    sorted.sort_by(f64::total_cmp);
    let p5 = percentile_sorted(&sorted, 5.0);
    let p95 = percentile_sorted(&sorted, 95.0);
    let mut bin_width = (p95 - p5) / 6.0;
    if !bin_width.is_finite() || bin_width < 1.0 {
        bin_width = 1.0;
    }

    let mut ranges = Vec::<(String, f64, Option<f64>)>::new();
    let fastest_hz = rate_label_hz(p5);
    ranges.push((format!(">{fastest_hz}"), 0.0, Some(p5)));
    for index in 0..6 {
        let min_us = p5 + index as f64 * bin_width;
        let max_us = p5 + (index + 1) as f64 * bin_width;
        let min_hz = rate_label_hz(max_us);
        let max_hz = rate_label_hz(min_us);
        ranges.push((format!("{min_hz}-{max_hz}"), min_us, Some(max_us)));
    }
    let slowest_hz = rate_label_hz(p95);
    ranges.push((format!("<{slowest_hz}"), p95, None));

    ranges
        .into_iter()
        .map(|(label, min_us, max_us)| {
            let count = intervals
                .iter()
                .filter(|value| {
                    **value >= min_us && max_us.map(|max| **value < max).unwrap_or(true)
                })
                .count();
            PollTimingBin {
                label,
                min_us,
                max_us,
                count,
                percent: 100.0 * count as f64 / intervals.len() as f64,
            }
        })
        .collect()
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn average(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn rate_label_hz(interval_us: f64) -> u64 {
    if interval_us <= 0.0 {
        0
    } else {
        (1_000_000.0 / interval_us).round().max(0.0) as u64
    }
}

fn vid_pid(device: &DeviceInfo) -> Option<String> {
    Some(format!(
        "{:04X}:{:04X}",
        device.vendor_id?, device.product_id?
    ))
}

fn save_result(result: PollTestResult) -> Result<PathBuf> {
    let output_dir = PathBuf::from(&result.config.output_dir);
    fs::create_dir_all(&output_dir)?;
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = output_dir.join(format!("poll_result_{seconds}.json"));
    let json = serde_json::to_string_pretty(&result)?;
    fs::write(&path, json)?;
    Ok(fs::canonicalize(&path).unwrap_or(path))
}
