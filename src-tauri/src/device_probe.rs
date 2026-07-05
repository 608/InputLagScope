#[cfg(not(windows))]
use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
#[cfg(not(windows))]
use hidapi::HidApi;
use serialport::{available_ports, SerialPortType};

use crate::models::{DeviceInfo, ProbeResult, ProtocolKind, SerialDeviceInfo};
use crate::protocol::classify_vid_pid;
use crate::xinput::XInputApi;

const RP2040_TRIGGER_VID: u16 = 0x16C0;
const RP2040_TRIGGER_PID: u16 = 0x05E1;
const INPUT_LAG_SCOPE_TRIGGER_PRODUCT: &str = "InputLagScope";
const INPUT_LAG_SCOPE_TRIGGER_SERIAL: &str = "ILS001";

pub fn probe_devices() -> Result<ProbeResult> {
    let serial_ports = std::thread::spawn(probe_serial_ports);
    let mut devices = probe_input_devices()?;
    devices.sort_by(|a, b| {
        protocol_rank(&a.protocol)
            .cmp(&protocol_rank(&b.protocol))
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(ProbeResult {
        devices,
        serial_ports: serial_ports.join().unwrap_or_default(),
    })
}

#[derive(Clone, Debug)]
struct XInputHidMetadata {
    name: String,
    vendor_id: u16,
    product_id: u16,
    usage_page: u16,
    usage: u16,
    interface_number: i32,
    manufacturer: Option<String>,
    serial_number: Option<String>,
    path: String,
}

#[cfg(not(windows))]
#[derive(Clone, Debug)]
struct HidStringSupplement {
    manufacturer: Option<String>,
    serial_number: Option<String>,
}

#[cfg(not(windows))]
#[derive(Default)]
struct HidStringSets {
    manufacturers: BTreeSet<String>,
    serial_numbers: BTreeSet<String>,
}

#[cfg(windows)]
#[derive(Clone, Debug)]
struct RawHidDevice {
    vendor_id: u16,
    product_id: u16,
    usage_page: u16,
    usage: u16,
    interface_number: i32,
    product: Option<String>,
    manufacturer: Option<String>,
    serial_number: Option<String>,
    path: String,
}

#[cfg(windows)]
fn probe_input_devices() -> Result<Vec<DeviceInfo>> {
    let raw_devices = probe_raw_hid_devices();
    let xinput_hid_metadata = collect_raw_xinput_metadata(&raw_devices);
    let mut devices = raw_devices
        .iter()
        .filter(|device| {
            !is_xinput_hid_interface(&device.path)
                && device.usage_page == 0x01
                && matches!(device.usage, 0x04 | 0x05)
        })
        .map(raw_hid_device_info)
        .collect::<Vec<_>>();
    devices.extend(probe_xinput_devices(&xinput_hid_metadata));
    Ok(devices)
}

#[cfg(not(windows))]
fn probe_input_devices() -> Result<Vec<DeviceInfo>> {
    let api = HidApi::new()?;
    let xinput_hid_metadata = collect_xinput_hid_metadata(&api);
    let mut devices = probe_hid_devices(&api)?;
    devices.extend(probe_xinput_devices(&xinput_hid_metadata));
    Ok(devices)
}

#[cfg(windows)]
fn raw_hid_device_info(device: &RawHidDevice) -> DeviceInfo {
    let protocol = classify_hid_device(device.vendor_id, device.product_id);
    let name = raw_hid_name(device);

    DeviceInfo {
        id: format!("hid:{}", device.path),
        name,
        protocol,
        vendor_id: Some(device.vendor_id),
        product_id: Some(device.product_id),
        usage_page: Some(device.usage_page),
        usage: Some(device.usage),
        interface_number: Some(device.interface_number),
        manufacturer: device.manufacturer.clone(),
        serial_number: device.serial_number.clone(),
        path: Some(device.path.clone()),
        xinput_user_index: None,
        input_report_bytes: None,
        parsed_button_count: None,
        parsed_axis_count: None,
        report_ids: Vec::new(),
    }
}

#[cfg(windows)]
fn raw_hid_name(device: &RawHidDevice) -> String {
    device.product.clone().unwrap_or_else(|| {
        format!(
            "HID Gamepad {:04X}:{:04X}",
            device.vendor_id, device.product_id
        )
    })
}

#[cfg(windows)]
fn collect_raw_xinput_metadata(devices: &[RawHidDevice]) -> Vec<XInputHidMetadata> {
    let mut metadata = devices
        .iter()
        .filter(|device| is_xinput_hid_interface(&device.path))
        .map(|device| XInputHidMetadata {
            name: device.product.clone().unwrap_or_else(|| {
                format!(
                    "XInput Controller {:04X}:{:04X}",
                    device.vendor_id, device.product_id
                )
            }),
            vendor_id: device.vendor_id,
            product_id: device.product_id,
            usage_page: device.usage_page,
            usage: device.usage,
            interface_number: device.interface_number,
            manufacturer: device.manufacturer.clone(),
            serial_number: device.serial_number.clone(),
            path: device.path.clone(),
        })
        .collect::<Vec<_>>();

    metadata.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.vendor_id.cmp(&b.vendor_id))
            .then_with(|| a.product_id.cmp(&b.product_id))
    });
    metadata
}

#[cfg(windows)]
fn probe_raw_hid_devices() -> Vec<RawHidDevice> {
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;

    use windows_sys::Win32::UI::Input::{
        GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RIDI_DEVICEINFO,
        RID_DEVICE_INFO, RIM_TYPEHID,
    };

    let mut device_count = 0_u32;
    let list_item_size = size_of::<RAWINPUTDEVICELIST>() as u32;
    let status = unsafe { GetRawInputDeviceList(null_mut(), &mut device_count, list_item_size) };
    if status == u32::MAX || device_count == 0 {
        return Vec::new();
    }

    let mut list = Vec::<RAWINPUTDEVICELIST>::with_capacity(device_count as usize);
    let stored =
        unsafe { GetRawInputDeviceList(list.as_mut_ptr(), &mut device_count, list_item_size) };
    if stored == u32::MAX {
        return Vec::new();
    }
    unsafe {
        list.set_len(stored as usize);
    }

    list.into_iter()
        .filter_map(|entry| {
            if entry.dwType != RIM_TYPEHID {
                return None;
            }

            let mut info = unsafe { zeroed::<RID_DEVICE_INFO>() };
            info.cbSize = size_of::<RID_DEVICE_INFO>() as u32;
            let mut info_size = info.cbSize;
            let status = unsafe {
                GetRawInputDeviceInfoW(
                    entry.hDevice,
                    RIDI_DEVICEINFO,
                    (&mut info as *mut RID_DEVICE_INFO).cast(),
                    &mut info_size,
                )
            };
            if status == u32::MAX || status == 0 || info.dwType != RIM_TYPEHID {
                return None;
            }

            let path = raw_input_device_name(entry.hDevice)?;
            let strings = crate::windows_hid::read_hid_device_strings(&path);
            let hid = unsafe { info.Anonymous.hid };
            let vendor_id = u16::try_from(hid.dwVendorId).unwrap_or_default();
            let product_id = u16::try_from(hid.dwProductId).unwrap_or_default();

            Some(RawHidDevice {
                vendor_id,
                product_id,
                usage_page: hid.usUsagePage,
                usage: hid.usUsage,
                interface_number: parse_interface_number(&path),
                product: strings.product,
                manufacturer: strings.manufacturer,
                serial_number: strings.serial_number,
                path,
            })
        })
        .collect()
}

#[cfg(windows)]
fn raw_input_device_name(handle: windows_sys::Win32::Foundation::HANDLE) -> Option<String> {
    use std::ptr::null_mut;

    use windows_sys::Win32::UI::Input::{GetRawInputDeviceInfoW, RIDI_DEVICENAME};

    let mut len = 0_u32;
    let status = unsafe { GetRawInputDeviceInfoW(handle, RIDI_DEVICENAME, null_mut(), &mut len) };
    if status != 0 || len == 0 {
        return None;
    }

    let mut buffer = Vec::<u16>::with_capacity(len as usize);
    let status = unsafe {
        GetRawInputDeviceInfoW(
            handle,
            RIDI_DEVICENAME,
            buffer.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if status == u32::MAX || status == 0 {
        return None;
    }
    unsafe {
        buffer.set_len(status as usize);
    }

    Some(
        String::from_utf16_lossy(&buffer)
            .trim_matches('\0')
            .to_string(),
    )
}

#[cfg(windows)]
fn parse_interface_number(path: &str) -> i32 {
    parse_hex_after_marker(path, "MI_").unwrap_or(-1)
}

#[cfg(windows)]
fn parse_hex_after_marker(path: &str, marker: &str) -> Option<i32> {
    let upper = path.to_ascii_uppercase();
    let index = upper.find(marker)? + marker.len();
    let digits = upper[index..]
        .chars()
        .take_while(|value| value.is_ascii_hexdigit())
        .collect::<String>();
    i32::from_str_radix(&digits, 16).ok()
}

#[cfg(not(windows))]
fn probe_hid_devices(api: &HidApi) -> Result<Vec<DeviceInfo>> {
    let devices = api
        .device_list()
        .filter(|info| {
            let path = info.path().to_string_lossy();
            let usage_page = info.usage_page();
            let usage = info.usage();
            !is_xinput_hid_interface(&path) && usage_page == 0x01 && matches!(usage, 0x04 | 0x05)
        })
        .map(|info| {
            let vid = info.vendor_id();
            let pid = info.product_id();
            let path = info.path().to_string_lossy().into_owned();
            let protocol = classify_hid_device(vid, pid);
            let name = info
                .product_string()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("HID {:04X}:{:04X}", vid, pid));

            DeviceInfo {
                id: format!("hid:{path}"),
                name,
                protocol,
                vendor_id: Some(vid),
                product_id: Some(pid),
                usage_page: Some(info.usage_page()),
                usage: Some(info.usage()),
                interface_number: Some(info.interface_number()),
                manufacturer: info.manufacturer_string().map(ToOwned::to_owned),
                serial_number: info.serial_number().map(ToOwned::to_owned),
                path: Some(path),
                xinput_user_index: None,
                input_report_bytes: None,
                parsed_button_count: None,
                parsed_axis_count: None,
                report_ids: Vec::new(),
            }
        })
        .collect();
    Ok(devices)
}

fn classify_hid_device(vid: u16, pid: u16) -> ProtocolKind {
    classify_vid_pid(vid, pid)
}

fn is_xinput_hid_interface(path: &str) -> bool {
    path.to_ascii_uppercase().contains("&IG_")
}

#[cfg(not(windows))]
fn collect_xinput_hid_metadata(api: &HidApi) -> Vec<XInputHidMetadata> {
    let supplements = collect_hid_string_supplements(api);
    let mut metadata = api
        .device_list()
        .filter_map(|info| {
            let path = info.path().to_string_lossy().into_owned();
            if !is_xinput_hid_interface(&path) {
                return None;
            }

            let vid = info.vendor_id();
            let pid = info.product_id();
            let supplement = supplements.get(&(vid, pid));
            let direct_strings = direct_hid_strings(&path);
            let name = info
                .product_string()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("XInput Controller {:04X}:{:04X}", vid, pid));
            let manufacturer = direct_strings
                .manufacturer
                .or_else(|| supplement.and_then(|value| value.manufacturer.clone()))
                .or_else(|| normalized_non_standard_string(info.manufacturer_string()));
            let serial_number = direct_strings
                .serial_number
                .or_else(|| supplement.and_then(|value| value.serial_number.clone()))
                .or_else(|| normalized_string(info.serial_number()));

            Some(XInputHidMetadata {
                name,
                vendor_id: vid,
                product_id: pid,
                usage_page: info.usage_page(),
                usage: info.usage(),
                interface_number: info.interface_number(),
                manufacturer,
                serial_number,
                path,
            })
        })
        .collect::<Vec<_>>();

    metadata.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.vendor_id.cmp(&b.vendor_id))
            .then_with(|| a.product_id.cmp(&b.product_id))
    });
    metadata
}

#[cfg(not(windows))]
fn collect_hid_string_supplements(api: &HidApi) -> BTreeMap<(u16, u16), HidStringSupplement> {
    let mut sets = BTreeMap::<(u16, u16), HidStringSets>::new();
    for info in api.device_list() {
        let path = info.path().to_string_lossy();
        if is_xinput_hid_interface(&path) {
            continue;
        }

        let strings = hid_strings(info, &path);
        let entry = sets
            .entry((info.vendor_id(), info.product_id()))
            .or_default();
        if let Some(manufacturer) = strings.manufacturer {
            entry.manufacturers.insert(manufacturer);
        }
        if let Some(serial_number) = strings.serial_number {
            entry.serial_numbers.insert(serial_number);
        }
    }

    sets.into_iter()
        .map(|(key, value)| {
            (
                key,
                HidStringSupplement {
                    manufacturer: single_string(value.manufacturers),
                    serial_number: single_string(value.serial_numbers),
                },
            )
        })
        .collect()
}

#[cfg(not(windows))]
fn hid_strings(info: &hidapi::DeviceInfo, path: &str) -> HidStringSupplement {
    let direct = direct_hid_strings(path);
    HidStringSupplement {
        manufacturer: direct
            .manufacturer
            .or_else(|| normalized_string(info.manufacturer_string())),
        serial_number: direct
            .serial_number
            .or_else(|| normalized_string(info.serial_number())),
    }
}

#[cfg(not(windows))]
fn direct_hid_strings(_path: &str) -> HidStringSupplement {
    HidStringSupplement {
        manufacturer: None,
        serial_number: None,
    }
}

#[cfg(not(windows))]
fn normalized_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(not(windows))]
fn normalized_non_standard_string(value: Option<&str>) -> Option<String> {
    let value = normalized_string(value)?;
    (!value.starts_with('(')).then_some(value)
}

#[cfg(not(windows))]
fn single_string(values: BTreeSet<String>) -> Option<String> {
    if values.len() == 1 {
        values.into_iter().next()
    } else {
        None
    }
}

fn probe_xinput_devices(metadata: &[XInputHidMetadata]) -> Vec<DeviceInfo> {
    let Ok(api) = XInputApi::load() else {
        return Vec::new();
    };

    let connected_slots = api.connected_slots();
    let slot_count = connected_slots.len();

    connected_slots
        .into_iter()
        .enumerate()
        .map(|(index, slot)| {
            let hid = if slot_count == 1 {
                metadata.first()
            } else if metadata.len() == slot_count {
                metadata.get(index)
            } else {
                None
            };
            DeviceInfo {
                id: format!("xinput:{slot}"),
                name: hid
                    .map(|metadata| metadata.name.clone())
                    .unwrap_or_else(|| format!("XInput Controller {slot}")),
                protocol: ProtocolKind::XInput,
                vendor_id: hid.map(|metadata| metadata.vendor_id),
                product_id: hid.map(|metadata| metadata.product_id),
                usage_page: hid.map(|metadata| metadata.usage_page),
                usage: hid.map(|metadata| metadata.usage),
                interface_number: hid.map(|metadata| metadata.interface_number),
                manufacturer: hid.and_then(|metadata| metadata.manufacturer.clone()),
                serial_number: hid.and_then(|metadata| metadata.serial_number.clone()),
                path: hid.map(|metadata| metadata.path.clone()),
                xinput_user_index: Some(slot),
                input_report_bytes: Some(14),
                parsed_button_count: Some(16),
                parsed_axis_count: Some(6),
                report_ids: Vec::new(),
            }
        })
        .collect()
}

fn probe_serial_ports() -> Vec<SerialDeviceInfo> {
    let display_names = probe_serial_display_names();
    available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|port| {
            let display_name = display_names
                .get(&port.port_name)
                .cloned()
                .or_else(|| serial_port_type_display_name(&port.port_name, &port.port_type));
            match port.port_type {
                SerialPortType::UsbPort(info) => SerialDeviceInfo {
                    port_name: port.port_name,
                    port_type: "usb".to_string(),
                    display_name,
                    vid: Some(info.vid),
                    pid: Some(info.pid),
                    manufacturer: info.manufacturer,
                    product: info.product,
                    serial_number: info.serial_number,
                },
                SerialPortType::PciPort => SerialDeviceInfo {
                    port_name: port.port_name,
                    port_type: "pci".to_string(),
                    display_name,
                    vid: None,
                    pid: None,
                    manufacturer: None,
                    product: None,
                    serial_number: None,
                },
                SerialPortType::BluetoothPort => SerialDeviceInfo {
                    port_name: port.port_name,
                    port_type: "bluetooth".to_string(),
                    display_name,
                    vid: None,
                    pid: None,
                    manufacturer: None,
                    product: None,
                    serial_number: None,
                },
                SerialPortType::Unknown => SerialDeviceInfo {
                    port_name: port.port_name,
                    port_type: "unknown".to_string(),
                    display_name,
                    vid: None,
                    pid: None,
                    manufacturer: None,
                    product: None,
                    serial_number: None,
                },
            }
        })
        .filter(is_rp2040_trigger_serial_port)
        .collect()
}

fn is_rp2040_trigger_serial_port(port: &SerialDeviceInfo) -> bool {
    if port.vid != Some(RP2040_TRIGGER_VID) || port.pid != Some(RP2040_TRIGGER_PID) {
        return false;
    }

    let name_matches =
        matches_text_identifier(
            port.display_name.as_deref(),
            INPUT_LAG_SCOPE_TRIGGER_PRODUCT,
        ) || matches_text_identifier(port.product.as_deref(), INPUT_LAG_SCOPE_TRIGGER_PRODUCT);
    let serial_matches = matches_text_identifier(
        port.serial_number.as_deref(),
        INPUT_LAG_SCOPE_TRIGGER_SERIAL,
    );
    name_matches || serial_matches
}

fn matches_text_identifier(value: Option<&str>, expected: &str) -> bool {
    value
        .map(str::trim)
        .map_or(false, |value| value == expected)
}

fn serial_port_type_display_name(port_name: &str, port_type: &SerialPortType) -> Option<String> {
    match port_type {
        SerialPortType::UsbPort(info) => info
            .product
            .as_deref()
            .or(info.manufacturer.as_deref())
            .and_then(|value| clean_serial_display_name(port_name, value)),
        _ => None,
    }
}

fn clean_serial_display_name(port_name: &str, value: &str) -> Option<String> {
    let mut display = value.trim().to_string();
    let suffix = format!("({port_name})");
    if display.ends_with(&suffix) {
        display.truncate(display.len().saturating_sub(suffix.len()));
        display = display.trim_end().to_string();
    }
    (!display.is_empty() && !display.eq_ignore_ascii_case(port_name)).then_some(display)
}

#[cfg(windows)]
fn probe_serial_display_names() -> std::collections::BTreeMap<String, String> {
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};

    use windows_sys::core::GUID;
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        CM_Get_Device_IDW, CM_Get_Parent, SetupDiClassGuidsFromNameW, SetupDiDestroyDeviceInfoList,
        SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDevicePropertyW,
        SetupDiGetDeviceRegistryPropertyW, SetupDiOpenDevRegKey, SetupDiOpenDeviceInfoW,
        CR_SUCCESS, DICS_FLAG_GLOBAL, DIGCF_ALLCLASSES, DIGCF_PRESENT, DIREG_DEV,
        MAX_DEVICE_ID_LEN, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME, SP_DEVINFO_DATA,
    };
    use windows_sys::Win32::Devices::Properties::{
        DEVPKEY_Device_BusReportedDeviceDesc, DEVPROP_TYPE_STRING,
    };
    use windows_sys::Win32::Foundation::{FALSE, INVALID_HANDLE_VALUE, MAX_PATH};
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegQueryValueExW, KEY_READ, REG_SZ};

    fn to_wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_wide(buffer: &[u16]) -> Option<String> {
        let end = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        let value = String::from_utf16_lossy(&buffer[..end]).trim().to_string();
        (!value.is_empty()).then_some(value)
    }

    fn class_guids(class_name: &str) -> Vec<GUID> {
        let class_name = to_wide(class_name);
        let mut required = 1_u32;
        let mut guids = vec![GUID::from_u128(0); required as usize];

        for _ in 0..2 {
            guids.resize(required as usize, GUID::from_u128(0));
            let ok = unsafe {
                SetupDiClassGuidsFromNameW(
                    class_name.as_ptr(),
                    guids.as_mut_ptr(),
                    guids.len() as u32,
                    &mut required,
                )
            };
            if ok == FALSE {
                continue;
            }
            guids.truncate(required as usize);
            return guids;
        }

        Vec::new()
    }

    fn registry_port_name(info_set: isize, devinfo: &SP_DEVINFO_DATA) -> Option<String> {
        let key = unsafe {
            SetupDiOpenDevRegKey(info_set, devinfo, DICS_FLAG_GLOBAL, 0, DIREG_DEV, KEY_READ)
        };
        if key == INVALID_HANDLE_VALUE {
            return None;
        }

        let mut buffer = [0_u16; MAX_PATH as usize];
        let mut byte_len = (buffer.len() * size_of::<u16>()) as u32;
        let mut value_type = 0_u32;
        let value_name = to_wide("PortName");
        let status = unsafe {
            RegQueryValueExW(
                key,
                value_name.as_ptr(),
                null(),
                &mut value_type,
                buffer.as_mut_ptr().cast(),
                &mut byte_len,
            )
        };
        unsafe {
            RegCloseKey(key);
        }
        if status != 0 || value_type != REG_SZ {
            return None;
        }
        from_wide(&buffer)
    }

    fn registry_property(
        info_set: isize,
        devinfo: &SP_DEVINFO_DATA,
        property: u32,
    ) -> Option<String> {
        let mut buffer = [0_u16; MAX_PATH as usize];
        let mut value_type = 0_u32;
        let ok = unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                info_set,
                devinfo,
                property,
                &mut value_type,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * size_of::<u16>()) as u32,
                null_mut(),
            )
        };
        if ok == FALSE || value_type != REG_SZ {
            return None;
        }
        from_wide(&buffer).map(|value| {
            value
                .split(';')
                .next_back()
                .unwrap_or(&value)
                .trim()
                .to_string()
        })
    }

    fn device_property_string(
        info_set: isize,
        devinfo: &SP_DEVINFO_DATA,
        property: &windows_sys::Win32::Foundation::DEVPROPKEY,
    ) -> Option<String> {
        let mut buffer = [0_u16; MAX_PATH as usize];
        let mut property_type = 0_u32;
        let ok = unsafe {
            SetupDiGetDevicePropertyW(
                info_set,
                devinfo,
                property,
                &mut property_type,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * size_of::<u16>()) as u32,
                null_mut(),
                0,
            )
        };
        if ok == FALSE || property_type != DEVPROP_TYPE_STRING {
            return None;
        }
        from_wide(&buffer)
    }

    fn parent_instance_id(devinfo: &SP_DEVINFO_DATA) -> Option<String> {
        let mut parent = 0_u32;
        let result = unsafe { CM_Get_Parent(&mut parent, devinfo.DevInst, 0) };
        if result != CR_SUCCESS {
            return None;
        }

        let mut buffer = [0_u16; MAX_DEVICE_ID_LEN as usize];
        let result =
            unsafe { CM_Get_Device_IDW(parent, buffer.as_mut_ptr(), buffer.len() as u32, 0) };
        if result != CR_SUCCESS {
            return None;
        }
        from_wide(&buffer)
    }

    fn parent_bus_reported_name(_info_set: isize, devinfo: &SP_DEVINFO_DATA) -> Option<String> {
        let parent_id = parent_instance_id(devinfo)?;
        let parent_id_wide = to_wide(&parent_id);

        let parent_info_set = unsafe {
            SetupDiGetClassDevsW(null(), null(), null_mut(), DIGCF_PRESENT | DIGCF_ALLCLASSES)
        };
        if parent_info_set == INVALID_HANDLE_VALUE as isize {
            return None;
        }

        let mut parent_info = unsafe { zeroed::<SP_DEVINFO_DATA>() };
        parent_info.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
        let ok = unsafe {
            SetupDiOpenDeviceInfoW(
                parent_info_set,
                parent_id_wide.as_ptr(),
                null_mut(),
                0,
                &mut parent_info,
            )
        };
        if ok == FALSE {
            unsafe {
                SetupDiDestroyDeviceInfoList(parent_info_set);
            }
            return None;
        }
        let name = device_property_string(
            parent_info_set,
            &parent_info,
            &DEVPKEY_Device_BusReportedDeviceDesc,
        );
        unsafe {
            SetupDiDestroyDeviceInfoList(parent_info_set);
        }
        name
    }

    let mut names = std::collections::BTreeMap::new();
    for class_name in ["Ports", "Modem"] {
        for guid in class_guids(class_name) {
            let info_set =
                unsafe { SetupDiGetClassDevsW(&guid, null(), null_mut(), DIGCF_PRESENT) };
            if info_set == INVALID_HANDLE_VALUE as isize {
                continue;
            }

            let mut index = 0_u32;
            loop {
                let mut devinfo = unsafe { zeroed::<SP_DEVINFO_DATA>() };
                devinfo.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
                let ok = unsafe { SetupDiEnumDeviceInfo(info_set, index, &mut devinfo) };
                if ok == FALSE {
                    break;
                }
                index += 1;

                let Some(port_name) = registry_port_name(info_set, &devinfo) else {
                    continue;
                };
                let display_name = parent_bus_reported_name(info_set, &devinfo)
                    .or_else(|| {
                        device_property_string(
                            info_set,
                            &devinfo,
                            &DEVPKEY_Device_BusReportedDeviceDesc,
                        )
                    })
                    .or_else(|| registry_property(info_set, &devinfo, SPDRP_FRIENDLYNAME))
                    .or_else(|| registry_property(info_set, &devinfo, SPDRP_DEVICEDESC))
                    .and_then(|value| clean_serial_display_name(&port_name, &value));
                if let Some(display_name) = display_name {
                    names.insert(port_name, display_name);
                }
            }

            unsafe {
                SetupDiDestroyDeviceInfoList(info_set);
            }
        }
    }
    names
}

#[cfg(not(windows))]
fn probe_serial_display_names() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::new()
}

fn protocol_rank(protocol: &ProtocolKind) -> u8 {
    match protocol {
        ProtocolKind::Ds4 | ProtocolKind::DualSense | ProtocolKind::Switch => 0,
        ProtocolKind::XInput => 1,
        ProtocolKind::GenericHid => 2,
        ProtocolKind::Unknown => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serial_port_info(
        vid: Option<u16>,
        pid: Option<u16>,
        display_name: Option<&str>,
        product: Option<&str>,
        serial_number: Option<&str>,
    ) -> SerialDeviceInfo {
        SerialDeviceInfo {
            port_name: "COM20".to_string(),
            port_type: "usb".to_string(),
            display_name: display_name.map(ToOwned::to_owned),
            vid,
            pid,
            manufacturer: None,
            product: product.map(ToOwned::to_owned),
            serial_number: serial_number.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn rp2040_trigger_port_matches_product_name() {
        let port = serial_port_info(
            Some(RP2040_TRIGGER_VID),
            Some(RP2040_TRIGGER_PID),
            Some(INPUT_LAG_SCOPE_TRIGGER_PRODUCT),
            None,
            None,
        );

        assert!(is_rp2040_trigger_serial_port(&port));
    }

    #[test]
    fn rp2040_trigger_port_matches_serial_number() {
        let port = serial_port_info(
            Some(RP2040_TRIGGER_VID),
            Some(RP2040_TRIGGER_PID),
            None,
            None,
            Some(INPUT_LAG_SCOPE_TRIGGER_SERIAL),
        );

        assert!(is_rp2040_trigger_serial_port(&port));
    }

    #[test]
    fn rp2040_trigger_port_rejects_other_vid_pid() {
        let port = serial_port_info(
            Some(0xCAFE),
            Some(0x4020),
            Some(INPUT_LAG_SCOPE_TRIGGER_PRODUCT),
            None,
            Some(INPUT_LAG_SCOPE_TRIGGER_SERIAL),
        );

        assert!(!is_rp2040_trigger_serial_port(&port));
    }
}
