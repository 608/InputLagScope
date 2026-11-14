use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use parking_lot::Mutex;

use crate::device_probe::probe_devices;
use crate::measurement::run_measurement;
use crate::models::{
    DeviceInfo, MeasurementConfig, MeasurementInputType, MeasurementStatus, ProtocolKind, Summary,
    TesterInfo,
};
use crate::tester::app::TesterApp;
use crate::tester::measure::MeasurementOutcome;
use crate::tester::model::{parse_report_index, TesterMeasurementConfig, TesterMeasurementStatus};
use crate::tester::protocol::completion_reason_name;
use crate::tester::stats::LatencyDistribution;

pub fn run_headless_cli(args: Vec<String>) -> Result<()> {
    let options = HeadlessOptions::parse(args)?;
    if let Some(tester) = options.tester.clone() {
        // Tester path: every failure is returned so that main() exits with 1.
        return run_tester_headless(&options, &tester);
    }
    let probe = probe_devices()?;
    let device_id = if let Some(device_id) = options.device_id {
        device_id
    } else if let Some(needle) = options.device_id_contains {
        probe
            .devices
            .iter()
            .find(|device| device.id.contains(&needle))
            .map(|device| device.id.clone())
            .ok_or_else(|| anyhow!("device id containing `{needle}` was not found"))?
    } else {
        probe
            .devices
            .iter()
            .find(|device| device.protocol == ProtocolKind::XInput)
            .or_else(|| probe.devices.first())
            .map(|device| device.id.clone())
            .ok_or_else(|| anyhow!("no input device was found"))?
    };

    let device = probe
        .devices
        .iter()
        .find(|device| device.id == device_id)
        .cloned()
        .ok_or_else(|| anyhow!("selected device is not found"))?;

    let config = MeasurementConfig {
        device_id,
        serial_port: options
            .serial_port
            .or_else(|| {
                probe
                    .serial_ports
                    .first()
                    .map(|port| port.port_name.clone())
            })
            .ok_or_else(|| anyhow!("serial port was not specified and no serial port was found"))?,
        baud_rate: options.baud_rate,
        sample_count: options.samples,
        timeout_ms: options.timeout_ms,
        input_type: options.input_type,
        button_index: options.button_index,
        axis_index: options.axis_index,
        axis_threshold: options.axis_threshold,
        neutral_sample_ms: options.neutral_sample_ms,
        retry_delay_ms: options.retry_delay_ms,
        output_dir: options.output_dir,
    };

    let status = Arc::new(Mutex::new(MeasurementStatus::idle()));
    let stop = Arc::new(AtomicBool::new(false));
    run_measurement(config, device.clone(), status.clone(), stop);
    print_summary(&device, &status.lock());
    Ok(())
}

fn print_summary(device: &DeviceInfo, status: &MeasurementStatus) {
    println!("Measurement finished");
    println!("device     : {}", device.name);
    println!("protocol   : {}", protocol_label(&device.protocol));
    if let (Some(vid), Some(pid)) = (device.vendor_id, device.product_id) {
        println!("vid_pid    : 0x{vid:04X}/0x{pid:04X}");
    }
    if let Some(serial) = &device.serial_number {
        println!("serial     : {serial}");
    }
    println!(
        "samples    : {}/{}",
        status.completed_samples, status.requested_samples
    );
    println!("failures   : {}", status.failures);
    print_summary_values(&status.summary);
    if let Some(path) = &status.output_path {
        println!("result     : {path}");
    }
}

fn print_summary_values(summary: &Summary) {
    println!("average_ms : {}", format_optional_ms(summary.average_ms));
    println!("jitter_ms  : {}", format_optional_ms(summary.jitter_ms));
    println!("minimum_ms : {}", format_optional_ms(summary.min_ms));
    println!("p05_ms     : {}", format_optional_ms(summary.p05_ms));
    println!("median_ms  : {}", format_optional_ms(summary.median_ms));
    println!("p95_ms     : {}", format_optional_ms(summary.p95_ms));
    println!("maximum_ms : {}", format_optional_ms(summary.max_ms));
}

fn format_optional_ms(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.6}"))
        .unwrap_or_else(|| "-".to_string())
}

fn protocol_label(protocol: &ProtocolKind) -> &'static str {
    match protocol {
        ProtocolKind::Ds4 => "ds4",
        ProtocolKind::DualSense => "dualsense",
        ProtocolKind::Switch => "switch",
        ProtocolKind::XInput => "xinput",
        ProtocolKind::GenericHid => "generic_hid",
        ProtocolKind::Unknown => "unknown",
    }
}

struct HeadlessOptions {
    device_id: Option<String>,
    device_id_contains: Option<String>,
    serial_port: Option<String>,
    baud_rate: u32,
    samples: usize,
    timeout_ms: u64,
    input_type: MeasurementInputType,
    button_index: i32,
    axis_index: i32,
    axis_threshold: f32,
    neutral_sample_ms: u64,
    retry_delay_ms: u64,
    output_dir: String,
    tester: Option<String>,
    seed: Option<u32>,
    measure_release: bool,
    /// Report ID from `--button-index` / `--axis-index` given as
    /// `report_id:index` (tester path only).
    report_id: Option<u8>,
}

impl HeadlessOptions {
    fn parse(args: Vec<String>) -> Result<Self> {
        let mut options = Self {
            device_id: None,
            device_id_contains: None,
            serial_port: None,
            baud_rate: 115200,
            samples: 100,
            timeout_ms: 1000,
            input_type: MeasurementInputType::Button,
            button_index: -1,
            axis_index: -1,
            axis_threshold: 0.35,
            neutral_sample_ms: 1000,
            retry_delay_ms: 5,
            output_dir: "results".to_string(),
            tester: None,
            seed: None,
            measure_release: false,
            report_id: None,
        };

        let mut index = 0;
        while index < args.len() {
            let key = &args[index];
            index += 1;
            match key.as_str() {
                "--device-id" => options.device_id = Some(take_value(&args, &mut index, key)?),
                "--device-id-contains" => {
                    options.device_id_contains = Some(take_value(&args, &mut index, key)?)
                }
                "--serial-port" => options.serial_port = Some(take_value(&args, &mut index, key)?),
                "--baud-rate" => options.baud_rate = parse_value(&args, &mut index, key)?,
                "--samples" => options.samples = parse_value(&args, &mut index, key)?,
                "--timeout-ms" => options.timeout_ms = parse_value(&args, &mut index, key)?,
                "--input-type" => {
                    options.input_type = match take_value(&args, &mut index, key)?.as_str() {
                        "button" => MeasurementInputType::Button,
                        "axis" => MeasurementInputType::Axis,
                        other => return Err(anyhow!("unsupported input type `{other}`")),
                    };
                }
                "--button-index" => {
                    options.button_index = options.parse_report_index(&args, &mut index, key)?
                }
                "--axis-index" => {
                    options.axis_index = options.parse_report_index(&args, &mut index, key)?
                }
                "--axis-threshold" => options.axis_threshold = parse_value(&args, &mut index, key)?,
                "--neutral-sample-ms" => {
                    options.neutral_sample_ms = parse_value(&args, &mut index, key)?
                }
                "--retry-delay-ms" => options.retry_delay_ms = parse_value(&args, &mut index, key)?,
                "--output-dir" => options.output_dir = take_value(&args, &mut index, key)?,
                "--tester" => options.tester = Some(take_value(&args, &mut index, key)?),
                "--seed" => options.seed = Some(parse_value(&args, &mut index, key)?),
                "--measure-release" => options.measure_release = true,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => return Err(anyhow!("unknown option `{other}`")),
            }
        }

        if options.tester.is_none() {
            for (set, name) in [
                (options.seed.is_some(), "--seed"),
                (options.measure_release, "--measure-release"),
                (
                    options.report_id.is_some(),
                    "a `report_id:index` value of --button-index / --axis-index",
                ),
            ] {
                if set {
                    return Err(anyhow!("`{name}` requires `--tester`"));
                }
            }
        }

        Ok(options)
    }

    /// `index` or `report_id:index`; the report ID must agree between
    /// `--button-index` and `--axis-index`.
    fn parse_report_index(&mut self, args: &[String], index: &mut usize, key: &str) -> Result<i32> {
        let value = take_value(args, index, key)?;
        let (report_id, parsed) =
            parse_report_index(&value).map_err(|error| anyhow!("`{key}`: {error}"))?;
        if let Some(report_id) = report_id {
            if self.report_id.is_some_and(|previous| previous != report_id) {
                return Err(anyhow!(
                    "`{key}`: report ID {report_id} differs from the one given before"
                ));
            }
            self.report_id = Some(report_id);
        }
        Ok(parsed)
    }
}

fn run_tester_headless(options: &HeadlessOptions, tester: &str) -> Result<()> {
    let app = TesterApp::with_serial_ports();
    let result = run_tester_headless_with(&app, options, tester);
    app.close_all();
    result
}

fn run_tester_headless_with(
    app: &TesterApp,
    options: &HeadlessOptions,
    tester: &str,
) -> Result<()> {
    let connection = app.connect(tester)?;
    let port = connection.info().port_name;
    for warning in connection.warnings() {
        eprintln!("warning    : {warning}");
    }
    let config = TesterMeasurementConfig {
        tester: port,
        sample_count: options.samples,
        timeout_ms: options.timeout_ms,
        release_timeout_ms: 1000,
        input_type: options.input_type.clone(),
        button_index: options.button_index,
        axis_index: options.axis_index,
        axis_threshold: options.axis_threshold,
        neutral_sample_ms: options.neutral_sample_ms,
        retry_delay_ms: options.retry_delay_ms,
        inter_trial_ms: None,
        output_dir: options.output_dir.clone(),
        measure_release: options.measure_release,
        settle_reports: 3,
        random_max_us: 0,
        seed: options.seed.unwrap_or(0),
        interface_number: None,
        report_id: options.report_id,
    };
    let (info, outcome) = app.run_measurement_blocking(&config)?;
    print_tester_summary(&info, &outcome, &app.measurement_status());
    match outcome.completion_reason {
        Some(0) => {}
        Some(reason) => {
            return Err(anyhow!(
                "the run ended early: {}",
                completion_reason_name(reason)
            ))
        }
        None => return Err(anyhow!("RUN_DONE was not received")),
    }
    if outcome.stats.latency.count == 0 {
        return Err(anyhow!(
            "no trial produced a latency (check the trigger wiring and the target input)"
        ));
    }
    Ok(())
}

fn print_tester_summary(
    tester: &TesterInfo,
    outcome: &MeasurementOutcome,
    status: &TesterMeasurementStatus,
) {
    println!("Tester measurement finished");
    println!("tester     : {} ({})", tester.uuid, tester.port_name);
    println!(
        "firmware   : {} build {}",
        tester.firmware_version, tester.build
    );
    if let Some(controller) = &status.controller {
        println!("device     : {}", controller.display_name());
        println!("protocol   : {}", controller.protocol.label());
        println!(
            "vid_pid    : 0x{:04X}/0x{:04X}",
            controller.vendor_id, controller.product_id
        );
    }
    let stats = &outcome.stats;
    println!("run_id     : {}", outcome.run_id);
    println!(
        "samples    : {}/{} valid of {} requested",
        stats.valid_latency_count, stats.completed, stats.requested
    );
    let fails = &stats.fail_counts;
    println!(
        "failures   : timeout {} not_released {} confirm_reverted {} queue_full {} disconnected {} aborted {}",
        fails.timeout,
        fails.not_released,
        fails.confirm_reverted,
        fails.event_queue_full,
        fails.disconnected,
        fails.aborted
    );
    if let Some(rate) = stats.success_rate {
        println!("success    : {:.2}%", rate * 100.0);
    }
    if stats.uncalibrated {
        println!("calibration: uncalibrated (statistics use t1_irq - t0_tick without c0/c1)");
    }
    print_distribution("", &stats.latency);
    if let Some(release) = &stats.release_latency {
        print_distribution("release_", release);
    }
    println!("result     : {}", outcome.output_path.display());
}

fn print_distribution(prefix: &str, distribution: &LatencyDistribution) {
    let line = |name: &str, value: Option<f64>| {
        println!(
            "{:<11}: {}",
            format!("{prefix}{name}"),
            format_optional_ms(value)
        );
    };
    line("mean_ms", distribution.mean_ms);
    line("stddev_ms", distribution.std_dev_ms);
    line("minimum_ms", distribution.min_ms);
    line("p50_ms", distribution.p50_ms);
    line("p95_ms", distribution.p95_ms);
    line("p99_ms", distribution.p99_ms);
    line("maximum_ms", distribution.max_ms);
}

fn take_value(args: &[String], index: &mut usize, key: &str) -> Result<String> {
    let value = args
        .get(*index)
        .ok_or_else(|| anyhow!("`{key}` requires a value"))?;
    *index += 1;
    Ok(value.clone())
}

fn parse_value<T>(args: &[String], index: &mut usize, key: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let value = take_value(args, index, key)?;
    value
        .parse::<T>()
        .map_err(|error| anyhow!("failed to parse `{key}` value `{value}`: {error}"))
}

fn print_help() {
    println!(
        "Usage: InputLagScope --headless-measure [options]\n\
         Options:\n\
           --device-id <id>\n\
           --device-id-contains <text>\n\
           --serial-port <COMx>\n\
           --samples <n>\n\
           --input-type button|axis\n\
           --button-index <n>        (tester: <report_id>:<n> for HID report IDs)\n\
           --axis-index <n>          (tester: <report_id>:<n> for HID report IDs)\n\
           --axis-threshold <value>\n\
           --retry-delay-ms <ms>\n\
           --output-dir <path>\n\
         CH32H417 tester:\n\
           --tester <uuid|COMx>      measure through the tester\n\
           --seed <n>                trial random seed (0 = hardware RNG)\n\
           --measure-release         also measure the release latency"
    );
}
