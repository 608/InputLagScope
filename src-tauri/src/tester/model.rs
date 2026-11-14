//! Types exchanged with the GUI (Tauri commands) and written to the result
//! JSON for the tester path.

use serde::{Deserialize, Serialize};

use crate::models::{
    AttachedController, MeasurementInputType, MeasurementStatus, PollTestStatus, ResolvedTarget,
    Summary, TesterInfo,
};

use super::poll::{LongInterval, TesterPollStats};
use super::protocol::{
    c1_status_name, completion_reason_name, fail_kind_name, host_state_name, measurement_kind_name,
    run_kind_name, speed_name, Calibration, HostProfile, PollResultEvent, RunDoneEvent,
    StatusResponse, TrialResultEvent, WatchSpec,
};
use super::stats::{
    ticks_to_ms, ticks_to_us, trial_confirm_latency_ticks, trial_latency_ticks,
    trial_release_latency_ticks, FailCounts, TesterLatencyStats,
};

/// Default wait between tester trials (ms).
pub const DEFAULT_INTER_TRIAL_MS: u64 = 15;

/// Trigger pin of a button measurement: T0 = PA0, wired to the button.
pub const TRIGGER_MASK_BUTTON: u8 = 0x01;
/// Trigger pin of a stick (axis) measurement: T1 = PA1, wired to the stick.
pub const TRIGGER_MASK_STICK: u8 = 0x02;

fn default_settle_reports() -> u8 {
    3
}

fn default_release_timeout_ms() -> u64 {
    1000
}

/// Configuration of a latency measurement through the tester. The fields
/// shared with `MeasurementConfig` keep their meaning.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TesterMeasurementConfig {
    /// Tester UUID or COM port; empty = the only connected tester.
    #[serde(default)]
    pub tester: String,
    pub sample_count: usize,
    /// Time to wait for the change after the trigger (TRIAL_START
    /// timeout_ms, at most 65535) and the detection timeout.
    pub timeout_ms: u64,
    #[serde(default = "default_release_timeout_ms")]
    pub release_timeout_ms: u64,
    pub input_type: MeasurementInputType,
    pub button_index: i32,
    pub axis_index: i32,
    pub axis_threshold: f32,
    pub neutral_sample_ms: u64,
    /// Settle time of target detection (as in the existing path); also the
    /// (`inter_trial_ms` defaults to `DEFAULT_INTER_TRIAL_MS`).
    pub retry_delay_ms: u64,
    #[serde(default)]
    pub inter_trial_ms: Option<u64>,
    pub output_dir: String,
    #[serde(default)]
    pub measure_release: bool,
    #[serde(default = "default_settle_reports")]
    pub settle_reports: u8,
    /// 0 = max(polling period, 1 ms) chosen by the tester.
    #[serde(default)]
    pub random_max_us: u32,
    /// 0 = hardware random number generator.
    #[serde(default)]
    pub seed: u32,
    /// Interface to measure; `None` = chosen from the protocol.
    #[serde(default)]
    pub interface_number: Option<u8>,
    /// Report ID that `button_index` / `axis_index` refer to, and the only
    /// report considered by detection and calibration. Only meaningful for
    /// generic HID controllers whose descriptor uses report IDs (their
    /// buttons and axes are numbered from 0 in every report). `None` =
    /// any report (a fixed button index must then be unambiguous).
    #[serde(default)]
    pub report_id: Option<u8>,
}

impl TesterMeasurementConfig {
    /// Wait between trials. Defaults to `DEFAULT_INTER_TRIAL_MS`: some
    /// controllers (e.g. HyperStrike) delay a press that comes within about
    /// 10 ms of the previous release to suppress double inputs (measured on
    /// the tester: 8 ms between trials gave ~5.1 ms, 10 ms gave ~0.15 ms),
    /// so a shorter wait measures that lockout instead of the input latency.
    pub fn inter_trial_ms(&self) -> u64 {
        self.inter_trial_ms.unwrap_or(DEFAULT_INTER_TRIAL_MS)
    }

    /// Trigger pins (bit n = Tn), fixed by the input type. A `trigger_mask`
    /// key in older saved configurations is ignored.
    pub fn trigger_mask(&self) -> u8 {
        match self.input_type {
            MeasurementInputType::Button => TRIGGER_MASK_BUTTON,
            MeasurementInputType::Axis => TRIGGER_MASK_STICK,
        }
    }
}

/// Parses `"index"` or `"report_id:index"` (report ID in decimal or with a
/// 0x prefix), e.g. `"5"`, `"2:5"`, `"0x02:5"`, `"-1"`.
pub fn parse_report_index(text: &str) -> Result<(Option<u8>, i32), String> {
    let text = text.trim();
    let parse_index = |value: &str| {
        value
            .trim()
            .parse::<i32>()
            .map_err(|error| format!("invalid index `{value}`: {error}"))
    };
    match text.split_once(':') {
        None => Ok((None, parse_index(text)?)),
        Some((report, index)) => {
            let report = report.trim();
            let report_id = match report
                .strip_prefix("0x")
                .or_else(|| report.strip_prefix("0X"))
            {
                Some(hex) => u8::from_str_radix(hex, 16),
                None => report.parse::<u8>(),
            }
            .map_err(|error| format!("invalid report ID `{report}`: {error}"))?;
            let index = parse_index(index)?;
            if index < 0 {
                return Err(format!(
                    "`{text}`: the index after a report ID must be >= 0"
                ));
            }
            Ok((Some(report_id), index))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TesterPollTestConfig {
    #[serde(default)]
    pub tester: String,
    /// 1..=120 seconds (POLL_TEST_START duration_ms 1000..=120000).
    pub duration_seconds: u64,
    pub output_dir: String,
    /// Also fetch the raw intervals (GET_OBJECT kind 7).
    #[serde(default)]
    pub keep_raw: bool,
    /// Raw interval limit; `None` = max_poll_raw_count from HELLO.
    #[serde(default)]
    pub raw_max: Option<u32>,
    #[serde(default)]
    pub interface_number: Option<u8>,
}

/// GET_STATUS with names.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TesterStatusView {
    #[serde(flatten)]
    pub raw: StatusResponse,
    pub host_state_name: String,
    pub speed_name: String,
    pub run_kind_name: String,
}

impl From<StatusResponse> for TesterStatusView {
    fn from(raw: StatusResponse) -> Self {
        Self {
            host_state_name: host_state_name(raw.host_state).to_string(),
            speed_name: speed_name(raw.speed).to_string(),
            run_kind_name: run_kind_name(raw.run_kind).to_string(),
            raw,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TesterProbeEntry {
    pub tester: TesterInfo,
    pub controller: Option<AttachedController>,
    pub status: Option<TesterStatusView>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TesterPortError {
    pub port_name: String,
    pub error: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TesterProbeResult {
    pub testers: Vec<TesterProbeEntry>,
    /// Candidate ports (VID:PID 16C0:05E1) that did not answer HELLO as a
    /// tester.
    pub errors: Vec<TesterPortError>,
}

/// Status of `tester_start_measurement`, polled like `measurement_status`.
/// The fields of `MeasurementStatus` are flattened, so the existing shape
/// is kept and the tester fields are added.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TesterMeasurementStatus {
    #[serde(flatten)]
    pub base: MeasurementStatus,
    pub phase: String,
    pub tester: Option<TesterInfo>,
    pub controller: Option<AttachedController>,
    pub run_id: Option<u32>,
    pub target: Option<ResolvedTarget>,
    pub watch: Option<WatchSpec>,
    pub valid_latency_count: usize,
    pub fail_counts: FailCounts,
    pub stats: Option<TesterLatencyStats>,
    pub release_summary: Option<Summary>,
    pub completion_reason: Option<String>,
    pub stop_requested: bool,
    /// The statistics use the uncorrected t1_irq - t0_tick because no trial
    /// had a valid latency (no calibration yet).
    pub uncalibrated: bool,
}

impl TesterMeasurementStatus {
    pub fn idle() -> Self {
        Self {
            phase: "idle".to_string(),
            ..Self::default()
        }
    }

    pub fn push_message(&mut self, message: impl Into<String>) {
        self.base.push_message(message);
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TesterPollStatus {
    #[serde(flatten)]
    pub base: PollTestStatus,
    pub phase: String,
    pub tester: Option<TesterInfo>,
    pub controller: Option<AttachedController>,
    pub run_id: Option<u32>,
    pub stats: Option<TesterPollStats>,
    pub incomplete: bool,
    pub completion_reason: Option<String>,
}

impl TesterPollStatus {
    pub fn idle() -> Self {
        Self {
            phase: "idle".to_string(),
            ..Self::default()
        }
    }

    pub fn push_message(&mut self, message: impl Into<String>) {
        self.base.push_message(message);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnumTraceExport {
    pub json_path: String,
    pub bin_path: String,
    pub device_generation: u32,
    pub index: u8,
    pub byte_length: usize,
    pub crc32: u32,
    pub record_count: usize,
    pub dropped_records: u32,
    pub parse_error: Option<String>,
}

/// TRIAL_START parameters as sent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TesterTrialConfig {
    pub trigger_mask: u8,
    pub measure_release: bool,
    pub settle_reports: u8,
    pub count: u16,
    pub timeout_ms: u16,
    pub release_timeout_ms: u16,
    pub inter_trial_ms: u16,
    pub random_max_us: u32,
    pub seed: u32,
}

/// One TRIAL_RESULT with every raw field plus the derived latencies.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TesterTrialRecord {
    pub run_id: u32,
    pub trial_index: u16,
    pub fail_kind: u8,
    pub fail_kind_name: String,
    pub release_fail_kind: u8,
    pub release_fail_kind_name: String,
    pub speed: u8,
    pub speed_name: String,
    pub c1_status: u8,
    pub c1_status_name: String,
    /// Upper bound of |c1 error| in ticks; 0xFFFFFFFF = unknown.
    pub c1_uncertainty: u32,
    pub measurement_kind: u8,
    pub measurement_kind_name: String,
    pub valid_flags: u8,
    pub latency_valid: bool,
    pub release_latency_valid: bool,
    pub random_delay_ticks: u32,
    pub t0_sched: u64,
    pub t0_isr: u64,
    pub t0_tick: u64,
    pub t1_irq: u64,
    pub confirm_t1_irq: u64,
    pub c0: i32,
    pub c1: i32,
    pub t0_frame: u16,
    pub t0_uframe: u8,
    pub t1_frame: u16,
    pub t1_uframe: u8,
    pub release_t0_tick: u64,
    pub release_t1_irq: u64,
    /// t1_irq - t0_tick without calibration, when t1_irq != 0.
    pub raw_delta_ticks: Option<i64>,
    pub latency_ticks: Option<i64>,
    pub latency_us: Option<f64>,
    pub latency_ms: Option<f64>,
    pub confirm_latency_ms: Option<f64>,
    pub release_raw_delta_ticks: Option<i64>,
    pub release_latency_ticks: Option<i64>,
    pub release_latency_us: Option<f64>,
    pub release_latency_ms: Option<f64>,
    pub report: Vec<u8>,
}

impl From<&TrialResultEvent> for TesterTrialRecord {
    fn from(trial: &TrialResultEvent) -> Self {
        let latency = trial_latency_ticks(trial);
        let release = trial_release_latency_ticks(trial);
        Self {
            run_id: trial.run_id,
            trial_index: trial.trial_index,
            fail_kind: trial.fail_kind,
            fail_kind_name: fail_kind_name(trial.fail_kind).to_string(),
            release_fail_kind: trial.release_fail_kind,
            release_fail_kind_name: fail_kind_name(trial.release_fail_kind).to_string(),
            speed: trial.speed,
            speed_name: speed_name(trial.speed).to_string(),
            c1_status: trial.c1_status,
            c1_status_name: c1_status_name(trial.c1_status).to_string(),
            c1_uncertainty: trial.c1_uncertainty,
            measurement_kind: trial.measurement_kind,
            measurement_kind_name: measurement_kind_name(trial.measurement_kind).to_string(),
            valid_flags: trial.valid_flags,
            latency_valid: trial.latency_valid(),
            release_latency_valid: trial.release_latency_valid(),
            random_delay_ticks: trial.random_delay_ticks,
            t0_sched: trial.t0_sched,
            t0_isr: trial.t0_isr,
            t0_tick: trial.t0_tick,
            t1_irq: trial.t1_irq,
            confirm_t1_irq: trial.confirm_t1_irq,
            c0: trial.c0,
            c1: trial.c1,
            t0_frame: trial.t0_frame,
            t0_uframe: trial.t0_uframe,
            t1_frame: trial.t1_frame,
            t1_uframe: trial.t1_uframe,
            release_t0_tick: trial.release_t0_tick,
            release_t1_irq: trial.release_t1_irq,
            raw_delta_ticks: (trial.t1_irq != 0)
                .then(|| trial.t1_irq.wrapping_sub(trial.t0_tick) as i64),
            latency_ticks: latency,
            latency_us: latency.map(ticks_to_us),
            latency_ms: latency.map(ticks_to_ms),
            confirm_latency_ms: trial_confirm_latency_ticks(trial).map(ticks_to_ms),
            release_raw_delta_ticks: (trial.release_t1_irq != 0 && trial.release_t0_tick != 0)
                .then(|| trial.release_t1_irq.wrapping_sub(trial.release_t0_tick) as i64),
            release_latency_ticks: release,
            release_latency_us: release.map(ticks_to_us),
            release_latency_ms: release.map(ticks_to_ms),
            report: trial.report.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TesterRunDone {
    pub run_id: u32,
    pub requested: u16,
    pub completed: u16,
    pub succeeded: u16,
    pub valid_latency_count: u16,
    pub fail_counts: FailCounts,
    pub completion_reason: u8,
    pub completion_reason_name: String,
}

impl From<&RunDoneEvent> for TesterRunDone {
    fn from(done: &RunDoneEvent) -> Self {
        Self {
            run_id: done.run_id,
            requested: done.requested,
            completed: done.completed,
            succeeded: done.succeeded,
            valid_latency_count: done.valid_latency_count,
            fail_counts: FailCounts::from_array(&done.fail_counts),
            completion_reason: done.completion_reason,
            completion_reason_name: completion_reason_name(done.completion_reason).to_string(),
        }
    }
}

/// "tester" section of the measurement result JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TesterMeasurementSection {
    /// 0 = latency is the estimate of t1_wire - t0_wire.
    pub measurement_kind: u8,
    pub measurement_kind_name: String,
    pub tick_ns: u64,
    pub run_id: u32,
    pub tester_uuid: String,
    pub firmware_version: String,
    pub firmware_build: String,
    pub tester: TesterInfo,
    pub host_profile: Option<HostProfile>,
    pub calibration: Option<Calibration>,
    pub controller: AttachedController,
    pub interface_number: u8,
    pub ep_in: u8,
    pub watch: WatchSpec,
    pub trial_config: TesterTrialConfig,
    pub run_done: Option<TesterRunDone>,
    pub stats: TesterLatencyStats,
    pub trials: Vec<TesterTrialRecord>,
    pub monitor_dropped_samples: u64,
    /// True when the statistics (and `trials[].latency_ms` of the existing
    /// format) use the uncorrected t1_irq - t0_tick because no trial had a
    /// valid latency; the calibrated `latency_*` of the records stay null.
    pub uncalibrated: bool,
}

/// Non-zero histogram bin (bin i covers i us <= interval < i + 1 us).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistogramBin {
    pub interval_us: u32,
    pub count: u32,
}

/// "tester" section of the polling-test result JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TesterPollSection {
    pub run_id: u32,
    pub completion_reason: u8,
    pub completion_reason_name: String,
    pub tester_uuid: String,
    pub firmware_version: String,
    pub tester: TesterInfo,
    pub host_profile: Option<HostProfile>,
    pub controller: AttachedController,
    pub interface_number: u8,
    pub ep_in: u8,
    pub keep_raw: bool,
    pub raw_max: u32,
    pub tick_ns: u64,
    /// POLL_RESULT as received.
    pub result: PollResultEvent,
    pub stats: TesterPollStats,
    pub histogram: Vec<HistogramBin>,
    pub long_intervals: Vec<LongInterval>,
    /// Raw intervals in ticks (0xFFFFFFFF = 42.9 s or longer), when kept.
    pub raw_intervals_ticks: Option<Vec<u32>>,
}
