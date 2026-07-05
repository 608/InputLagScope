use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolKind {
    Ds4,
    DualSense,
    Switch,
    #[serde(rename = "xinput")]
    XInput,
    GenericHid,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub protocol: ProtocolKind,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub usage_page: Option<u16>,
    pub usage: Option<u16>,
    pub interface_number: Option<i32>,
    pub manufacturer: Option<String>,
    pub serial_number: Option<String>,
    pub path: Option<String>,
    pub xinput_user_index: Option<u32>,
    pub input_report_bytes: Option<usize>,
    pub parsed_button_count: Option<usize>,
    pub parsed_axis_count: Option<usize>,
    pub report_ids: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerialDeviceInfo {
    pub port_name: String,
    pub port_type: String,
    pub display_name: Option<String>,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial_number: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeResult {
    pub devices: Vec<DeviceInfo>,
    pub serial_ports: Vec<SerialDeviceInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementInputType {
    Button,
    Axis,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeasurementConfig {
    pub device_id: String,
    pub serial_port: String,
    pub baud_rate: u32,
    pub sample_count: usize,
    pub timeout_ms: u64,
    pub input_type: MeasurementInputType,
    pub button_index: i32,
    pub axis_index: i32,
    pub axis_threshold: f32,
    pub neutral_sample_ms: u64,
    pub retry_delay_ms: u64,
    pub output_dir: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxisThresholdCalibration {
    pub threshold: f32,
    pub axis_index: usize,
    pub neutral_value: f32,
    pub activated_value: f32,
    pub delta: f32,
    pub noise: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InputState {
    pub buttons: Vec<bool>,
    pub axes: Vec<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawSample {
    pub timestamp_ns: u64,
    pub sequence: u64,
    pub protocol: ProtocolKind,
    pub raw_report: Vec<u8>,
    pub state: InputState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrialResult {
    pub index: usize,
    pub latency_ms: Option<f64>,
    pub confirm_latency_ms: Option<f64>,
    pub t0_ns: u64,
    #[serde(default)]
    pub t0_device_us: Option<u64>,
    pub t1_ns: Option<u64>,
    pub confirm_t1_ns: Option<u64>,
    pub detected_input_index: Option<usize>,
    pub detected_value: Option<f32>,
    pub failure: Option<String>,
    pub raw_report: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Summary {
    pub count: usize,
    pub average_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub min_ms: Option<f64>,
    #[serde(default)]
    pub p05_ms: Option<f64>,
    pub median_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LatencyBin {
    pub label: String,
    pub min_ms: f64,
    pub max_ms: Option<f64>,
    pub count: usize,
    pub percent: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MeasurementStatus {
    pub running: bool,
    pub requested_samples: usize,
    pub completed_samples: usize,
    pub failures: usize,
    #[serde(default)]
    pub dropped_samples: u64,
    pub summary: Summary,
    #[serde(default)]
    pub latency_bins: Vec<LatencyBin>,
    #[serde(default)]
    pub latency_series: Vec<f64>,
    pub output_path: Option<String>,
    pub messages: Vec<String>,
}

impl MeasurementStatus {
    pub fn idle() -> Self {
        Self::default()
    }

    pub fn push_message(&mut self, message: impl Into<String>) {
        let stamped = format!("[{}] {}", log_timestamp(), message.into());
        self.messages.push(stamped);
        if self.messages.len() > 200 {
            let excess = self.messages.len() - 200;
            self.messages.drain(0..excess);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeasurementResult {
    pub config: MeasurementConfig,
    pub device: DeviceInfo,
    pub target: ResolvedTarget,
    pub summary: Summary,
    pub failures: usize,
    pub capture_dropped_samples: u64,
    pub latency_bins: Vec<LatencyBin>,
    pub trials: Vec<TrialResult>,
    pub neutral_axes: Vec<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResolvedTarget {
    pub input_type: Option<MeasurementInputType>,
    pub button_index: Option<usize>,
    pub button_assert_state: Option<bool>,
    pub axis_index: Option<usize>,
    pub axis_direction: Option<i8>,
    pub axis_threshold: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PollTestConfig {
    pub device_id: String,
    pub duration_seconds: u64,
    pub output_dir: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PollTestSummary {
    pub selected_device_name: Option<String>,
    pub selected_vid_pid: Option<String>,
    pub sample_count: usize,
    pub poll_rate_hz: Option<f64>,
    pub average_interval_us: Option<f64>,
    pub median_interval_us: Option<f64>,
    pub p95_interval_us: Option<f64>,
    pub max_interval_us: Option<f64>,
    pub stall_count: usize,
    pub stall_ms: f64,
    pub stall_threshold_us: Option<f64>,
    pub capture_duration_ms: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PollTimingBin {
    pub label: String,
    pub min_us: f64,
    pub max_us: Option<f64>,
    pub count: usize,
    pub percent: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PollStall {
    pub interval_us: f64,
    pub at_ms: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PollTestStatus {
    pub running: bool,
    pub requested_duration_seconds: u64,
    pub elapsed_ms: u64,
    #[serde(default)]
    pub dropped_samples: u64,
    pub summary: Option<PollTestSummary>,
    pub bins: Vec<PollTimingBin>,
    pub output_path: Option<String>,
    pub messages: Vec<String>,
}

impl PollTestStatus {
    pub fn idle() -> Self {
        Self::default()
    }

    pub fn push_message(&mut self, message: impl Into<String>) {
        let stamped = format!("[{}] {}", log_timestamp(), message.into());
        self.messages.push(stamped);
        if self.messages.len() > 200 {
            let excess = self.messages.len() - 200;
            self.messages.drain(0..excess);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PollTestResult {
    pub config: PollTestConfig,
    pub device: Option<DeviceInfo>,
    pub summary: PollTestSummary,
    pub capture_dropped_samples: u64,
    pub bins: Vec<PollTimingBin>,
    pub intervals_us: Vec<f64>,
    pub stalls: Vec<PollStall>,
}

#[cfg(all(windows, not(test)))]
fn log_timestamp() -> String {
    use std::mem::MaybeUninit;
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;

    let mut time = MaybeUninit::<SYSTEMTIME>::zeroed();
    unsafe {
        GetLocalTime(time.as_mut_ptr());
        let time = time.assume_init();
        format!("{:02}:{:02}:{:02}", time.wHour, time.wMinute, time.wSecond)
    }
}

#[cfg(not(all(windows, not(test))))]
fn log_timestamp() -> String {
    "00:00:00".to_string()
}
