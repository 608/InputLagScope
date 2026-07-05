use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::Sender;
use hidapi::HidApi;

use crate::clock::timestamp_ns;
use crate::hid_descriptor::HidReportParser;
use crate::models::{DeviceInfo, ProtocolKind, RawSample};
use crate::protocol::parse_report_with_descriptor;
use crate::thread_tuning::set_current_thread_high_priority;
use crate::xinput::{snapshot_to_sample, XInputApi};

#[derive(Default)]
pub struct CaptureStats {
    dropped_samples: AtomicU64,
}

impl CaptureStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn dropped_samples(&self) -> u64 {
        self.dropped_samples.load(Ordering::Relaxed)
    }

    fn record_drop(&self) {
        self.dropped_samples.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn start_capture(
    device: DeviceInfo,
    stop: Arc<AtomicBool>,
    samples: Sender<RawSample>,
    stats: Arc<CaptureStats>,
) -> Result<JoinHandle<()>> {
    match device.protocol {
        ProtocolKind::XInput if device.xinput_user_index.is_some() => {
            Ok(thread::spawn(move || {
                capture_xinput(device, stop, samples, stats);
            }))
        }
        ProtocolKind::XInput => Err(anyhow!("XInput capture requires an XInput user index")),
        ProtocolKind::Ds4
        | ProtocolKind::DualSense
        | ProtocolKind::Switch
        | ProtocolKind::GenericHid
        | ProtocolKind::Unknown
            if device.path.is_some() =>
        {
            let path = device.path.clone().unwrap();
            Ok(thread::spawn(move || {
                capture_hid(device, path, stop, samples, stats);
            }))
        }
        _ => Err(anyhow!("capture source is not usable")),
    }
}

fn capture_hid(
    device: DeviceInfo,
    path: String,
    stop: Arc<AtomicBool>,
    samples: Sender<RawSample>,
    stats: Arc<CaptureStats>,
) {
    let result = (|| -> Result<()> {
        set_current_thread_high_priority();
        let path_c = CString::new(path.clone()).context("HID path contains NUL")?;
        #[cfg(windows)]
        let descriptor_parser = if device.protocol == ProtocolKind::Switch {
            let api = HidApi::new()?;
            let hid = api.open_path(&path_c)?;
            initialize_hid_if_needed(&device, &hid);
            build_descriptor_parser(&device, &hid)
        } else {
            None
        };
        #[cfg(not(windows))]
        let descriptor_parser = {
            let api = HidApi::new()?;
            let hid = api.open_path(&path_c)?;
            initialize_hid_if_needed(&device, &hid);
            build_descriptor_parser(&device, &hid)
        };

        #[cfg(windows)]
        {
            capture_hid_direct_windows(
                device,
                path,
                descriptor_parser,
                stop,
                samples.clone(),
                stats.clone(),
            )?;
        }

        #[cfg(not(windows))]
        {
            let mut sequence = 0_u64;
            let report_bytes = device.input_report_bytes.unwrap_or(64).clamp(1, 1024);
            let hid = {
                let api = HidApi::new()?;
                api.open_path(&path_c)?
            };
            let mut buf = vec![0_u8; report_bytes];

            while !stop.load(Ordering::Relaxed) {
                let read_result = hid
                    .read_timeout(&mut buf, 10)
                    .map(|len| (len > 0).then(|| buf[..len].to_vec()))
                    .map_err(anyhow::Error::new);

                match read_result {
                    Ok(None) => {}
                    Ok(Some(raw)) => {
                        let sample = RawSample {
                            timestamp_ns: timestamp_ns(),
                            sequence,
                            protocol: device.protocol.clone(),
                            state: parse_report_with_descriptor(
                                &device.protocol,
                                &raw,
                                descriptor_parser.as_ref(),
                            ),
                            raw_report: raw,
                        };
                        sequence = sequence.wrapping_add(1);
                        if samples.try_send(sample).is_err() {
                            stats.record_drop();
                            thread::yield_now();
                        }
                    }
                    Err(_) => thread::sleep(Duration::from_millis(1)),
                }
            }
        }
        Ok(())
    })();

    if let Err(error) = result {
        if samples
            .try_send(RawSample {
                timestamp_ns: timestamp_ns(),
                sequence: 0,
                protocol: ProtocolKind::Unknown,
                raw_report: format!("capture error: {error:#}").into_bytes(),
                state: Default::default(),
            })
            .is_err()
        {
            stats.record_drop();
        }
    }
}

#[cfg(windows)]
fn capture_hid_direct_windows(
    device: DeviceInfo,
    path: String,
    descriptor_parser: Option<HidReportParser>,
    stop: Arc<AtomicBool>,
    samples: Sender<RawSample>,
    stats: Arc<CaptureStats>,
) -> Result<()> {
    let mut sequence = 0_u64;
    let report_bytes = device.input_report_bytes.unwrap_or(64).clamp(1, 1024);
    let reader = crate::windows_hid::WindowsHidReader::open(&path, report_bytes)?;

    while !stop.load(Ordering::Relaxed) {
        match reader.read_timeout(100) {
            Ok(None) => {}
            Ok(Some(report)) => {
                let sample = RawSample {
                    timestamp_ns: report.timestamp_ns,
                    sequence,
                    protocol: device.protocol.clone(),
                    state: parse_report_with_descriptor(
                        &device.protocol,
                        &report.raw,
                        descriptor_parser.as_ref(),
                    ),
                    raw_report: report.raw,
                };
                sequence = sequence.wrapping_add(1);
                if samples.try_send(sample).is_err() {
                    stats.record_drop();
                    thread::yield_now();
                }
            }
            Err(_) => thread::sleep(Duration::from_millis(1)),
        }
    }

    Ok(())
}

fn capture_xinput(
    device: DeviceInfo,
    stop: Arc<AtomicBool>,
    samples: Sender<RawSample>,
    stats: Arc<CaptureStats>,
) {
    set_current_thread_high_priority();
    let Some(slot) = device.xinput_user_index else {
        return;
    };
    let Ok(api) = XInputApi::load() else {
        return;
    };

    let mut last_packet = None;
    let mut sequence = 0_u64;
    while !stop.load(Ordering::Relaxed) {
        if let Ok(snapshot) = api.get_state(slot) {
            if last_packet != Some(snapshot.packet_number) {
                last_packet = Some(snapshot.packet_number);
                let sample = snapshot_to_sample(snapshot, sequence);
                sequence = sequence.wrapping_add(1);
                if samples.try_send(sample).is_err() {
                    stats.record_drop();
                }
            }
        }
        std::hint::spin_loop();
    }
}

fn build_descriptor_parser(
    device: &DeviceInfo,
    hid: &hidapi::HidDevice,
) -> Option<HidReportParser> {
    if !matches!(
        device.protocol,
        ProtocolKind::GenericHid | ProtocolKind::Unknown
    ) {
        return None;
    }

    let mut descriptor = vec![0_u8; 4096];
    let len = hid.get_report_descriptor(&mut descriptor).ok()?;
    HidReportParser::from_descriptor(&descriptor[..len]).ok()
}

fn initialize_hid_if_needed(device: &DeviceInfo, hid: &hidapi::HidDevice) {
    if device.protocol == ProtocolKind::Switch {
        let sequence = [
            vec![0x80, 0x02],
            vec![0x80, 0x03],
            vec![0x80, 0x02],
            vec![0x80, 0x04],
            vec![0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03, 0x30],
        ];
        for packet in sequence {
            let _ = hid.write(&packet);
            thread::sleep(Duration::from_millis(20));
        }
    }
}
