//! Tester connections and the operations behind the Tauri commands.
//!
//! One COM port open = one session, so connections
//! are kept open and reused by every command; a connection whose link
//! failed is dropped and reopened (with a new HELLO) on the next use.
//! Every new session writes the stored vendor-code table and calibration
//! to the tester.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use crossbeam_channel::RecvTimeoutError;
use parking_lot::Mutex;
use serde::Serialize;

use crate::models::{AttachedController, AxisThresholdCalibration, SerialDeviceInfo, TesterInfo};

use super::controller::discover_controller;
use super::measure::{
    run_tester_measurement, run_tester_measurement_worker, tester_auto_axis_threshold,
    MeasurementOutcome,
};
use super::model::{
    EnumTraceExport, TesterMeasurementConfig, TesterMeasurementStatus,
    TesterPollStatus, TesterPollTestConfig, TesterPortError, TesterProbeEntry, TesterProbeResult,
    TesterStatusView,
};
use super::objects::read_object;
use super::poll::{record_poll_error, run_tester_poll_test};
use super::protocol::{
    self as proto, EnumRecord, EnumTrace, HelloResponse, TesterEvent,
    CAP_CALIBRATION_MODE, CAP_ENUM_TRACE, CAP_MONITOR, CAP_POLL_TEST, CAP_TRIAL, OBJ_ENUM_TRACE,
};
use super::settings::{dedupe_vendor_entries, SettingsStore};
use super::transport::{crc32_iso_hdlc, ClientOptions, TesterClient, TesterError};

const REENUMERATE_TIMEOUT: Duration = Duration::from_secs(15);

pub type PortOpener = Arc<dyn Fn(&str) -> Result<TesterClient, TesterError> + Send + Sync>;
pub type PortLister = Arc<dyn Fn() -> Vec<SerialDeviceInfo> + Send + Sync>;

pub fn tester_info_from_hello(port_name: &str, hello: &HelloResponse) -> TesterInfo {
    let mut capability_names = Vec::new();
    for (bit, name) in [
        (CAP_MONITOR, "monitor"),
        (CAP_TRIAL, "trial"),
        (CAP_POLL_TEST, "poll_test"),
        (CAP_ENUM_TRACE, "enum_trace"),
        (CAP_CALIBRATION_MODE, "calibration_mode"),
    ] {
        if hello.capabilities & bit != 0 {
            capability_names.push(name.to_string());
        }
    }
    TesterInfo {
        port_name: port_name.to_string(),
        uuid: hello.uuid_hex(),
        fw_major: hello.fw_major,
        fw_minor: hello.fw_minor,
        firmware_version: format!("{}.{}", hello.fw_major, hello.fw_minor),
        build: hello.build_string(),
        protocol_version: hello.protocol_version,
        capabilities: hello.capabilities,
        capability_names,
        max_payload: hello.max_payload,
        max_poll_raw_count: hello.max_poll_raw_count,
        session_id: hello.session_id,
    }
}

/// Merges the tester's vendor table into the stored table (entries learned
/// by the tester during enumeration are kept by the PC).
pub fn pull_vendor_table(client: &TesterClient, settings: &SettingsStore) -> Result<bool> {
    let learned = client.get_vendor_table()?;
    settings.merge_vendor_table(&learned)
}

/// Starts a session: HELLO, then the vendor-code table (merged with what
/// the tester already learned) and the stored calibration are written.
/// Failures after HELLO are returned as warnings.
pub fn start_session(
    client: &TesterClient,
    settings: &SettingsStore,
) -> Result<(TesterInfo, Vec<String>), TesterError> {
    let hello = client.hello()?;
    let info = tester_info_from_hello(client.port_name(), &hello);
    let mut warnings = Vec::new();
    match pull_vendor_table(client, settings) {
        Ok(_) => {
            let table = dedupe_vendor_entries(&settings.vendor_table());
            if let Err(error) = client.set_vendor_table(&table) {
                warnings.push(format!("writing the vendor-code table failed: {error}"));
            }
        }
        // SET replaces the whole table: without the tester's current table
        // the entries it learned would be lost, so nothing is written.
        Err(error) => warnings.push(format!(
            "reading the vendor-code table failed, the stored table was not written: {error:#}"
        )),
    }
    Ok((info, warnings))
}

pub struct TesterConnection {
    pub client: Arc<TesterClient>,
    info: Mutex<TesterInfo>,
    controller: Mutex<Option<AttachedController>>,
    warnings: Mutex<Vec<String>>,
}

impl TesterConnection {
    pub fn new(client: Arc<TesterClient>, info: TesterInfo, warnings: Vec<String>) -> Self {
        Self {
            client,
            info: Mutex::new(info),
            controller: Mutex::new(None),
            warnings: Mutex::new(warnings),
        }
    }

    pub fn info(&self) -> TesterInfo {
        self.info.lock().clone()
    }

    pub fn warnings(&self) -> Vec<String> {
        self.warnings.lock().clone()
    }

    pub fn is_alive(&self) -> bool {
        self.client.is_alive()
    }

    /// Attached controller; descriptors are read again only when the
    /// device generation changed.
    pub fn controller(&self) -> Result<Option<AttachedController>, TesterError> {
        let status = self.client.get_status()?;
        {
            let cached = self.controller.lock();
            if let Some(controller) = cached.as_ref() {
                if controller.device_generation == status.device_generation
                    && status.host_state == 5
                {
                    return Ok(Some(controller.clone()));
                }
            }
        }
        self.refresh_controller()
    }

    pub fn refresh_controller(&self) -> Result<Option<AttachedController>, TesterError> {
        let interval_mode = self
            .client
            .get_host_profile()
            .map(|profile| profile.interval_mode)
            .unwrap_or(0);
        let controller = discover_controller(&self.client, interval_mode)?;
        *self.controller.lock() = controller.clone();
        Ok(controller)
    }

    pub fn require_controller(&self) -> Result<AttachedController> {
        self.controller()?
            .ok_or_else(|| anyhow!("no controller is attached to the tester (NO_DEVICE)"))
    }
}

struct WorkerHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    fn join(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// What the tester path is doing. Measurements, polling tests and axis
/// calibration exclude each other; the state is checked and changed in one
/// critical section before any connection work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Idle,
    StartingMeasurement,
    Measuring,
    StartingPoll,
    Polling,
    Calibrating,
    Reenumerating,
}

impl Operation {
    fn describe(self) -> &'static str {
        match self {
            Self::Idle => "nothing",
            Self::StartingMeasurement | Self::Measuring => "a tester measurement",
            Self::StartingPoll | Self::Polling => "a tester polling test",
            Self::Calibrating => "axis threshold calibration",
            Self::Reenumerating => "re-enumeration",
        }
    }
}

/// Holds a non-idle operation; dropping it (normal end, error, panic)
/// returns the state to `Idle`.
pub struct OperationGuard {
    state: Arc<Mutex<Operation>>,
}

impl OperationGuard {
    pub fn begin(state: &Arc<Mutex<Operation>>, next: Operation) -> Result<Self> {
        let mut current = state.lock();
        if *current != Operation::Idle {
            bail!("{} is already running", current.describe());
        }
        *current = next;
        Ok(Self {
            state: state.clone(),
        })
    }

    pub fn set(&self, next: Operation) {
        *self.state.lock() = next;
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        *self.state.lock() = Operation::Idle;
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| text.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

pub struct TesterApp {
    settings: Arc<SettingsStore>,
    lister: PortLister,
    opener: PortOpener,
    connections: Mutex<BTreeMap<String, Arc<TesterConnection>>>,
    measurement_worker: Mutex<Option<WorkerHandle>>,
    poll_worker: Mutex<Option<WorkerHandle>>,
    operation: Arc<Mutex<Operation>>,
    measurement_status: Arc<Mutex<TesterMeasurementStatus>>,
    poll_status: Arc<Mutex<TesterPollStatus>>,
}

/// Lists the COM ports with the tester's VID:PID (16C0:05E1), without the
/// RP2040 trigger of InputLagScope, which shares the ID.
pub fn serial_port_lister() -> PortLister {
    Arc::new(crate::device_probe::probe_tester_candidate_ports)
}

pub fn serial_port_opener() -> PortOpener {
    Arc::new(|port: &str| TesterClient::open_serial(port, ClientOptions::default()))
}

impl TesterApp {
    pub fn new(settings: Arc<SettingsStore>, lister: PortLister, opener: PortOpener) -> Self {
        Self {
            settings,
            lister,
            opener,
            connections: Mutex::new(BTreeMap::new()),
            measurement_worker: Mutex::new(None),
            poll_worker: Mutex::new(None),
            operation: Arc::new(Mutex::new(Operation::Idle)),
            measurement_status: Arc::new(Mutex::new(TesterMeasurementStatus::idle())),
            poll_status: Arc::new(Mutex::new(TesterPollStatus::idle())),
        }
    }

    pub fn with_serial_ports() -> Self {
        Self::new(
            Arc::new(SettingsStore::open_default()),
            serial_port_lister(),
            serial_port_opener(),
        )
    }

    fn open_connection(&self, port_name: &str) -> Result<Arc<TesterConnection>> {
        let client = Arc::new(
            (self.opener)(port_name).with_context(|| format!("failed to open {port_name}"))?,
        );
        match start_session(&client, &self.settings) {
            Ok((info, warnings)) => Ok(Arc::new(TesterConnection::new(client, info, warnings))),
            Err(error) => {
                client.close();
                Err(anyhow!(error).context(format!("{port_name} did not answer HELLO as a tester")))
            }
        }
    }

    fn live_connection(&self, port_name: &str) -> Option<Arc<TesterConnection>> {
        let mut connections = self.connections.lock();
        match connections.get(port_name) {
            Some(connection) if connection.is_alive() => Some(connection.clone()),
            Some(_) => {
                connections.remove(port_name);
                None
            }
            None => None,
        }
    }

    /// Lists testers: already connected ones are refreshed without a new
    /// session; other candidate ports are opened and greeted with HELLO.
    pub fn probe(&self) -> TesterProbeResult {
        let ports = (self.lister)();
        let port_names: Vec<String> = ports.iter().map(|port| port.port_name.clone()).collect();
        self.connections
            .lock()
            .retain(|name, connection| port_names.contains(name) && connection.is_alive());

        let mut result = TesterProbeResult::default();
        let mut handles = Vec::new();
        for port in port_names {
            if let Some(connection) = self.live_connection(&port) {
                handles.push((port, None, Some(connection)));
                continue;
            }
            let settings = self.settings.clone();
            let opener = self.opener.clone();
            let name = port.clone();
            let handle = thread::spawn(move || -> Result<Arc<TesterConnection>> {
                let client =
                    Arc::new(opener(&name).with_context(|| format!("failed to open {name}"))?);
                match start_session(&client, &settings) {
                    Ok((info, warnings)) => {
                        Ok(Arc::new(TesterConnection::new(client, info, warnings)))
                    }
                    Err(error) => {
                        client.close();
                        Err(anyhow!(error)
                            .context(format!("{name} did not answer HELLO as a tester")))
                    }
                }
            });
            handles.push((port, Some(handle), None));
        }

        for (port, handle, existing) in handles {
            let connection = match (handle, existing) {
                (_, Some(connection)) => connection,
                (Some(handle), None) => match handle.join() {
                    Ok(Ok(connection)) => {
                        self.connections
                            .lock()
                            .insert(port.clone(), connection.clone());
                        connection
                    }
                    Ok(Err(error)) => {
                        result.errors.push(TesterPortError {
                            port_name: port,
                            error: format!("{error:#}"),
                        });
                        continue;
                    }
                    Err(_) => {
                        result.errors.push(TesterPortError {
                            port_name: port,
                            error: "probe thread panicked".to_string(),
                        });
                        continue;
                    }
                },
                (None, None) => continue,
            };
            let mut warnings = connection.warnings();
            if let Some(error) = self.settings.load_error() {
                warnings.push(format!("tester settings were not loaded: {error}"));
            }
            let status = match connection.client.get_status() {
                Ok(status) => Some(TesterStatusView::from(status)),
                Err(error) => {
                    warnings.push(format!("GET_STATUS failed: {error}"));
                    None
                }
            };
            let controller = match connection.controller() {
                Ok(controller) => controller.map(|controller| controller.without_descriptors()),
                Err(error) => {
                    warnings.push(format!("reading the controller failed: {error}"));
                    None
                }
            };
            result.testers.push(TesterProbeEntry {
                tester: connection.info(),
                controller,
                status,
                warnings,
            });
        }
        result
    }

    /// Finds a connected tester by UUID or COM port (case-insensitive), or
    /// opens it. An empty `tester` selects the only connected tester.
    pub fn connect(&self, tester: &str) -> Result<Arc<TesterConnection>> {
        let wanted = tester.trim();
        {
            let mut connections = self.connections.lock();
            connections.retain(|_, connection| connection.is_alive());
            if wanted.is_empty() {
                if connections.len() == 1 {
                    return Ok(connections.values().next().unwrap().clone());
                }
            } else if let Some(connection) = connections.values().find(|connection| {
                let info = connection.info();
                info.port_name.eq_ignore_ascii_case(wanted)
                    || info.uuid.eq_ignore_ascii_case(wanted)
            }) {
                return Ok(connection.clone());
            }
        }

        let ports = (self.lister)();
        if !wanted.is_empty() {
            if let Some(port) = ports
                .iter()
                .find(|port| port.port_name.eq_ignore_ascii_case(wanted))
            {
                let connection = self.open_connection(&port.port_name)?;
                self.connections
                    .lock()
                    .insert(port.port_name.clone(), connection.clone());
                return Ok(connection);
            }
        }
        let probe = self.probe();
        let matches: Vec<_> = probe
            .testers
            .iter()
            .filter(|entry| {
                wanted.is_empty()
                    || entry.tester.uuid.eq_ignore_ascii_case(wanted)
                    || entry.tester.port_name.eq_ignore_ascii_case(wanted)
            })
            .collect();
        match matches.as_slice() {
            [entry] => self
                .live_connection(&entry.tester.port_name)
                .ok_or_else(|| anyhow!("tester on {} is not connected", entry.tester.port_name)),
            [] if wanted.is_empty() => bail!("no tester was found"),
            [] => bail!("tester `{wanted}` was not found"),
            _ => bail!("several testers were found; select one by UUID or COM port"),
        }
    }

    pub fn start_measurement(&self, config: TesterMeasurementConfig) -> Result<()> {
        let guard = OperationGuard::begin(&self.operation, Operation::StartingMeasurement)?;
        let connection = self.connect(&config.tester)?;
        let controller = connection.require_controller()?;
        let mut worker_slot = self.measurement_worker.lock();
        if let Some(mut worker) = worker_slot.take() {
            worker.join();
        }
        {
            let mut status = self.measurement_status.lock();
            *status = TesterMeasurementStatus::idle();
            status.base.running = true;
            status.base.requested_samples = config.sample_count;
            status.phase = "starting".to_string();
            status.tester = Some(connection.info());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let status = self.measurement_status.clone();
        let client = connection.client.clone();
        let tester = connection.info();
        guard.set(Operation::Measuring);
        let join = thread::spawn(move || {
            let _guard = guard;
            let worker_status = status.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_tester_measurement_worker(
                    client,
                    tester,
                    controller,
                    config,
                    worker_status,
                    worker_stop,
                );
            }));
            if let Err(payload) = result {
                let mut status = status.lock();
                status.base.running = false;
                status.phase = "error".to_string();
                status.push_message(format!(
                    "error: measurement worker panicked: {}",
                    panic_message(payload.as_ref())
                ));
            }
        });
        *worker_slot = Some(WorkerHandle {
            stop,
            join: Some(join),
        });
        Ok(())
    }

    pub fn stop_measurement(&self) {
        if let Some(worker) = self.measurement_worker.lock().as_ref() {
            worker.stop.store(true, Ordering::Relaxed);
        }
    }

    pub fn measurement_status(&self) -> TesterMeasurementStatus {
        self.measurement_status.lock().clone()
    }

    /// Runs a measurement on the calling thread (headless CLI).
    pub fn run_measurement_blocking(
        &self,
        config: &TesterMeasurementConfig,
    ) -> Result<(TesterInfo, MeasurementOutcome)> {
        let guard = OperationGuard::begin(&self.operation, Operation::StartingMeasurement)?;
        let connection = self.connect(&config.tester)?;
        let controller = connection.require_controller()?;
        guard.set(Operation::Measuring);
        let stop = Arc::new(AtomicBool::new(false));
        let tester = connection.info();
        let outcome = run_tester_measurement(
            &connection.client,
            &tester,
            &controller,
            config,
            &self.measurement_status,
            &stop,
        );
        if outcome.is_err() {
            self.measurement_status.lock().base.running = false;
        }
        outcome.map(|outcome| (tester, outcome))
    }

    pub fn start_poll_test(&self, config: TesterPollTestConfig) -> Result<()> {
        let guard = OperationGuard::begin(&self.operation, Operation::StartingPoll)?;
        let connection = self.connect(&config.tester)?;
        let controller = connection.require_controller()?;
        let mut worker_slot = self.poll_worker.lock();
        if let Some(mut worker) = worker_slot.take() {
            worker.join();
        }
        {
            let mut status = self.poll_status.lock();
            *status = TesterPollStatus::idle();
            status.base.running = true;
            status.base.requested_duration_seconds = config.duration_seconds;
            status.phase = "starting".to_string();
            status.tester = Some(connection.info());
            status.controller = Some(controller.without_descriptors());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let status = self.poll_status.clone();
        let client = connection.client.clone();
        let tester = connection.info();
        guard.set(Operation::Polling);
        let join = thread::spawn(move || {
            let _guard = guard;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_tester_poll_test(
                    &client,
                    &tester,
                    &controller,
                    &config,
                    &status,
                    &worker_stop,
                )
            }));
            match result {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => record_poll_error(&status, &error),
                Err(payload) => record_poll_error(
                    &status,
                    &anyhow!(
                        "polling worker panicked: {}",
                        panic_message(payload.as_ref())
                    ),
                ),
            }
        });
        *worker_slot = Some(WorkerHandle {
            stop,
            join: Some(join),
        });
        Ok(())
    }

    pub fn stop_poll_test(&self) {
        if let Some(worker) = self.poll_worker.lock().as_ref() {
            worker.stop.store(true, Ordering::Relaxed);
        }
    }

    pub fn poll_status(&self) -> TesterPollStatus {
        self.poll_status.lock().clone()
    }

    pub fn auto_axis_threshold(
        &self,
        config: &TesterMeasurementConfig,
    ) -> Result<AxisThresholdCalibration> {
        let _guard = OperationGuard::begin(&self.operation, Operation::Calibrating)?;
        let connection = self.connect(&config.tester)?;
        let controller = connection.require_controller()?;
        tester_auto_axis_threshold(&connection.client, &controller, config)
    }

    /// REENUMERATE: waits for DEVICE_READY (success) or for an
    /// ENUM_PROGRESS with a non-zero result (failure), then reads the
    /// controller again and keeps what the tester learned in the
    /// vendor-code table.
    /// Runs under the operation state: it is refused while a measurement,
    /// polling test or calibration runs (a new device generation would make
    /// their report monitor drop every later report), and they are refused
    /// while it runs.
    pub fn reenumerate(&self, tester: &str) -> Result<Option<AttachedController>> {
        let _guard = OperationGuard::begin(&self.operation, Operation::Reenumerating)?;
        let connection = self.connect(tester)?;
        let client = &connection.client;
        let events = client.subscribe();
        client.reenumerate()?;
        let deadline = Instant::now() + REENUMERATE_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("the tester did not finish re-enumeration in 15 s");
            }
            match events.recv_timeout(remaining.min(Duration::from_millis(100))) {
                Ok(TesterEvent::DeviceReady(_)) => break,
                Ok(TesterEvent::EnumProgress(progress)) if progress.result != 0 => bail!(
                    "re-enumeration failed at step {} with code {}",
                    progress.step,
                    progress.result
                ),
                Ok(TesterEvent::Error(event)) => bail!(
                    "tester error {} ({}) detail {}",
                    event.code,
                    proto::error_code_name(event.code),
                    event.detail
                ),
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => bail!(
                    "tester connection lost: {}",
                    client.close_reason().unwrap_or_default()
                ),
            }
        }
        if let Err(error) = pull_vendor_table(client, &self.settings) {
            connection
                .warnings
                .lock()
                .push(format!("reading the vendor-code table failed: {error:#}"));
        }
        let controller = connection.refresh_controller()?;
        Ok(controller.map(|controller| controller.without_descriptors()))
    }

    /// Writes the enumeration trace (GET_OBJECT kind 6) as the raw object
    /// (.bin) and as decoded JSON.
    pub fn export_enum_trace(
        &self,
        tester: &str,
        output_dir: &str,
        previous: bool,
    ) -> Result<EnumTraceExport> {
        let connection = self.connect(tester)?;
        let client = &connection.client;
        let status = client.get_status()?;
        let index = u8::from(previous);
        let generation = status
            .device_generation
            .checked_sub(u32::from(index))
            .ok_or_else(|| anyhow!("there is no previous connection generation"))?;
        let data = read_object(client, OBJ_ENUM_TRACE, index, generation)?;
        let info = connection.info();
        let output_dir = PathBuf::from(output_dir);
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let stem = format!("enum_trace_{}_{generation}_{seconds}", info.uuid);
        let bin_path = output_dir.join(format!("{stem}.bin"));
        let json_path = output_dir.join(format!("{stem}.json"));
        fs::write(&bin_path, &data)?;

        let crc32 = crc32_iso_hdlc(&data);
        let (trace, parse_error) = match EnumTrace::decode(&data) {
            Ok(trace) => (Some(trace), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let document = EnumTraceDocument {
            tester: info,
            index,
            source_id: generation,
            byte_length: data.len(),
            crc32,
            parse_error: parse_error.clone(),
            record_version: trace.as_ref().map(|trace| trace.record_version),
            record_count: trace.as_ref().map(|trace| trace.record_count),
            dropped_records: trace.as_ref().map(|trace| trace.dropped_records),
            device_generation: trace.as_ref().and_then(|trace| trace.device_generation),
            flags: trace.as_ref().and_then(|trace| trace.flags),
            open_nak_count: trace.as_ref().and_then(|trace| trace.open_nak_count),
            records: trace
                .as_ref()
                .map(|trace| trace.records.iter().map(EnumRecordView::from).collect())
                .unwrap_or_default(),
        };
        fs::write(&json_path, serde_json::to_string_pretty(&document)?)?;
        Ok(EnumTraceExport {
            json_path: fs::canonicalize(&json_path)
                .unwrap_or(json_path)
                .display()
                .to_string(),
            bin_path: fs::canonicalize(&bin_path)
                .unwrap_or(bin_path)
                .display()
                .to_string(),
            device_generation: generation,
            index,
            byte_length: data.len(),
            crc32,
            record_count: document.records.len(),
            dropped_records: document.dropped_records.unwrap_or(0),
            parse_error,
        })
    }

    /// Closes every connection (tests and shutdown).
    pub fn close_all(&self) {
        let connections: Vec<_> = self.connections.lock().values().cloned().collect();
        for connection in connections {
            connection.client.close();
        }
        self.connections.lock().clear();
    }
}

#[derive(Serialize)]
struct EnumRecordView {
    #[serde(flatten)]
    record: EnumRecord,
    kind_name: &'static str,
    token_name: &'static str,
    result_name: &'static str,
}

impl From<&EnumRecord> for EnumRecordView {
    fn from(record: &EnumRecord) -> Self {
        Self {
            kind_name: record.kind_name(),
            token_name: record.token_name(),
            result_name: record.result_name(),
            record: record.clone(),
        }
    }
}

#[derive(Serialize)]
struct EnumTraceDocument {
    tester: TesterInfo,
    index: u8,
    source_id: u32,
    byte_length: usize,
    crc32: u32,
    parse_error: Option<String>,
    record_version: Option<u16>,
    record_count: Option<u16>,
    dropped_records: Option<u32>,
    device_generation: Option<u32>,
    flags: Option<u8>,
    open_nak_count: Option<u8>,
    records: Vec<EnumRecordView>,
}
