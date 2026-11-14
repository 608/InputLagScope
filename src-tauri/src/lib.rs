#![cfg_attr(test, allow(dead_code))]

mod capture;
mod clock;
mod device_probe;
mod headless;
mod hid_descriptor;
mod latency_distribution;
mod measurement;
mod models;
mod polling_rate;
mod protocol;
mod report_layout;
mod serial_trigger;
mod stats;
mod tester;
mod thread_tuning;
mod windows_hid;
mod xinput;

pub use headless::run_headless_cli;

#[cfg(not(test))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(test))]
use std::sync::Arc;
#[cfg(not(test))]
use std::thread::JoinHandle;

#[cfg(not(test))]
use models::{
    AttachedController, AxisThresholdCalibration, MeasurementConfig, MeasurementStatus,
    PollTestConfig, PollTestStatus, ProbeResult,
};
#[cfg(not(test))]
use parking_lot::Mutex;
#[cfg(not(test))]
use tauri::State;
#[cfg(not(test))]
use tester::app::TesterApp;
#[cfg(not(test))]
use tester::model::{
    EnumTraceExport, TesterMeasurementConfig, TesterMeasurementStatus,
    TesterPollStatus, TesterPollTestConfig, TesterProbeResult,
};

#[cfg(not(test))]
struct WorkerHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

#[cfg(not(test))]
struct AppState {
    worker: Mutex<Option<WorkerHandle>>,
    poll_worker: Mutex<Option<WorkerHandle>>,
    axis_calibration_running: Mutex<bool>,
    status: Arc<Mutex<MeasurementStatus>>,
    poll_status: Arc<Mutex<PollTestStatus>>,
    last_probe: Arc<Mutex<Option<ProbeResult>>>,
    tester: Arc<TesterApp>,
}

#[cfg(not(test))]
impl Default for AppState {
    fn default() -> Self {
        Self {
            worker: Mutex::new(None),
            poll_worker: Mutex::new(None),
            axis_calibration_running: Mutex::new(false),
            status: Arc::new(Mutex::new(MeasurementStatus::idle())),
            poll_status: Arc::new(Mutex::new(PollTestStatus::idle())),
            last_probe: Arc::new(Mutex::new(None)),
            tester: Arc::new(TesterApp::with_serial_ports()),
        }
    }
}

// ---------------------------------------------------------------------------
// CH32H417 tester commands (separate from the Windows-HID path below)

#[cfg(not(test))]
async fn run_tester_blocking<T, F>(state: &State<'_, AppState>, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(Arc<TesterApp>) -> anyhow::Result<T> + Send + 'static,
{
    let app = state.tester.clone();
    tauri::async_runtime::spawn_blocking(move || task(app).map_err(|error| format!("{error:#}")))
        .await
        .map_err(|error| format!("tester task failed: {error}"))?
}

#[cfg(not(test))]
#[tauri::command]
async fn tester_probe(state: State<'_, AppState>) -> Result<TesterProbeResult, String> {
    run_tester_blocking(&state, |app| Ok(app.probe())).await
}

#[cfg(not(test))]
#[tauri::command]
async fn tester_start_measurement(
    state: State<'_, AppState>,
    config: TesterMeasurementConfig,
) -> Result<(), String> {
    run_tester_blocking(&state, move |app| app.start_measurement(config)).await
}

#[cfg(not(test))]
#[tauri::command]
fn tester_stop_measurement(state: State<'_, AppState>) -> Result<(), String> {
    state.tester.stop_measurement();
    Ok(())
}

#[cfg(not(test))]
#[tauri::command]
fn tester_measurement_status(
    state: State<'_, AppState>,
) -> Result<TesterMeasurementStatus, String> {
    Ok(state.tester.measurement_status())
}

#[cfg(not(test))]
#[tauri::command]
async fn tester_start_poll_test(
    state: State<'_, AppState>,
    config: TesterPollTestConfig,
) -> Result<(), String> {
    run_tester_blocking(&state, move |app| app.start_poll_test(config)).await
}

#[cfg(not(test))]
#[tauri::command]
fn tester_stop_poll_test(state: State<'_, AppState>) -> Result<(), String> {
    state.tester.stop_poll_test();
    Ok(())
}

#[cfg(not(test))]
#[tauri::command]
fn tester_poll_status(state: State<'_, AppState>) -> Result<TesterPollStatus, String> {
    Ok(state.tester.poll_status())
}

#[cfg(not(test))]
#[tauri::command]
async fn tester_auto_axis_threshold(
    state: State<'_, AppState>,
    config: TesterMeasurementConfig,
) -> Result<AxisThresholdCalibration, String> {
    run_tester_blocking(&state, move |app| app.auto_axis_threshold(&config)).await
}

#[cfg(not(test))]
#[tauri::command]
async fn tester_reenumerate(
    state: State<'_, AppState>,
    tester: String,
) -> Result<Option<AttachedController>, String> {
    run_tester_blocking(&state, move |app| app.reenumerate(&tester)).await
}

#[cfg(not(test))]
#[tauri::command]
async fn tester_export_enum_trace(
    state: State<'_, AppState>,
    tester: String,
    output_dir: String,
    previous: Option<bool>,
) -> Result<EnumTraceExport, String> {
    run_tester_blocking(&state, move |app| {
        app.export_enum_trace(&tester, &output_dir, previous.unwrap_or(false))
    })
    .await
}

#[cfg(not(test))]
#[tauri::command]
async fn probe_devices(state: State<'_, AppState>) -> Result<ProbeResult, String> {
    let cache = state.last_probe.clone();
    let result = tauri::async_runtime::spawn_blocking(|| {
        device_probe::probe_devices().map_err(|error| format!("{error:#}"))
    })
    .await
    .map_err(|error| format!("probe task failed: {error}"))??;
    *cache.lock() = Some(result.clone());
    Ok(result)
}

#[cfg(not(test))]
#[tauri::command]
fn start_measurement(state: State<'_, AppState>, config: MeasurementConfig) -> Result<(), String> {
    let mut worker_slot = state.worker.lock();
    if *state.axis_calibration_running.lock() {
        return Err("axis threshold calibration is already running".to_string());
    }
    if state.status.lock().running {
        return Err("measurement is already running".to_string());
    }
    if state.poll_status.lock().running {
        return Err("polling rate test is already running".to_string());
    }

    let device = state
        .last_probe
        .lock()
        .as_ref()
        .and_then(|probe| {
            probe
                .devices
                .iter()
                .find(|device| device.id == config.device_id)
                .cloned()
        })
        .ok_or_else(|| "device not found; probe devices first".to_string())?;

    if let Some(mut worker) = worker_slot.take() {
        if let Some(join) = worker.join.take() {
            let _ = join.join();
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let status = state.status.clone();
    let join = std::thread::spawn(move || {
        measurement::run_measurement(config, device, status, worker_stop);
    });

    *worker_slot = Some(WorkerHandle {
        stop,
        join: Some(join),
    });

    Ok(())
}

#[cfg(not(test))]
#[tauri::command]
fn stop_measurement(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(worker) = state.worker.lock().as_ref() {
        worker.stop.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[cfg(not(test))]
#[tauri::command]
fn measurement_status(state: State<'_, AppState>) -> Result<MeasurementStatus, String> {
    Ok(state.status.lock().clone())
}

#[cfg(not(test))]
#[tauri::command]
fn start_poll_test(state: State<'_, AppState>, config: PollTestConfig) -> Result<(), String> {
    let mut worker_slot = state.poll_worker.lock();
    if *state.axis_calibration_running.lock() {
        return Err("axis threshold calibration is already running".to_string());
    }
    if state.poll_status.lock().running {
        return Err("polling rate test is already running".to_string());
    }
    if state.status.lock().running {
        return Err("measurement is already running".to_string());
    }

    let device = state
        .last_probe
        .lock()
        .as_ref()
        .and_then(|probe| {
            probe
                .devices
                .iter()
                .find(|device| device.id == config.device_id)
                .cloned()
        })
        .ok_or_else(|| "device not found; probe devices first".to_string())?;

    if let Some(mut worker) = worker_slot.take() {
        if let Some(join) = worker.join.take() {
            let _ = join.join();
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let status = state.poll_status.clone();
    let join = std::thread::spawn(move || {
        polling_rate::run_poll_test(config, device, status, worker_stop);
    });

    *worker_slot = Some(WorkerHandle {
        stop,
        join: Some(join),
    });

    Ok(())
}

#[cfg(not(test))]
#[tauri::command]
fn stop_poll_test(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(worker) = state.poll_worker.lock().as_ref() {
        worker.stop.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[cfg(not(test))]
#[tauri::command]
fn poll_test_status(state: State<'_, AppState>) -> Result<PollTestStatus, String> {
    Ok(state.poll_status.lock().clone())
}

#[cfg(not(test))]
#[tauri::command]
async fn auto_axis_threshold(
    state: State<'_, AppState>,
    config: MeasurementConfig,
) -> Result<AxisThresholdCalibration, String> {
    {
        let mut running = state.axis_calibration_running.lock();
        if *running {
            return Err("axis threshold calibration is already running".to_string());
        }
        *running = true;
    }

    if state.status.lock().running {
        *state.axis_calibration_running.lock() = false;
        return Err("measurement is already running".to_string());
    }
    if state.poll_status.lock().running {
        *state.axis_calibration_running.lock() = false;
        return Err("polling rate test is already running".to_string());
    }

    let device = state
        .last_probe
        .lock()
        .as_ref()
        .and_then(|probe| {
            probe
                .devices
                .iter()
                .find(|device| device.id == config.device_id)
                .cloned()
        })
        .ok_or_else(|| {
            *state.axis_calibration_running.lock() = false;
            "device not found; probe devices first".to_string()
        })?;

    let result = tauri::async_runtime::spawn_blocking(move || {
        measurement::auto_axis_threshold(config, device).map_err(|error| format!("{error:#}"))
    })
    .await;

    *state.axis_calibration_running.lock() = false;
    result.map_err(|error| format!("axis threshold calibration task failed: {error}"))?
}

#[cfg(not(test))]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            probe_devices,
            start_measurement,
            stop_measurement,
            measurement_status,
            start_poll_test,
            stop_poll_test,
            poll_test_status,
            auto_axis_threshold,
            tester_probe,
            tester_start_measurement,
            tester_stop_measurement,
            tester_measurement_status,
            tester_start_poll_test,
            tester_stop_poll_test,
            tester_poll_status,
            tester_auto_axis_threshold,
            tester_reenumerate,
            tester_export_enum_trace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
