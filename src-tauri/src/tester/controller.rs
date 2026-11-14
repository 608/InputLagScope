//! Discovery and protocol classification of the controller attached to the
//! tester: GET_DEVICE_INFO gives VID/PID, the tester's
//! classification and the interfaces; the descriptors (device,
//! configuration, HID report descriptors, strings) are pulled as objects.

use anyhow::{anyhow, Result};

use crate::hid_descriptor::HidReportParser;
use crate::models::ProtocolKind;
use crate::models::{AttachedController, ControllerDescriptors, InterfaceReportDescriptor};
use crate::protocol::classify_vid_pid;
use crate::report_layout::{ControllerProtocol, ReportDecoder};

use super::objects::{decode_string_descriptor, read_object};
use super::protocol::{
    classification_name, speed_name, DeviceInfoResponse, InterfaceInfo, OBJ_CONFIG_DESCRIPTOR,
    OBJ_DEVICE_DESCRIPTOR, OBJ_REPORT_DESCRIPTOR, OBJ_STRING_DESCRIPTOR, STATUS_NO_DEVICE,
};
use super::transport::{TesterClient, TesterError};

/// Polling period: Full/Low Speed use bInterval
/// ms (rounded down to a power of two when interval_mode is 0), High Speed
/// uses 2^(bInterval-1) microframes of 125 us.
pub fn poll_period_us(speed: u8, b_interval: u8, interval_mode: u8) -> Option<f64> {
    match speed {
        1 | 2 => {
            let b = u32::from(b_interval.max(1));
            let ms = if interval_mode == 0 {
                1_u32 << (31 - b.leading_zeros())
            } else {
                b
            };
            Some(f64::from(ms) * 1000.0)
        }
        3 => {
            let b = u32::from(b_interval.clamp(1, 16));
            Some(f64::from(1_u32 << (b - 1)) * 125.0)
        }
        _ => None,
    }
}

fn protocol_from_vid_pid(vid: u16, pid: u16) -> ControllerProtocol {
    match classify_vid_pid(vid, pid) {
        ProtocolKind::Ds4 => ControllerProtocol::Ds4,
        ProtocolKind::DualSense => ControllerProtocol::DualSense,
        ProtocolKind::Switch => ControllerProtocol::Switch,
        _ => ControllerProtocol::GenericHid,
    }
}

fn parse_descriptor(
    descriptors: &[InterfaceReportDescriptor],
    iface: u8,
) -> Option<HidReportParser> {
    descriptors
        .iter()
        .find(|item| item.interface_number == iface)
        .and_then(|item| HidReportParser::from_descriptor(&item.descriptor).ok())
}

/// Chooses the protocol and the interface to measure:
/// 1. a GIP interface (class FF/47/D0) or classification 4 -> GIP
/// 2. an XUSB gamepad interface (FF/5D/01) or classification 3 -> XUSB
///    (preferred over HID in XUSB+HID composites)
/// 3. HID interfaces: DS4 / DualSense / Switch Pro by VID/PID
///    (`protocol.rs::classify_vid_pid`), otherwise generic HID using the
///    interface whose report descriptor has the most buttons and axes.
pub fn classify_controller(
    info: &DeviceInfoResponse,
    report_descriptors: &[InterfaceReportDescriptor],
) -> (ControllerProtocol, Option<u8>) {
    let with_in = |iface: &&InterfaceInfo| iface.ep_in != 0;

    if let Some(iface) = info.interfaces.iter().filter(with_in).find(|i| i.is_gip()) {
        return (ControllerProtocol::Gip, Some(iface.iface_number));
    }
    if info.classification == 4 {
        let target = info.interfaces.iter().find(with_in).map(|i| i.iface_number);
        return (ControllerProtocol::Gip, target);
    }
    if let Some(iface) = info
        .interfaces
        .iter()
        .filter(with_in)
        .find(|i| i.is_xusb_gamepad())
    {
        return (ControllerProtocol::Xusb, Some(iface.iface_number));
    }
    if info.classification == 3 {
        let target = info.interfaces.iter().find(with_in).map(|i| i.iface_number);
        return (ControllerProtocol::Xusb, target);
    }

    let hid: Vec<&InterfaceInfo> = info
        .interfaces
        .iter()
        .filter(with_in)
        .filter(|i| i.is_hid())
        .collect();
    if hid.is_empty() {
        return (ControllerProtocol::Unknown, None);
    }
    let protocol = protocol_from_vid_pid(info.vid, info.pid);
    if protocol != ControllerProtocol::GenericHid {
        return (protocol, Some(hid[0].iface_number));
    }
    let best = hid
        .iter()
        .map(|iface| {
            let score = parse_descriptor(report_descriptors, iface.iface_number)
                .map(|parser| {
                    let fields = parser.input_fields();
                    fields
                        .iter()
                        .filter(|field| field.kind != crate::hid_descriptor::HidFieldKind::Other)
                        .count()
                })
                .unwrap_or(0);
            (score, iface.iface_number)
        })
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
        .map(|(_, number)| number);
    (ControllerProtocol::GenericHid, best)
}

/// Reads GET_DEVICE_INFO and the descriptors. `Ok(None)` when no
/// controller is configured (NO_DEVICE).
pub fn discover_controller(
    client: &TesterClient,
    interval_mode: u8,
) -> Result<Option<AttachedController>, TesterError> {
    let info = match client.get_device_info() {
        Ok(info) => info,
        Err(error) if error.is_status(STATUS_NO_DEVICE) => return Ok(None),
        Err(error) => return Err(error),
    };
    let generation = info.device_generation;
    let mut descriptors = ControllerDescriptors::default();

    let fatal = |error: &TesterError| {
        matches!(
            error,
            TesterError::Disconnected(_) | TesterError::Timeout { .. } | TesterError::Io(_)
        )
    };

    match read_object(client, OBJ_DEVICE_DESCRIPTOR, 0, generation) {
        Ok(data) => descriptors.device = data,
        Err(error) if fatal(&error) => return Err(error),
        Err(error) => descriptors
            .errors
            .push(format!("device descriptor: {error}")),
    }
    match read_object(client, OBJ_CONFIG_DESCRIPTOR, 0, generation) {
        Ok(data) => descriptors.configuration = data,
        Err(error) if fatal(&error) => return Err(error),
        Err(error) => descriptors
            .errors
            .push(format!("configuration descriptor: {error}")),
    }
    let mut hid_numbers: Vec<u8> = info
        .interfaces
        .iter()
        .filter(|iface| iface.is_hid() && iface.report_desc_len > 0)
        .map(|iface| iface.iface_number)
        .collect();
    hid_numbers.dedup();
    for number in hid_numbers {
        match read_object(client, OBJ_REPORT_DESCRIPTOR, number, generation) {
            Ok(data) => descriptors
                .report_descriptors
                .push(InterfaceReportDescriptor {
                    interface_number: number,
                    descriptor: data,
                }),
            Err(error) if fatal(&error) => return Err(error),
            Err(error) => descriptors
                .errors
                .push(format!("report descriptor of interface {number}: {error}")),
        }
    }

    let mut strings = [None, None, None];
    if descriptors.device.len() >= 18 {
        for (slot, offset) in [14_usize, 15, 16].iter().enumerate() {
            let index = descriptors.device[*offset];
            if index == 0 {
                continue;
            }
            match read_object(client, OBJ_STRING_DESCRIPTOR, index, generation) {
                Ok(data) => strings[slot] = decode_string_descriptor(&data),
                Err(error) if fatal(&error) => return Err(error),
                Err(error) => descriptors
                    .errors
                    .push(format!("string descriptor {index}: {error}")),
            }
        }
    }
    let [manufacturer, product, serial_number] = strings;

    Ok(Some(build_controller(
        info,
        descriptors,
        manufacturer,
        product,
        serial_number,
        interval_mode,
    )))
}

pub fn build_controller(
    info: DeviceInfoResponse,
    descriptors: ControllerDescriptors,
    manufacturer: Option<String>,
    product: Option<String>,
    serial_number: Option<String>,
    interval_mode: u8,
) -> AttachedController {
    let (protocol, target) = classify_controller(&info, &descriptors.report_descriptors);
    let target_iface = target.and_then(|number| find_interface(&info.interfaces, number));
    let (buttons, axes) = target
        .and_then(|number| {
            let parser = parse_descriptor(&descriptors.report_descriptors, number);
            ReportDecoder::new(protocol, parser).map(|decoder| {
                let layout = decoder.default_layout();
                (layout.buttons.len(), layout.axes.len())
            })
        })
        .map_or((None, None), |(b, a)| (Some(b), Some(a)));
    AttachedController {
        device_generation: info.device_generation,
        speed: info.speed,
        speed_name: speed_name(info.speed).to_string(),
        address: info.address,
        vendor_id: info.vid,
        product_id: info.pid,
        bcd_usb: info.bcd_usb,
        bcd_device: info.bcd_device,
        device_class: info.device_class,
        classification: info.classification,
        classification_name: classification_name(info.classification).to_string(),
        num_interfaces: info.num_interfaces,
        vendor_code_state: info.vendor_code_state,
        vendor_code: info.vendor_code,
        manufacturer,
        product,
        serial_number,
        protocol,
        target_interface: target_iface.as_ref().map(|iface| iface.iface_number),
        target_ep_in: target_iface.as_ref().map(|iface| iface.ep_in),
        poll_period_us: target_iface
            .as_ref()
            .and_then(|iface| poll_period_us(info.speed, iface.in_interval, interval_mode)),
        parsed_button_count: buttons,
        parsed_axis_count: axes,
        interfaces: info.interfaces,
        descriptors: Some(descriptors),
    }
}

/// Interface entry with an IN endpoint for `number` (several alternate
/// settings may share a number).
fn find_interface(interfaces: &[InterfaceInfo], number: u8) -> Option<InterfaceInfo> {
    interfaces
        .iter()
        .filter(|iface| iface.iface_number == number)
        .find(|iface| iface.ep_in != 0)
        .or_else(|| interfaces.iter().find(|iface| iface.iface_number == number))
        .cloned()
}

/// Interface chosen for a measurement or a polling test.
#[derive(Clone, Debug)]
pub struct SelectedInterface {
    pub iface: InterfaceInfo,
    pub protocol: ControllerProtocol,
    pub parser: Option<HidReportParser>,
}

impl SelectedInterface {
    pub fn decoder(&self) -> Result<ReportDecoder> {
        ReportDecoder::new(self.protocol, self.parser.clone()).ok_or_else(|| {
            anyhow!(
                "interface {} ({}) cannot be decoded: {}",
                self.iface.iface_number,
                self.protocol.label(),
                if self.protocol == ControllerProtocol::GenericHid {
                    "HID report descriptor is not available"
                } else {
                    "unsupported protocol"
                }
            )
        })
    }
}

/// Resolves the interface to use: `requested` or the classified target.
pub fn select_interface(
    controller: &AttachedController,
    requested: Option<u8>,
) -> Result<SelectedInterface> {
    let number = requested
        .or(controller.target_interface)
        .ok_or_else(|| anyhow!("the attached controller has no interface to measure"))?;
    let iface = find_interface(&controller.interfaces, number)
        .ok_or_else(|| anyhow!("interface {number} does not exist on the controller"))?;
    if iface.ep_in == 0 {
        return Err(anyhow!("interface {number} has no IN endpoint"));
    }
    let protocol = if Some(number) == controller.target_interface {
        controller.protocol
    } else if iface.is_gip() {
        ControllerProtocol::Gip
    } else if iface.is_xusb_gamepad() {
        ControllerProtocol::Xusb
    } else if iface.is_hid() {
        protocol_from_vid_pid(controller.vendor_id, controller.product_id)
    } else {
        ControllerProtocol::Unknown
    };
    let parser = controller
        .descriptors
        .as_ref()
        .and_then(|descriptors| parse_descriptor(&descriptors.report_descriptors, number));
    Ok(SelectedInterface {
        iface,
        protocol,
        parser,
    })
}
