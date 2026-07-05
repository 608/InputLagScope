use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use parking_lot::Mutex;

use crate::device_probe::probe_devices;
use crate::measurement::run_measurement;
use crate::models::{
    DeviceInfo, MeasurementConfig, MeasurementInputType, MeasurementStatus, ProtocolKind, Summary,
};

pub fn run_headless_cli(args: Vec<String>) -> Result<()> {
    let options = HeadlessOptions::parse(args)?;
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
                "--button-index" => options.button_index = parse_value(&args, &mut index, key)?,
                "--axis-index" => options.axis_index = parse_value(&args, &mut index, key)?,
                "--axis-threshold" => options.axis_threshold = parse_value(&args, &mut index, key)?,
                "--neutral-sample-ms" => {
                    options.neutral_sample_ms = parse_value(&args, &mut index, key)?
                }
                "--retry-delay-ms" => options.retry_delay_ms = parse_value(&args, &mut index, key)?,
                "--output-dir" => options.output_dir = take_value(&args, &mut index, key)?,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => return Err(anyhow!("unknown option `{other}`")),
            }
        }

        Ok(options)
    }
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
           --button-index <n>\n\
           --axis-index <n>\n\
           --axis-threshold <value>\n\
           --retry-delay-ms <ms>\n\
           --output-dir <path>"
    );
}
