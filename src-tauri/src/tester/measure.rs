//! Latency measurement through the tester.
//!
//! 1. MONITOR_START on the target interface; MONITOR_REPORT events are
//!    decoded into `RawSample`s (timestamp = t1_irq in ns of the tester
//!    clock) tagged with their report group (`detect::TesterSample`).
//! 2. Target detection (button) or axis detection, with the trigger driven
//!    by TRIGGER_MANUAL, following `measurement.rs` step by step but per
//!    report group (`detect.rs`).
//! 3. The watch spec is built from the field location of the detected
//!    input and sent with SET_WATCH; TRIAL_START runs the trials on the
//!    tester, which reports TRIAL_RESULT events and RUN_DONE.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;

use crate::latency_distribution::{latency_bins, latency_series};
use crate::measurement::{
    calibrated_axis_threshold, push_summary_lines, save_result, validate_axis_threshold,
    AXIS_CALIBRATION_MIN_DELTA, RING_CAPACITY,
};
use crate::models::{
    AttachedController, AxisThresholdCalibration, DeviceInfo, MeasurementConfig,
    MeasurementInputType, MeasurementResult, MeasurementTarget, RawSample, ResolvedTarget,
    TesterInfo, TrialResult,
};
use crate::report_layout::{
    ButtonLocation, FieldLocation, PressedRule, ReportDecoder, ReportLayout,
};

use super::controller::select_interface;
use super::detect::{
    collect_axis_means, collect_peak_axis_delta, detect_axis_target, detect_button_target,
    resolve_fixed_button_report, validate_report_selection, wait_for_first_sample, SampleRing,
    TesterSample, TriggerPort,
};
use super::model::{
    TesterMeasurementConfig, TesterMeasurementSection, TesterMeasurementStatus, TesterRunDone,
    TesterTrialConfig, TesterTrialRecord,
};
use super::protocol::{
    completion_reason_name, error_code_name, measurement_kind_name, Calibration, HostProfile,
    MonitorStart, RunDoneEvent, TesterEvent, TrialResultEvent, TrialStart, WatchRule, WatchSpec,
    MAX_REPORT_LENGTH,
};
use super::runctl::{send_stop, unconfirmed_end, RunKind, StopOutcome};
use super::stats::{
    compute_tester_stats, distribution, select_press_latencies, select_release_latencies,
    summary_from_distribution, ticks_to_ms, trial_confirm_latency_ticks, trial_latency_ticks,
    FailCounts, TesterLatencyStats, TICK_NS,
};
use super::transport::{TesterClient, TESTER_BAUD_RATE};

/// MONITOR_START parameters used for detection and calibration: every
/// report (not only changed ones) at most once per millisecond, so that
/// the sample stream matches the one the existing Windows-HID path works on
/// (averages over a window need samples even when nothing changes).
pub const MONITOR_CHANGE_ONLY: u8 = 0;
pub const MONITOR_MIN_INTERVAL_US: u32 = 1000;
const FIRST_SAMPLE_TIMEOUT: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// Monitor stream

/// MONITOR_REPORT events of one interface, decoded into `RawSample`s and
/// tagged with their report group.
pub struct MonitorStream {
    pub rx: Receiver<TesterSample>,
    client: Arc<TesterClient>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    dropped: Arc<AtomicU64>,
    stopped: bool,
}

impl MonitorStream {
    pub fn start(
        client: Arc<TesterClient>,
        iface: u8,
        ep_in: u8,
        device_generation: u32,
        mut decoder: ReportDecoder,
    ) -> Result<Self> {
        let events = client.subscribe();
        let (tx, rx) = bounded::<TesterSample>(RING_CAPACITY);
        let stop = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let protocol = decoder.protocol().to_protocol_kind();
        let thread_stop = stop.clone();
        let thread_dropped = dropped.clone();
        let join = thread::Builder::new()
            .name("tester-monitor".to_string())
            .spawn(move || {
                forward_monitor_reports(
                    events,
                    tx,
                    &thread_stop,
                    &thread_dropped,
                    iface,
                    ep_in,
                    device_generation,
                    &mut decoder,
                    protocol,
                )
            })
            .context("failed to spawn monitor thread")?;
        let stream = Self {
            rx,
            client: client.clone(),
            stop,
            join: Some(join),
            dropped,
            stopped: false,
        };
        client.monitor_start(&MonitorStart {
            iface,
            ep_in,
            change_only: MONITOR_CHANGE_ONLY,
            min_interval_us: MONITOR_MIN_INTERVAL_US,
        })?;
        Ok(stream)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Stops the monitor on the tester and the forwarding thread.
    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        if self.client.is_alive() {
            self.client.monitor_stop()?;
        }
        Ok(())
    }
}

impl Drop for MonitorStream {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn forward_monitor_reports(
    events: Receiver<TesterEvent>,
    tx: Sender<TesterSample>,
    stop: &AtomicBool,
    dropped: &AtomicU64,
    iface: u8,
    ep_in: u8,
    device_generation: u32,
    decoder: &mut ReportDecoder,
    protocol: crate::models::ProtocolKind,
) {
    let mut sequence = 0_u64;
    let mut events_open = true;
    while !stop.load(Ordering::Relaxed) {
        if !events_open {
            // Keep the sender alive so that readers time out normally.
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        match events.recv_timeout(Duration::from_millis(20)) {
            Ok(TesterEvent::MonitorReport(report))
                if report.iface == iface
                    && report.ep_in == ep_in
                    && report.device_generation == device_generation =>
            {
                let report_id = decoder.report_key(&report.data);
                let Some(state) = decoder.decode(&report.data) else {
                    continue;
                };
                let sample = TesterSample {
                    report_id,
                    sample: RawSample {
                        timestamp_ns: report.t1_irq.saturating_mul(TICK_NS),
                        sequence,
                        protocol: protocol.clone(),
                        raw_report: report.data,
                        state,
                    },
                };
                sequence = sequence.wrapping_add(1);
                if tx.try_send(sample).is_err() {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => events_open = false,
        }
    }
}

// ---------------------------------------------------------------------------
// Watch spec

fn watch_base(field: &FieldLocation) -> Result<(u8, u8, u16, u16, u8, bool)> {
    let min_length = field.min_length.max(field.bytes_needed());
    if usize::from(min_length) > MAX_REPORT_LENGTH {
        bail!("report length {min_length} exceeds 1024 bytes");
    }
    let bit_offset = u16::try_from(field.bit_offset)
        .map_err(|_| anyhow!("bit offset {} is too large", field.bit_offset))?;
    if !(1..=32).contains(&field.bit_size) {
        bail!("field width {} is outside 1..=32", field.bit_size);
    }
    let (mode, id) = match field.report_id {
        Some(id) => (1, id),
        None => (0, 0),
    };
    Ok((
        mode,
        id,
        min_length,
        bit_offset,
        field.bit_size,
        field.is_signed,
    ))
}

/// Watch spec for a button (kind 0), or for a multi-bit HID button field
/// an axis rule `|value - 0| >= 1`. `assert_state` false means the input is
/// active when the button reads "released" (complement of the pressed
/// values).
pub fn build_button_watch(
    layout: &ReportLayout,
    index: usize,
    assert_state: bool,
    iface: u8,
    ep_in: u8,
) -> Result<WatchSpec> {
    let location: &ButtonLocation = layout
        .buttons
        .get(index)
        .and_then(Option::as_ref)
        .ok_or_else(|| anyhow!("button {index} has no field location in the report"))?;
    let (report_id_mode, report_id, min_length, bit_offset, bit_size, is_signed) =
        watch_base(&location.field)?;
    let rule = match (&location.pressed, assert_state) {
        (PressedRule::Values { values }, true) => WatchRule::Values {
            values: values.clone(),
        },
        (PressedRule::Values { values }, false) => {
            if bit_size > 6 {
                bail!("cannot invert the pressed values of a {bit_size}-bit field");
            }
            let complement: Vec<u32> = (0..(1_u32 << bit_size))
                .filter(|value| !values.contains(value))
                .collect();
            if complement.is_empty() || complement.len() > 4 {
                bail!(
                    "released values of button {index} do not fit in 4 watch values ({})",
                    complement.len()
                );
            }
            WatchRule::Values { values: complement }
        }
        (PressedRule::NonZero, true) => WatchRule::Axis {
            neutral: 0,
            threshold: 1,
            direction: 0,
        },
        (PressedRule::NonZero, false) => WatchRule::Values { values: vec![0] },
    };
    let watch = WatchSpec {
        iface,
        ep_in,
        report_id_mode,
        report_id,
        min_length,
        bit_offset,
        bit_size,
        is_signed,
        rule,
    };
    watch.validate().map_err(|message| anyhow!(message))?;
    Ok(watch)
}

/// Watch spec for an axis (kind 1). `neutral` and `threshold` are in the
/// normalized units of `InputState`. The boundary is computed in normalized
/// space first and converted to raw units, rounding outward:
///   neutral_raw = round(denorm(neutral))
///   direction +1: boundary = ceil(denorm(neutral + threshold))
///   direction -1: boundary = floor(denorm(neutral - threshold))
///   threshold_raw = max(1, |boundary - neutral_raw|)
/// so that `neutral_raw + direction * threshold_raw` is the first raw value
/// whose normalized delta reaches the threshold. Direction 0 (either side)
/// is only an approximation when the two sides have different scales: the
/// smaller of the two distances is used.
pub fn build_axis_watch(
    layout: &ReportLayout,
    index: usize,
    neutral: f32,
    threshold: f32,
    direction: i8,
    iface: u8,
    ep_in: u8,
) -> Result<WatchSpec> {
    let location = layout
        .axes
        .get(index)
        .and_then(Option::as_ref)
        .ok_or_else(|| anyhow!("axis {index} has no field location in the report"))?;
    let (report_id_mode, report_id, min_length, bit_offset, bit_size, is_signed) =
        watch_base(&location.field)?;
    let (low, high) = if is_signed {
        (-(1_i64 << (bit_size - 1)), (1_i64 << (bit_size - 1)) - 1)
    } else {
        (0, (1_i64 << bit_size) - 1)
    };
    validate_axis_threshold(threshold)?;
    if !neutral.is_finite() {
        bail!("axis neutral {neutral} is not a finite value");
    }
    let normalization = &location.normalization;
    let neutral_value = f64::from(neutral);
    let threshold_value = f64::from(threshold.abs());
    let neutral_raw = (normalization.denormalize(neutral_value).round() as i64)
        .clamp(low, high)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    let upper = || {
        (normalization
            .denormalize(neutral_value + threshold_value)
            .ceil() as i64)
            - neutral_raw
    };
    let lower = || {
        neutral_raw
            - (normalization
                .denormalize(neutral_value - threshold_value)
                .floor() as i64)
    };
    let direction = direction.signum();
    let distance = match direction {
        1 => upper(),
        -1 => lower(),
        _ => upper().min(lower()),
    };
    let threshold_raw = distance.clamp(1, i64::from(u32::MAX)) as u32;
    let neutral_raw = neutral_raw as i32;
    let watch = WatchSpec {
        iface,
        ep_in,
        report_id_mode,
        report_id,
        min_length,
        bit_offset,
        bit_size,
        is_signed,
        rule: WatchRule::Axis {
            neutral: neutral_raw,
            threshold: threshold_raw,
            direction,
        },
    };
    watch.validate().map_err(|message| anyhow!(message))?;
    Ok(watch)
}

// ---------------------------------------------------------------------------
// Measurement

#[derive(Debug)]
pub struct MeasurementOutcome {
    pub output_path: PathBuf,
    pub stats: TesterLatencyStats,
    pub completion_reason: Option<u8>,
    pub run_id: u32,
}

pub(super) fn validate_config(config: &TesterMeasurementConfig) -> Result<()> {
    if !(1..=10_000).contains(&config.sample_count) {
        bail!("sample count must be 1 to 10000");
    }
    for (name, value) in [
        ("timeout_ms", config.timeout_ms),
        ("release_timeout_ms", config.release_timeout_ms),
        ("inter_trial_ms", config.inter_trial_ms()),
    ] {
        if value > u64::from(u16::MAX) {
            bail!("{name} must be at most 65535");
        }
    }
    // The tester does not treat 0 as "default" and
    // answers BAD_PARAM; inter_trial_ms 0 is accepted.
    for (name, value) in [
        ("timeout_ms", config.timeout_ms),
        ("release_timeout_ms", config.release_timeout_ms),
    ] {
        if value == 0 {
            bail!("{name} must be at least 1 (the tester rejects 0; it is not a default)");
        }
    }
    if !(1..=16).contains(&config.settle_reports) {
        bail!("settle_reports must be 1 to 16");
    }
    if config.random_max_us > 10_000_000 {
        bail!("random_max_us must be at most 10000000");
    }
    // Detection compares the signed threshold and the watch uses its
    // magnitude; both agree only for a positive, reachable value.
    if config.input_type == MeasurementInputType::Axis {
        validate_axis_threshold(config.axis_threshold)?;
    }
    Ok(())
}

pub fn device_info_from_controller(
    tester: &TesterInfo,
    controller: &AttachedController,
    iface: u8,
) -> DeviceInfo {
    DeviceInfo {
        id: format!("tester:{}:{}", tester.uuid, iface),
        name: controller.display_name(),
        protocol: controller.protocol.to_protocol_kind(),
        vendor_id: Some(controller.vendor_id),
        product_id: Some(controller.product_id),
        usage_page: None,
        usage: None,
        interface_number: Some(i32::from(iface)),
        manufacturer: controller.manufacturer.clone(),
        serial_number: controller.serial_number.clone(),
        path: None,
        xinput_user_index: None,
        input_report_bytes: None,
        parsed_button_count: controller.parsed_button_count,
        parsed_axis_count: controller.parsed_axis_count,
        report_ids: Vec::new(),
    }
}

/// `latency` is the value the statistics use for this trial
/// (`LatencySelection`); `uncalibrated` tells whether it is the uncorrected
/// t1_irq - t0_tick, in which case the confirmation latency is uncorrected
/// too.
fn trial_result_from_event(
    trial: &TrialResultEvent,
    latency: Option<i64>,
    uncalibrated: bool,
    target: &ResolvedTarget,
    layout: &ReportLayout,
) -> TrialResult {
    let confirm = if uncalibrated {
        (latency.is_some() && trial.confirm_t1_irq != 0)
            .then(|| trial.confirm_t1_irq.wrapping_sub(trial.t0_tick) as i64)
    } else {
        trial_confirm_latency_ticks(trial)
    };
    let detected_index = match target.input_type {
        Some(MeasurementInputType::Axis) => target.axis_index,
        _ => target.button_index,
    };
    let detected_value = match target.input_type {
        Some(MeasurementInputType::Axis) => target
            .axis_index
            .and_then(|index| layout.axes.get(index).cloned().flatten())
            .and_then(|axis| axis.read_normalized(&trial.report)),
        _ => None,
    };
    TrialResult {
        index: usize::from(trial.trial_index),
        latency_ms: latency.map(ticks_to_ms),
        confirm_latency_ms: confirm.map(ticks_to_ms),
        t0_ns: trial.t0_tick.saturating_mul(TICK_NS),
        t0_device_us: None,
        t1_ns: (trial.t1_irq != 0).then(|| trial.t1_irq.saturating_mul(TICK_NS)),
        confirm_t1_ns: (trial.confirm_t1_irq != 0)
            .then(|| trial.confirm_t1_irq.saturating_mul(TICK_NS)),
        detected_input_index: (trial.fail_kind == 0).then_some(detected_index).flatten(),
        detected_value: (trial.fail_kind == 0).then_some(detected_value).flatten(),
        failure: if latency.is_some() {
            None
        } else if trial.fail_kind != 0 {
            Some(super::protocol::fail_kind_name(trial.fail_kind).to_string())
        } else {
            Some("latency_invalid".to_string())
        },
        raw_report: (!trial.report.is_empty()).then(|| trial.report.clone()),
    }
}

fn update_trial_status(
    status: &Arc<Mutex<TesterMeasurementStatus>>,
    trials: &[TrialResultEvent],
    measure_release: bool,
) {
    let press = select_press_latencies(trials);
    let latencies = press.values_ms();
    let valid_latency_count = trials
        .iter()
        .filter(|trial| trial_latency_ticks(trial).is_some())
        .count();
    let mut fail_counts = FailCounts::default();
    for trial in trials {
        fail_counts.add(trial.fail_kind);
    }
    let summary = summary_from_distribution(&distribution(&latencies));
    let release = measure_release.then(|| select_release_latencies(trials));
    let mut status = status.lock();
    status.base.completed_samples = trials.len();
    status.base.failures = trials.len() - latencies.len();
    status.base.summary = summary;
    status.base.latency_bins = latency_bins(&latencies);
    status.base.latency_series = latency_series(&latencies);
    status.valid_latency_count = valid_latency_count;
    status.fail_counts = fail_counts;
    status.uncalibrated =
        press.uncalibrated || release.as_ref().is_some_and(|release| release.uncalibrated);
    if let Some(release) = release {
        status.release_summary = Some(summary_from_distribution(&distribution(
            &release.values_ms(),
        )));
    }
}

pub struct TrialRun {
    pub trials: Vec<TrialResultEvent>,
    pub run_done: Option<RunDoneEvent>,
}

/// Sends SET_WATCH and TRIAL_START and collects TRIAL_RESULT events until
/// RUN_DONE. A stop request sends TRIAL_STOP and still waits for RUN_DONE.
///
/// The run counts as possibly active from the moment TRIAL_START is sent:
/// when its response is lost, TRIAL_STOP is sent with the same run_id and
/// RUN_DONE is awaited; when TRIAL_STOP is not answered, RUN_DONE is still
/// awaited. If the end cannot be confirmed, the connection is closed
/// (`runctl::unconfirmed_end`).
pub fn run_trials(
    client: &TesterClient,
    watch: &WatchSpec,
    request: &TrialStart,
    status: &Arc<Mutex<TesterMeasurementStatus>>,
    stop: &AtomicBool,
) -> Result<TrialRun> {
    let run_id = request.run_id;
    let events = client.subscribe();
    client.set_watch(watch)?;
    let start_error = match client.trial_start(request) {
        Ok(()) => None,
        // The tester answered with a status: the run was not started.
        Err(error) if !error.outcome_unknown() => return Err(error.into()),
        Err(error) => {
            status.lock().push_message(format!(
                "TRIAL_START was not confirmed ({error}); stopping run_id {run_id}"
            ));
            Some(error)
        }
    };
    let measure_release = request.measure_release != 0;
    let idle_limit = Duration::from_millis(
        u64::from(request.release_timeout_ms) * 2
            + u64::from(request.timeout_ms) * 2
            + u64::from(request.inter_trial_ms)
            + u64::from(request.random_max_us / 1000)
            + 5_000,
    );
    let mut trials = Vec::<TrialResultEvent>::with_capacity(usize::from(request.count));
    let mut last_progress = Instant::now();
    let mut stop_sent: Option<(Instant, Duration)> = None;
    loop {
        let stop_requested = stop.load(Ordering::Relaxed);
        if stop_sent.is_none()
            && (start_error.is_some() || stop_requested || last_progress.elapsed() > idle_limit)
        {
            if start_error.is_none() {
                let mut status = status.lock();
                status.stop_requested = true;
                status.push_message(if stop_requested {
                    "stopping trials"
                } else {
                    "no trial progress; sending TRIAL_STOP"
                });
            }
            let outcome = send_stop(client, RunKind::Trial, run_id);
            if let StopOutcome::Unconfirmed(error) = &outcome {
                status.lock().push_message(format!(
                    "TRIAL_STOP was not confirmed ({error}); waiting for RUN_DONE"
                ));
            }
            stop_sent = Some((Instant::now(), outcome.wait()));
        }
        if let Some((sent, wait)) = stop_sent {
            if sent.elapsed() > wait {
                let what = match &start_error {
                    Some(error) => format!(
                        "TRIAL_START was not confirmed ({error}) and RUN_DONE did not arrive"
                    ),
                    None => "RUN_DONE did not arrive after TRIAL_STOP".to_string(),
                };
                return Err(unconfirmed_end(client, run_id, &what));
            }
        }
        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(TesterEvent::TrialResult(trial)) if trial.run_id == run_id => {
                trials.push(trial);
                last_progress = Instant::now();
                update_trial_status(status, &trials, measure_release);
            }
            Ok(TesterEvent::RunDone(done)) if done.run_id == run_id => {
                if let Some(error) = start_error {
                    bail!(
                        "TRIAL_START was not confirmed ({error}); run {run_id} was stopped \
                         (RUN_DONE: {})",
                        completion_reason_name(done.completion_reason)
                    );
                }
                return Ok(TrialRun {
                    trials,
                    run_done: Some(done),
                });
            }
            Ok(TesterEvent::DeviceDetached(event)) => status.lock().push_message(format!(
                "controller detached (generation {})",
                event.device_generation
            )),
            Ok(TesterEvent::Error(event)) => status.lock().push_message(format!(
                "tester error {} ({}) detail {}",
                event.code,
                error_code_name(event.code),
                event.detail
            )),
            Ok(TesterEvent::Recovered(event)) => status.lock().push_message(format!(
                "tester recovered the USB host (cause {}, result {})",
                event.cause, event.result
            )),
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => bail!(
                "tester connection lost: {}",
                client.close_reason().unwrap_or_default()
            ),
        }
    }
}

fn record_error(status: &Arc<Mutex<TesterMeasurementStatus>>, error: &anyhow::Error) {
    let mut status = status.lock();
    status.base.running = false;
    status.phase = "error".to_string();
    status.push_message(format!("error: {error:#}"));
}

/// Worker entry: runs the measurement and writes errors into the status.
pub fn run_tester_measurement_worker(
    client: Arc<TesterClient>,
    tester: TesterInfo,
    controller: AttachedController,
    config: TesterMeasurementConfig,
    status: Arc<Mutex<TesterMeasurementStatus>>,
    stop: Arc<AtomicBool>,
) {
    if let Err(error) =
        run_tester_measurement(&client, &tester, &controller, &config, &status, &stop)
    {
        record_error(&status, &error);
    }
}

pub fn run_tester_measurement(
    client: &Arc<TesterClient>,
    tester: &TesterInfo,
    controller: &AttachedController,
    config: &TesterMeasurementConfig,
    status: &Arc<Mutex<TesterMeasurementStatus>>,
    stop: &Arc<AtomicBool>,
) -> Result<MeasurementOutcome> {
    validate_config(config)?;
    let selected = select_interface(controller, config.interface_number)?;
    let decoder = selected.decoder()?;
    let iface = selected.iface.iface_number;
    let ep_in = selected.iface.ep_in;
    {
        let mut status = status.lock();
        status.base.running = true;
        status.base.requested_samples = config.sample_count;
        status.phase = "monitor".to_string();
        status.tester = Some(tester.clone());
        status.controller = Some(controller.without_descriptors());
        status.push_message(format!(
            "tester {} ({}) controller {} [{}] interface {iface} endpoint 0x{ep_in:02X}",
            tester.uuid,
            tester.port_name,
            controller.display_name(),
            selected.protocol.label()
        ));
    }
    let log = |message: &str| status.lock().push_message(message.to_string());

    let host_profile: Option<HostProfile> = client.get_host_profile().ok();
    let calibration: Option<Calibration> = client.get_calibration().ok();
    if let Some(calibration) = &calibration {
        if !calibration.c0_valid() {
            log("calibration c0 is not valid: trials will have latency_valid = 0");
        }
    }

    // --- detection with the report monitor
    let restrict = config.report_id;
    validate_report_selection(&decoder, restrict)?;
    let mut monitor = MonitorStream::start(
        client.clone(),
        iface,
        ep_in,
        controller.device_generation,
        decoder.clone(),
    )?;
    let mut ring = SampleRing::new();
    wait_for_first_sample(&monitor.rx, &mut ring, FIRST_SAMPLE_TIMEOUT, restrict)?;

    let neutral = if config.input_type == MeasurementInputType::Axis {
        log("sampling neutral axis");
        collect_axis_means(
            &monitor.rx,
            &mut ring,
            Duration::from_millis(config.neutral_sample_ms),
            restrict,
        )
    } else {
        Default::default()
    };
    let trigger = TriggerPort::new(client, config.trigger_mask());
    let (target, layout) = match config.input_type {
        MeasurementInputType::Button if config.button_index >= 0 => {
            let index = config.button_index as usize;
            let report_id = resolve_fixed_button_report(&decoder, index, restrict)?;
            ring.drain(&monitor.rx);
            let layout = match ring.latest_of(report_id) {
                Some(sample) => decoder.layout_for_report(&sample.sample.raw_report),
                None => decoder.layout_for_key(report_id),
            };
            (
                ResolvedTarget {
                    input_type: Some(MeasurementInputType::Button),
                    button_index: Some(index),
                    report_id,
                    ..ResolvedTarget::default()
                },
                layout,
            )
        }
        MeasurementInputType::Button => {
            let (target, sample) = detect_button_target(
                &trigger,
                &monitor.rx,
                &mut ring,
                config.timeout_ms,
                config.retry_delay_ms,
                restrict,
                &log,
                stop,
            )?;
            (target, decoder.layout_for_report(&sample.sample.raw_report))
        }
        MeasurementInputType::Axis => {
            let (target, sample) = detect_axis_target(
                &trigger,
                &monitor.rx,
                &mut ring,
                &neutral,
                config.axis_index,
                config.axis_threshold,
                config.timeout_ms,
                config.retry_delay_ms,
                restrict,
                &log,
                stop,
            )?;
            (target, decoder.layout_for_report(&sample.sample.raw_report))
        }
    };
    drop(trigger);
    let monitor_dropped = monitor.dropped();
    monitor.stop()?;
    drop(monitor);

    if stop.load(Ordering::Relaxed) {
        bail!("measurement stopped before the trials started");
    }

    // --- watch spec
    let neutral_axes = neutral.get(&target.report_id).cloned().unwrap_or_default();
    let watch = match config.input_type {
        MeasurementInputType::Button => {
            let index = target
                .button_index
                .ok_or_else(|| anyhow!("no target button"))?;
            build_button_watch(
                &layout,
                index,
                target.button_assert_state.unwrap_or(true),
                iface,
                ep_in,
            )?
        }
        MeasurementInputType::Axis => {
            let index = target.axis_index.ok_or_else(|| anyhow!("no target axis"))?;
            // The measurement needs a detected side; direction 0 would be
            // approximate (see build_axis_watch).
            let direction = match target.axis_direction {
                Some(direction @ (1 | -1)) => direction,
                other => bail!("the axis direction was not detected ({other:?})"),
            };
            let neutral = neutral_axes.get(index).copied().unwrap_or(0.0);
            build_axis_watch(
                &layout,
                index,
                neutral,
                config.axis_threshold,
                direction,
                iface,
                ep_in,
            )?
        }
    };

    // --- trials
    let run_id = client.next_run_id()?;
    let request = TrialStart {
        run_id,
        trigger_mask: config.trigger_mask(),
        measure_release: u8::from(config.measure_release),
        settle_reports: config.settle_reports,
        count: config.sample_count as u16,
        timeout_ms: config.timeout_ms as u16,
        release_timeout_ms: config.release_timeout_ms as u16,
        inter_trial_ms: config.inter_trial_ms() as u16,
        random_max_us: config.random_max_us,
        seed: config.seed,
    };
    {
        let mut status = status.lock();
        status.phase = "trials".to_string();
        status.run_id = Some(run_id);
        status.target = Some(target.clone());
        status.watch = Some(watch.clone());
        status.push_message(format!(
            "running {} trials (run_id {run_id})",
            config.sample_count
        ));
    }
    let run = run_trials(client, &watch, &request, status, stop)?;
    let calibration = client.get_calibration().ok();

    // --- results
    let stats = compute_tester_stats(
        &run.trials,
        run.run_done.as_ref(),
        config.sample_count,
        config.measure_release,
    );
    let press = select_press_latencies(&run.trials);
    let latencies = press.values_ms();
    let summary = summary_from_distribution(&stats.latency);
    let bins = latency_bins(&latencies);
    let device = device_info_from_controller(tester, controller, iface);
    let measurement_kind = run.trials.first().map_or(0, |trial| trial.measurement_kind);
    let section = TesterMeasurementSection {
        measurement_kind,
        measurement_kind_name: measurement_kind_name(measurement_kind).to_string(),
        tick_ns: TICK_NS,
        run_id,
        tester_uuid: tester.uuid.clone(),
        firmware_version: tester.firmware_version.clone(),
        firmware_build: tester.build.clone(),
        tester: tester.clone(),
        host_profile,
        calibration,
        controller: controller.clone(),
        interface_number: iface,
        ep_in,
        watch: watch.clone(),
        trial_config: TesterTrialConfig {
            trigger_mask: request.trigger_mask,
            measure_release: config.measure_release,
            settle_reports: request.settle_reports,
            count: request.count,
            timeout_ms: request.timeout_ms,
            release_timeout_ms: request.release_timeout_ms,
            inter_trial_ms: request.inter_trial_ms,
            random_max_us: request.random_max_us,
            seed: request.seed,
        },
        run_done: run.run_done.as_ref().map(TesterRunDone::from),
        stats: stats.clone(),
        trials: run.trials.iter().map(TesterTrialRecord::from).collect(),
        monitor_dropped_samples: monitor_dropped,
        uncalibrated: stats.uncalibrated,
    };
    let trials: Vec<TrialResult> = run
        .trials
        .iter()
        .zip(press.ticks.iter())
        .map(|(trial, latency)| {
            trial_result_from_event(trial, *latency, press.uncalibrated, &target, &layout)
        })
        .collect();
    let failures = trials
        .iter()
        .filter(|trial| trial.latency_ms.is_none())
        .count();
    let output_path = save_result(MeasurementResult {
        config: MeasurementConfig {
            device_id: device.id.clone(),
            serial_port: tester.port_name.clone(),
            baud_rate: TESTER_BAUD_RATE,
            sample_count: config.sample_count,
            timeout_ms: config.timeout_ms,
            input_type: config.input_type.clone(),
            button_index: config.button_index,
            axis_index: config.axis_index,
            axis_threshold: config.axis_threshold,
            neutral_sample_ms: config.neutral_sample_ms,
            retry_delay_ms: config.retry_delay_ms,
            output_dir: config.output_dir.clone(),
        },
        device,
        target: target.clone(),
        summary: summary.clone(),
        failures,
        capture_dropped_samples: monitor_dropped,
        latency_bins: bins.clone(),
        trials,
        neutral_axes,
        measurement_target: Some(MeasurementTarget::Tester {
            tester: tester.clone(),
            controller: controller.without_descriptors(),
        }),
        tester: Some(section),
    })?;

    let completion_reason = run.run_done.as_ref().map(|done| done.completion_reason);
    {
        let mut status = status.lock();
        status.base.running = false;
        status.base.summary = summary.clone();
        status.base.failures = failures;
        status.base.dropped_samples = monitor_dropped;
        status.base.latency_bins = bins;
        status.base.latency_series = latency_series(&latencies);
        status.base.output_path = Some(output_path.display().to_string());
        status.stats = Some(stats.clone());
        status.uncalibrated = stats.uncalibrated;
        status.phase = "finished".to_string();
        status.completion_reason =
            completion_reason.map(|reason| completion_reason_name(reason).to_string());
        status.push_message("measurement finished");
        status.push_message(format!("output_path : {}", output_path.display()));
        push_summary_lines(&mut status.base, &summary);
        status.push_message(format!(
            "{:<12} : {}/{}",
            "valid", stats.valid_latency_count, stats.completed
        ));
        if stats.uncalibrated {
            status.push_message(
                "uncalibrated: no trial had a valid latency (calibration c0 missing), so the \
                 statistics use the uncorrected t1_irq - t0_tick",
            );
        }
        if let Some(reason) = &status.completion_reason.clone() {
            status.push_message(format!("{:<12} : {reason}", "completion"));
        }
    }

    Ok(MeasurementOutcome {
        output_path,
        stats,
        completion_reason,
        run_id,
    })
}

/// Axis threshold calibration through the tester, following
/// `measurement.rs::auto_axis_threshold` step by step, with neutral values
/// and deltas per report group.
pub fn tester_auto_axis_threshold(
    client: &Arc<TesterClient>,
    controller: &AttachedController,
    config: &TesterMeasurementConfig,
) -> Result<AxisThresholdCalibration> {
    if config.input_type != MeasurementInputType::Axis {
        bail!("axis threshold calibration requires axis input type");
    }
    let selected = select_interface(controller, config.interface_number)?;
    let decoder = selected.decoder()?;
    let restrict = config.report_id;
    validate_report_selection(&decoder, restrict)?;
    let mut monitor = MonitorStream::start(
        client.clone(),
        selected.iface.iface_number,
        selected.iface.ep_in,
        controller.device_generation,
        decoder,
    )?;
    let trigger = TriggerPort::new(client, config.trigger_mask());
    let result = (|| -> Result<AxisThresholdCalibration> {
        let sample_rx = &monitor.rx;
        let mut ring = SampleRing::new();

        wait_for_first_sample(sample_rx, &mut ring, FIRST_SAMPLE_TIMEOUT, restrict)?;

        trigger.release()?;
        thread::sleep(Duration::from_millis(config.retry_delay_ms));
        ring.drain(sample_rx);

        let neutral = collect_axis_means(
            sample_rx,
            &mut ring,
            Duration::from_millis(config.neutral_sample_ms),
            restrict,
        );
        if neutral.values().all(Vec::is_empty) {
            return Err(anyhow!("no axis samples were observed for calibration"));
        }

        let noise_window = Duration::from_millis(config.retry_delay_ms.clamp(50, 250));
        let noise = collect_peak_axis_delta(
            sample_rx,
            &mut ring,
            &neutral,
            config.axis_index,
            restrict,
            None,
            noise_window,
        )
        .map(|(_, hit)| hit.score)
        .unwrap_or(0.0);

        let t0_ns = trigger.assert()?;
        let active_window = Duration::from_millis(config.timeout_ms.clamp(50, 5000));
        let peak = collect_peak_axis_delta(
            sample_rx,
            &mut ring,
            &neutral,
            config.axis_index,
            restrict,
            Some(t0_ns),
            active_window,
        );
        let Some((report_id, peak)) = peak else {
            return trigger.release_after(Err(anyhow!(
                "no axis movement was observed during calibration"
            )));
        };

        trigger.release()?;
        thread::sleep(Duration::from_millis(config.retry_delay_ms));

        if peak.score < AXIS_CALIBRATION_MIN_DELTA || peak.score <= noise * 1.5 {
            return Err(anyhow!(
                "axis movement was too small for calibration: delta={:.3}, noise={:.3}",
                peak.score,
                noise
            ));
        }

        let threshold = calibrated_axis_threshold(peak.score, noise);
        let neutral_value = neutral
            .get(&report_id)
            .and_then(|axes| axes.get(peak.index))
            .copied()
            .unwrap_or(0.0);
        Ok(AxisThresholdCalibration {
            threshold,
            axis_index: peak.index,
            neutral_value,
            activated_value: peak.value.unwrap_or(neutral_value),
            delta: peak.value.unwrap_or(neutral_value) - neutral_value,
            noise,
            report_id,
        })
    })();
    // Release on every path; a failed release is an error.
    let result = if trigger.is_engaged() {
        trigger.release_after(result)
    } else {
        result
    };
    drop(trigger);
    let stopped = monitor.stop();
    let calibration = result?;
    stopped?;
    Ok(calibration)
}
