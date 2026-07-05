use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{bounded, Receiver};
use parking_lot::Mutex;

use crate::capture::{start_capture, CaptureStats};
use crate::latency_distribution::{latency_bins, latency_series};
use crate::models::{
    AxisThresholdCalibration, DeviceInfo, MeasurementConfig, MeasurementInputType,
    MeasurementResult, MeasurementStatus, RawSample, ResolvedTarget, Summary, TrialResult,
};
use crate::serial_trigger::SerialTrigger;
use crate::stats::summarize;
use crate::thread_tuning::set_current_thread_high_priority;

const RING_CAPACITY: usize = 8192;
const RANDOM_WAIT_MAX_US: u64 = 999;
const AXIS_CALIBRATION_MIN_DELTA: f32 = 0.02;
const AXIS_CALIBRATION_NOISE_MULTIPLIER: f32 = 3.0;
const AXIS_CALIBRATION_DELTA_RATIO: f32 = 0.3;
const AXIS_CALIBRATION_DELTA_CAP: f32 = 0.85;

pub fn auto_axis_threshold(
    config: MeasurementConfig,
    device: DeviceInfo,
) -> Result<AxisThresholdCalibration> {
    if config.input_type != MeasurementInputType::Axis {
        return Err(anyhow!(
            "axis threshold calibration requires axis input type"
        ));
    }

    set_current_thread_high_priority();

    let (sample_tx, sample_rx) = bounded::<RawSample>(RING_CAPACITY);
    let stop = Arc::new(AtomicBool::new(false));
    let capture_stats = Arc::new(CaptureStats::new());
    let capture_handle = start_capture(device, stop.clone(), sample_tx, capture_stats)?;
    let result = (|| -> Result<AxisThresholdCalibration> {
        let mut trigger = SerialTrigger::open(&config.serial_port, config.baud_rate)
            .with_context(|| format!("failed to open {}", config.serial_port))?;
        let mut ring = VecDeque::<RawSample>::with_capacity(RING_CAPACITY);

        wait_for_first_sample(&sample_rx, &mut ring, Duration::from_secs(2))?;

        let _ = trigger.release_gpio2();
        thread::sleep(Duration::from_millis(config.retry_delay_ms));
        drain_samples(&sample_rx, &mut ring);

        let neutral_axes = collect_axis_means(
            &sample_rx,
            &mut ring,
            Duration::from_millis(config.neutral_sample_ms),
        );
        if neutral_axes.is_empty() {
            return Err(anyhow!("no axis samples were observed for calibration"));
        }

        let noise_window = Duration::from_millis(config.retry_delay_ms.max(50).min(250));
        let noise = collect_peak_axis_delta(
            &sample_rx,
            &mut ring,
            &neutral_axes,
            config.axis_index,
            None,
            noise_window,
        )
        .map(|hit| hit.score)
        .unwrap_or(0.0);

        let t0_ns = trigger.assert_gpio2()?;
        let t0_ns = t0_ns.pc_ns;
        let active_window = Duration::from_millis(config.timeout_ms.max(50).min(5000));
        let peak = match collect_peak_axis_delta(
            &sample_rx,
            &mut ring,
            &neutral_axes,
            config.axis_index,
            Some(t0_ns),
            active_window,
        ) {
            Some(hit) => hit,
            None => {
                let _ = trigger.release_gpio2();
                return Err(anyhow!("no axis movement was observed during calibration"));
            }
        };

        let _ = trigger.release_gpio2();
        thread::sleep(Duration::from_millis(config.retry_delay_ms));

        if peak.score < AXIS_CALIBRATION_MIN_DELTA || peak.score <= noise * 1.5 {
            return Err(anyhow!(
                "axis movement was too small for calibration: delta={:.3}, noise={:.3}",
                peak.score,
                noise
            ));
        }

        let threshold = calibrated_axis_threshold(peak.score, noise);
        let neutral_value = neutral_axes.get(peak.index).copied().unwrap_or(0.0);
        Ok(AxisThresholdCalibration {
            threshold,
            axis_index: peak.index,
            neutral_value,
            activated_value: peak.value.unwrap_or(neutral_value),
            delta: peak.value.unwrap_or(neutral_value) - neutral_value,
            noise,
        })
    })();

    stop.store(true, Ordering::Relaxed);
    let _ = capture_handle.join();
    result
}

pub fn run_measurement(
    config: MeasurementConfig,
    device: DeviceInfo,
    status: Arc<Mutex<MeasurementStatus>>,
    stop: Arc<AtomicBool>,
) {
    if let Err(error) = run_measurement_inner(config, device, status.clone(), stop.clone()) {
        let mut status = status.lock();
        status.running = false;
        status.push_message(format!("error: {error:#}"));
    }
}

fn run_measurement_inner(
    config: MeasurementConfig,
    device: DeviceInfo,
    status: Arc<Mutex<MeasurementStatus>>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    set_current_thread_high_priority();

    {
        let mut status = status.lock();
        *status = MeasurementStatus {
            running: true,
            requested_samples: config.sample_count,
            ..MeasurementStatus::default()
        };
        status.push_message(format!("opening {}", device.name));
    }

    let (sample_tx, sample_rx) = bounded::<RawSample>(RING_CAPACITY);
    let capture_stats = Arc::new(CaptureStats::new());
    let capture_handle = start_capture(
        device.clone(),
        stop.clone(),
        sample_tx,
        capture_stats.clone(),
    )?;
    let mut trigger = SerialTrigger::open(&config.serial_port, config.baud_rate)
        .with_context(|| format!("failed to open {}", config.serial_port))?;

    let mut ring = VecDeque::<RawSample>::with_capacity(RING_CAPACITY);
    wait_for_first_sample(&sample_rx, &mut ring, Duration::from_secs(2))?;

    let neutral_axes = if config.input_type == MeasurementInputType::Axis {
        collect_neutral_axes(&sample_rx, &mut ring, config.neutral_sample_ms, &status)
    } else {
        Vec::new()
    };
    let target = resolve_target(
        &config,
        &mut trigger,
        &sample_rx,
        &mut ring,
        &neutral_axes,
        &status,
        &stop,
    )?;

    let mut trials = Vec::with_capacity(config.sample_count);
    let mut latencies = Vec::<f64>::new();
    let mut failures = 0_usize;
    let mut random_wait = RandomWait::new();

    for index in 0..config.sample_count {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let _ = trigger.release_gpio2();
        thread::sleep(Duration::from_millis(config.retry_delay_ms));
        trigger.refresh_device_clock()?;
        drain_samples(&sample_rx, &mut ring);

        let baseline = latest_sample(&ring).cloned();
        let Some(baseline) = baseline else {
            failures += 1;
            trials.push(TrialResult {
                index,
                latency_ms: None,
                confirm_latency_ms: None,
                t0_ns: 0,
                t0_device_us: None,
                t1_ns: None,
                confirm_t1_ns: None,
                detected_input_index: None,
                detected_value: None,
                failure: Some("no baseline sample".to_string()),
                raw_report: None,
            });
            continue;
        };

        random_wait.wait();
        let t0 = trigger.assert_gpio2()?;
        let trial = wait_for_change(
            index,
            t0.pc_ns,
            t0.device_us,
            &baseline,
            &neutral_axes,
            &target,
            &config,
            &sample_rx,
            &mut ring,
            &stop,
        );

        let detected = trial.latency_ms.is_some();
        if let Some(latency_ms) = trial.latency_ms {
            latencies.push(latency_ms);
        } else {
            failures += 1;
        }

        if detected {
            random_wait.wait();
        }
        let _ = trigger.release_gpio2();
        thread::sleep(Duration::from_millis(config.retry_delay_ms));

        trials.push(trial);
        {
            let mut status = status.lock();
            status.completed_samples = trials.len();
            status.failures = failures;
            status.summary = summarize(&latencies);
            status.latency_bins = latency_bins(&latencies);
            status.latency_series = latency_series(&latencies);
            status.dropped_samples = capture_stats.dropped_samples();
        }
    }

    stop.store(true, Ordering::Relaxed);
    let _ = capture_handle.join();

    let summary = summarize(&latencies);
    let bins = latency_bins(&latencies);
    let dropped_samples = capture_stats.dropped_samples();
    let output_path = save_result(MeasurementResult {
        config,
        device,
        target,
        summary: summary.clone(),
        failures,
        capture_dropped_samples: dropped_samples,
        latency_bins: bins.clone(),
        trials,
        neutral_axes,
    })?;

    {
        let mut status = status.lock();
        status.running = false;
        status.summary = summary.clone();
        status.failures = failures;
        status.dropped_samples = dropped_samples;
        status.latency_bins = bins;
        status.latency_series = latency_series(&latencies);
        status.output_path = Some(output_path.display().to_string());
        status.push_message("measurement finished");
        status.push_message(format!("output_path : {}", output_path.display()));
        push_summary_lines(&mut status, &summary);
        status.push_message(format!("{:<12} : {dropped_samples}", "sample_drops"));
    }

    Ok(())
}

fn push_summary_lines(status: &mut MeasurementStatus, summary: &Summary) {
    if summary.count == 0 {
        return;
    }
    let line = |key: &str, value: String| format!("{key:<12} : {value}");
    status.push_message(line("samples", summary.count.to_string()));
    if let Some(value) = summary.average_ms {
        status.push_message(line("average_ms", format!("{value:.6}")));
    }
    if let Some(value) = summary.jitter_ms {
        status.push_message(line("jitter_ms", format!("{value:.6}")));
    }
    if let Some(value) = summary.min_ms {
        status.push_message(line("minimum_ms", format!("{value:.6}")));
    }
    if let Some(value) = summary.p05_ms {
        status.push_message(line("p05_ms", format!("{value:.6}")));
    }
    if let Some(value) = summary.median_ms {
        status.push_message(line("median_ms", format!("{value:.6}")));
    }
    if let Some(value) = summary.p95_ms {
        status.push_message(line("p95_ms", format!("{value:.6}")));
    }
    if let Some(value) = summary.max_ms {
        status.push_message(line("maximum_ms", format!("{value:.6}")));
    }
}

fn resolve_target(
    config: &MeasurementConfig,
    trigger: &mut SerialTrigger,
    sample_rx: &Receiver<RawSample>,
    ring: &mut VecDeque<RawSample>,
    neutral_axes: &[f32],
    status: &Arc<Mutex<MeasurementStatus>>,
    stop: &Arc<AtomicBool>,
) -> Result<ResolvedTarget> {
    match config.input_type {
        MeasurementInputType::Button => {
            if config.button_index >= 0 {
                return Ok(ResolvedTarget {
                    input_type: Some(MeasurementInputType::Button),
                    button_index: Some(config.button_index as usize),
                    ..ResolvedTarget::default()
                });
            }
            detect_button_target(
                trigger,
                sample_rx,
                ring,
                config.timeout_ms,
                config.retry_delay_ms,
                status,
                stop,
            )
        }
        MeasurementInputType::Axis => detect_axis_target(
            trigger,
            sample_rx,
            ring,
            neutral_axes,
            config.axis_index,
            config.axis_threshold,
            config.timeout_ms,
            config.retry_delay_ms,
            status,
            stop,
        ),
    }
}

fn detect_button_target(
    trigger: &mut SerialTrigger,
    sample_rx: &Receiver<RawSample>,
    ring: &mut VecDeque<RawSample>,
    timeout_ms: u64,
    retry_delay_ms: u64,
    status: &Arc<Mutex<MeasurementStatus>>,
    stop: &Arc<AtomicBool>,
) -> Result<ResolvedTarget> {
    {
        let mut status = status.lock();
        status.push_message("detecting target button");
    }

    let _ = trigger.release_gpio2();
    thread::sleep(Duration::from_millis(retry_delay_ms));
    drain_samples(sample_rx, ring);
    let baseline = latest_sample(ring)
        .cloned()
        .ok_or_else(|| anyhow!("no baseline sample for button detection"))?;

    let t0 = trigger.assert_gpio2()?;
    let t0_ns = t0.pc_ns;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        if let Ok(sample) = sample_rx.recv_timeout(Duration::from_millis(1)) {
            let hit = detect_button_hit(&sample, &baseline, None, None);
            push_ring(ring, sample.clone());
            if sample.timestamp_ns >= t0_ns {
                if let Some(hit) = hit {
                    let _ = trigger.release_gpio2();
                    thread::sleep(Duration::from_millis(retry_delay_ms));
                    return Ok(ResolvedTarget {
                        input_type: Some(MeasurementInputType::Button),
                        button_index: Some(hit.index),
                        button_assert_state: Some(hit.button_state.unwrap_or(true)),
                        ..ResolvedTarget::default()
                    });
                }
            }
        }
    }

    Err(anyhow!("target button detection timed out"))
}

#[allow(clippy::too_many_arguments)]
fn detect_axis_target(
    trigger: &mut SerialTrigger,
    sample_rx: &Receiver<RawSample>,
    ring: &mut VecDeque<RawSample>,
    neutral_axes: &[f32],
    axis_index: i32,
    threshold: f32,
    timeout_ms: u64,
    retry_delay_ms: u64,
    status: &Arc<Mutex<MeasurementStatus>>,
    stop: &Arc<AtomicBool>,
) -> Result<ResolvedTarget> {
    {
        let mut status = status.lock();
        status.push_message("detecting target axis");
    }

    let _ = trigger.release_gpio2();
    thread::sleep(Duration::from_millis(retry_delay_ms));
    drain_samples(sample_rx, ring);

    let t0 = trigger.assert_gpio2()?;
    let t0_ns = t0.pc_ns;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        if let Ok(sample) = sample_rx.recv_timeout(Duration::from_millis(1)) {
            let hit = detect_axis_hit(&sample, neutral_axes, axis_index, None, threshold);
            push_ring(ring, sample.clone());
            if sample.timestamp_ns >= t0_ns {
                if let Some(hit) = hit {
                    let _ = trigger.release_gpio2();
                    thread::sleep(Duration::from_millis(retry_delay_ms));
                    return Ok(ResolvedTarget {
                        input_type: Some(MeasurementInputType::Axis),
                        axis_index: Some(hit.index),
                        axis_direction: hit.axis_direction,
                        axis_threshold: Some(threshold),
                        ..ResolvedTarget::default()
                    });
                }
            }
        }
    }

    Err(anyhow!("target axis detection timed out"))
}

fn wait_for_first_sample(
    sample_rx: &Receiver<RawSample>,
    ring: &mut VecDeque<RawSample>,
    timeout: Duration,
) -> Result<()> {
    let sample = sample_rx
        .recv_timeout(timeout)
        .context("input sample did not arrive")?;
    push_ring(ring, sample);
    drain_samples(sample_rx, ring);
    Ok(())
}

fn collect_neutral_axes(
    sample_rx: &Receiver<RawSample>,
    ring: &mut VecDeque<RawSample>,
    duration_ms: u64,
    status: &Arc<Mutex<MeasurementStatus>>,
) -> Vec<f32> {
    {
        let mut status = status.lock();
        status.push_message("sampling neutral axis");
    }

    let deadline = Instant::now() + Duration::from_millis(duration_ms);
    let mut sums = Vec::<f64>::new();
    let mut counts = Vec::<usize>::new();

    while Instant::now() < deadline {
        if let Ok(sample) = sample_rx.recv_timeout(Duration::from_millis(10)) {
            for (index, value) in sample.state.axes.iter().enumerate() {
                if sums.len() <= index {
                    sums.resize(index + 1, 0.0);
                    counts.resize(index + 1, 0);
                }
                sums[index] += f64::from(*value);
                counts[index] += 1;
            }
            push_ring(ring, sample);
        }
    }

    sums.iter()
        .zip(counts.iter())
        .map(|(sum, count)| {
            if *count == 0 {
                0.0
            } else {
                (sum / *count as f64) as f32
            }
        })
        .collect()
}

fn collect_axis_means(
    sample_rx: &Receiver<RawSample>,
    ring: &mut VecDeque<RawSample>,
    duration: Duration,
) -> Vec<f32> {
    let deadline = Instant::now() + duration;
    let mut sums = Vec::<f64>::new();
    let mut counts = Vec::<usize>::new();

    while Instant::now() < deadline {
        if let Ok(sample) = sample_rx.recv_timeout(Duration::from_millis(10)) {
            for (index, value) in sample.state.axes.iter().enumerate() {
                if sums.len() <= index {
                    sums.resize(index + 1, 0.0);
                    counts.resize(index + 1, 0);
                }
                sums[index] += f64::from(*value);
                counts[index] += 1;
            }
            push_ring(ring, sample);
        }
    }

    sums.iter()
        .zip(counts.iter())
        .map(|(sum, count)| {
            if *count == 0 {
                0.0
            } else {
                (sum / *count as f64) as f32
            }
        })
        .collect()
}

fn collect_peak_axis_delta(
    sample_rx: &Receiver<RawSample>,
    ring: &mut VecDeque<RawSample>,
    neutral_axes: &[f32],
    axis_index: i32,
    min_timestamp_ns: Option<u64>,
    duration: Duration,
) -> Option<DetectionHit> {
    let deadline = Instant::now() + duration;
    let mut best = None::<DetectionHit>;

    while Instant::now() < deadline {
        let Ok(sample) = sample_rx.recv_timeout(Duration::from_millis(1)) else {
            continue;
        };
        let hit = min_timestamp_ns
            .map_or(true, |timestamp| sample.timestamp_ns >= timestamp)
            .then(|| peak_axis_delta(&sample, neutral_axes, axis_index))
            .flatten();
        push_ring(ring, sample);
        if let Some(hit) = hit {
            if best
                .as_ref()
                .map_or(true, |current| hit.score > current.score)
            {
                best = Some(hit);
            }
        }
    }

    best
}

fn peak_axis_delta(
    sample: &RawSample,
    neutral_axes: &[f32],
    axis_index: i32,
) -> Option<DetectionHit> {
    if sample.state.axes.is_empty() {
        return None;
    }

    if axis_index >= 0 {
        let index = axis_index as usize;
        let neutral = neutral_axes.get(index).copied().unwrap_or(0.0);
        let value = *sample.state.axes.get(index)?;
        let delta = value - neutral;
        return Some(DetectionHit {
            index,
            value: Some(value),
            button_state: None,
            axis_direction: Some(direction(delta)),
            score: delta.abs(),
        });
    }

    sample
        .state
        .axes
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let neutral = neutral_axes.get(index).copied().unwrap_or(0.0);
            let delta = *value - neutral;
            DetectionHit {
                index,
                value: Some(*value),
                button_state: None,
                axis_direction: Some(direction(delta)),
                score: delta.abs(),
            }
        })
        .max_by(|a, b| a.score.total_cmp(&b.score))
}

fn calibrated_axis_threshold(delta: f32, noise: f32) -> f32 {
    let threshold = (delta * AXIS_CALIBRATION_DELTA_RATIO)
        .max(noise * AXIS_CALIBRATION_NOISE_MULTIPLIER)
        .max(0.01);
    threshold.min(delta * AXIS_CALIBRATION_DELTA_CAP).min(1.0)
}

#[allow(clippy::too_many_arguments)]
fn wait_for_change(
    index: usize,
    t0_ns: u64,
    t0_device_us: u64,
    baseline: &RawSample,
    neutral_axes: &[f32],
    target: &ResolvedTarget,
    config: &MeasurementConfig,
    sample_rx: &Receiver<RawSample>,
    ring: &mut VecDeque<RawSample>,
    stop: &Arc<AtomicBool>,
) -> TrialResult {
    let deadline = Instant::now() + Duration::from_millis(config.timeout_ms);

    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        if let Ok(sample) = sample_rx.recv_timeout(Duration::from_millis(1)) {
            let hit = if sample.timestamp_ns >= t0_ns {
                sample_matches(&sample, baseline, neutral_axes, target, config)
            } else {
                None
            };
            push_ring(ring, sample.clone());
            if let Some(hit) = hit {
                let latency_ms = (sample.timestamp_ns.saturating_sub(t0_ns)) as f64 / 1_000_000.0;
                let confirmation_deadline =
                    deadline.min(Instant::now() + Duration::from_millis(config.retry_delay_ms));
                let confirmation = wait_for_confirmation(
                    t0_ns,
                    baseline,
                    neutral_axes,
                    target,
                    config,
                    sample_rx,
                    ring,
                    stop,
                    confirmation_deadline,
                );
                return TrialResult {
                    index,
                    latency_ms: Some(latency_ms),
                    confirm_latency_ms: confirmation.as_ref().map(|sample| {
                        (sample.timestamp_ns.saturating_sub(t0_ns)) as f64 / 1_000_000.0
                    }),
                    t0_ns,
                    t0_device_us: Some(t0_device_us),
                    t1_ns: Some(sample.timestamp_ns),
                    confirm_t1_ns: confirmation.as_ref().map(|sample| sample.timestamp_ns),
                    detected_input_index: Some(hit.index),
                    detected_value: hit.value,
                    failure: None,
                    raw_report: Some(sample.raw_report),
                };
            }
        }
    }

    TrialResult {
        index,
        latency_ms: None,
        confirm_latency_ms: None,
        t0_ns,
        t0_device_us: Some(t0_device_us),
        t1_ns: None,
        confirm_t1_ns: None,
        detected_input_index: None,
        detected_value: None,
        failure: Some("timeout".to_string()),
        raw_report: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn wait_for_confirmation(
    t0_ns: u64,
    baseline: &RawSample,
    neutral_axes: &[f32],
    target: &ResolvedTarget,
    config: &MeasurementConfig,
    sample_rx: &Receiver<RawSample>,
    ring: &mut VecDeque<RawSample>,
    stop: &Arc<AtomicBool>,
    deadline: Instant,
) -> Option<RawSample> {
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        let Ok(sample) = sample_rx.recv_timeout(Duration::from_millis(1)) else {
            continue;
        };
        let confirmed = sample.timestamp_ns >= t0_ns
            && sample_matches(&sample, baseline, neutral_axes, target, config).is_some();
        push_ring(ring, sample.clone());
        if confirmed {
            return Some(sample);
        }
    }
    None
}

fn sample_matches(
    sample: &RawSample,
    baseline: &RawSample,
    neutral_axes: &[f32],
    target: &ResolvedTarget,
    config: &MeasurementConfig,
) -> Option<DetectionHit> {
    match config.input_type {
        MeasurementInputType::Button => detect_button_hit(
            sample,
            baseline,
            target.button_index,
            target.button_assert_state,
        ),
        MeasurementInputType::Axis => detect_axis_hit(
            sample,
            neutral_axes,
            target
                .axis_index
                .map(|index| index as i32)
                .unwrap_or(config.axis_index),
            target.axis_direction,
            config.axis_threshold,
        ),
    }
}

#[derive(Clone, Debug)]
struct DetectionHit {
    index: usize,
    value: Option<f32>,
    button_state: Option<bool>,
    axis_direction: Option<i8>,
    score: f32,
}

fn detect_button_hit(
    sample: &RawSample,
    baseline: &RawSample,
    button_index: Option<usize>,
    expected_state: Option<bool>,
) -> Option<DetectionHit> {
    if sample.state.buttons.is_empty() || baseline.state.buttons.is_empty() {
        return raw_changed(sample, baseline).then_some(DetectionHit {
            index: 0,
            value: None,
            button_state: None,
            axis_direction: None,
            score: 1.0,
        });
    }

    if let Some(index) = button_index {
        let (current, previous) = sample
            .state
            .buttons
            .get(index)
            .zip(baseline.state.buttons.get(index))?;
        return button_state_matches(*current, *previous, expected_state).then_some(DetectionHit {
            index,
            value: None,
            button_state: Some(*current),
            axis_direction: None,
            score: 1.0,
        });
    }

    sample
        .state
        .buttons
        .iter()
        .zip(baseline.state.buttons.iter())
        .enumerate()
        .find_map(|(index, (current, previous))| {
            button_state_matches(*current, *previous, expected_state).then_some(DetectionHit {
                index,
                value: None,
                button_state: Some(*current),
                axis_direction: None,
                score: 1.0,
            })
        })
}

fn button_state_matches(current: bool, previous: bool, expected_state: Option<bool>) -> bool {
    match expected_state {
        Some(expected) => current == expected && previous != expected,
        None => current != previous,
    }
}

fn detect_axis_hit(
    sample: &RawSample,
    neutral_axes: &[f32],
    axis_index: i32,
    axis_direction: Option<i8>,
    threshold: f32,
) -> Option<DetectionHit> {
    if sample.state.axes.is_empty() {
        return None;
    }

    if axis_index >= 0 {
        let index = axis_index as usize;
        let neutral = neutral_axes.get(index).copied().unwrap_or(0.0);
        let value = *sample.state.axes.get(index)?;
        let delta = value - neutral;
        return axis_delta_matches(delta, axis_direction, threshold).then_some(DetectionHit {
            index,
            value: Some(value),
            button_state: None,
            axis_direction: Some(axis_direction.unwrap_or_else(|| direction(delta))),
            score: delta.abs(),
        });
    }

    sample
        .state
        .axes
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let delta = *value - neutral_axes.get(index).copied().unwrap_or(0.0);
            axis_delta_matches(delta, axis_direction, threshold).then_some(DetectionHit {
                index,
                value: Some(*value),
                button_state: None,
                axis_direction: Some(axis_direction.unwrap_or_else(|| direction(delta))),
                score: delta.abs(),
            })
        })
        .max_by(|a, b| a.score.total_cmp(&b.score))
}

fn axis_delta_matches(delta: f32, axis_direction: Option<i8>, threshold: f32) -> bool {
    match axis_direction {
        Some(direction) if direction > 0 => delta >= threshold,
        Some(direction) if direction < 0 => delta <= -threshold,
        _ => delta.abs() >= threshold,
    }
}

fn direction(delta: f32) -> i8 {
    if delta < 0.0 {
        -1
    } else {
        1
    }
}

fn raw_changed(sample: &RawSample, baseline: &RawSample) -> bool {
    sample.raw_report != baseline.raw_report
}

fn drain_samples(sample_rx: &Receiver<RawSample>, ring: &mut VecDeque<RawSample>) {
    while let Ok(sample) = sample_rx.try_recv() {
        push_ring(ring, sample);
    }
}

fn push_ring(ring: &mut VecDeque<RawSample>, sample: RawSample) {
    if ring.len() >= RING_CAPACITY {
        ring.pop_front();
    }
    ring.push_back(sample);
}

fn latest_sample(ring: &VecDeque<RawSample>) -> Option<&RawSample> {
    ring.back()
}

struct RandomWait {
    state: u64,
}

impl RandomWait {
    fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn wait(&mut self) {
        let wait_us = self.next_wait_us();
        if wait_us == 0 {
            return;
        }

        let deadline = Instant::now() + Duration::from_micros(wait_us);
        while Instant::now() < deadline {
            std::hint::spin_loop();
        }
    }

    fn next_wait_us(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state % (RANDOM_WAIT_MAX_US + 1)
    }
}

fn save_result(result: MeasurementResult) -> Result<PathBuf> {
    let output_dir = PathBuf::from(&result.config.output_dir);
    fs::create_dir_all(&output_dir)?;
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = output_dir.join(format!("result_{seconds}.json"));
    let json = serde_json::to_string_pretty(&result)?;
    fs::write(&path, json)?;
    Ok(fs::canonicalize(&path).unwrap_or(path))
}
