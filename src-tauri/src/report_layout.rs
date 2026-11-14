//! Report layouts: where each logical button and axis of a controller report
//! lives on the wire, plus decoders for the report formats the CH32H417
//! tester can deliver (DS4, DualSense, Switch Pro, XUSB, GIP, generic HID).
//!
//! The decoded state uses the same representation as `protocol.rs`
//! (`InputState { buttons: Vec<bool>, axes: Vec<f32> }`), and every logical
//! button or axis carries the raw field location needed to build the
//! tester's SET_WATCH: bit offset counted from the
//! least significant bit of byte 0 of the report (the report-ID byte
//! included), bit width, signedness and the "pressed" values.

use serde::{Deserialize, Serialize};

use crate::hid_descriptor::{read_bits, sign_extend, HidInputField, HidReportParser};
use crate::models::{InputState, ProtocolKind};
use crate::protocol::{
    normalize_i16, normalize_u12, normalize_u8, parse_ds4, parse_dualsense, parse_switch,
    parse_xinput_raw,
};

/// Protocol of a controller attached to the tester.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControllerProtocol {
    Ds4,
    DualSense,
    Switch,
    Xusb,
    Gip,
    GenericHid,
    Unknown,
}

impl ControllerProtocol {
    /// Closest protocol of the existing Windows-HID path (used when a tester
    /// result is written in the existing result format).
    pub fn to_protocol_kind(self) -> ProtocolKind {
        match self {
            Self::Ds4 => ProtocolKind::Ds4,
            Self::DualSense => ProtocolKind::DualSense,
            Self::Switch => ProtocolKind::Switch,
            Self::Xusb => ProtocolKind::XInput,
            Self::GenericHid => ProtocolKind::GenericHid,
            Self::Gip | Self::Unknown => ProtocolKind::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ds4 => "ds4",
            Self::DualSense => "dual_sense",
            Self::Switch => "switch",
            Self::Xusb => "xusb",
            Self::Gip => "gip",
            Self::GenericHid => "generic_hid",
            Self::Unknown => "unknown",
        }
    }
}

/// Raw location of one value inside a report.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldLocation {
    /// Required value of byte 0 of the report (report ID or packet type).
    /// `None` means no condition.
    pub report_id: Option<u8>,
    /// Shortest report that the decoder accepts for this field.
    pub min_length: u16,
    /// Bit position from the least significant bit of byte 0.
    pub bit_offset: u32,
    pub bit_size: u8,
    pub is_signed: bool,
    pub logical_min: i32,
    pub logical_max: i32,
    #[serde(default)]
    pub usage_page: Option<u16>,
    #[serde(default)]
    pub usage: Option<u16>,
}

impl FieldLocation {
    #[allow(clippy::too_many_arguments)]
    fn new(
        report_id: Option<u8>,
        min_length: u16,
        bit_offset: u32,
        bit_size: u8,
        is_signed: bool,
        logical_min: i32,
        logical_max: i32,
    ) -> Self {
        Self {
            report_id,
            min_length,
            bit_offset,
            bit_size,
            is_signed,
            logical_min,
            logical_max,
            usage_page: None,
            usage: None,
        }
    }

    fn bit(report_id: Option<u8>, min_length: u16, bit_offset: u32) -> Self {
        Self::new(report_id, min_length, bit_offset, 1, false, 0, 1)
    }

    /// Reads the value the same way the tester does (little-endian bit
    /// extraction, sign extension from the top bit when signed). Returns
    /// `None` when the report does not satisfy the report-ID condition or
    /// is too short for the field.
    pub fn extract(&self, raw: &[u8]) -> Option<i64> {
        if let Some(report_id) = self.report_id {
            if raw.first().copied() != Some(report_id) {
                return None;
            }
        }
        if self.bit_size == 0 || self.bit_size > 32 {
            return None;
        }
        let value = read_bits(raw, self.bit_offset, u32::from(self.bit_size))?;
        Some(if self.is_signed {
            i64::from(sign_extend(value, u32::from(self.bit_size)))
        } else {
            i64::from(value)
        })
    }

    /// Bytes needed to read the field.
    pub fn bytes_needed(&self) -> u16 {
        self.bit_offset
            .saturating_add(u32::from(self.bit_size))
            .div_ceil(8)
            .min(u32::from(u16::MAX)) as u16
    }
}

/// When a button field counts as pressed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "rule", rename_all = "snake_case")]
pub enum PressedRule {
    /// Pressed when the value equals one of these (at most 4 values).
    Values { values: Vec<u32> },
    /// Pressed when the value is not zero (multi-bit HID button fields).
    NonZero,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ButtonLocation {
    pub name: String,
    pub field: FieldLocation,
    pub pressed: PressedRule,
}

impl ButtonLocation {
    fn bit(name: &str, report_id: Option<u8>, min_length: u16, bit_offset: u32) -> Self {
        Self {
            name: name.to_string(),
            field: FieldLocation::bit(report_id, min_length, bit_offset),
            pressed: PressedRule::Values { values: vec![1] },
        }
    }

    fn hat(name: &str, report_id: u8, min_length: u16, bit_offset: u32, values: &[u32]) -> Self {
        Self {
            name: name.to_string(),
            field: FieldLocation::new(Some(report_id), min_length, bit_offset, 4, false, 0, 7),
            pressed: PressedRule::Values {
                values: values.to_vec(),
            },
        }
    }
}

/// How a raw axis value is mapped to the normalized value in `InputState`.
/// Each variant reproduces the exact expression used by `protocol.rs` or
/// `hid_descriptor.rs`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Normalization {
    /// `(v - 127.5) / 127.5` (DS4, DualSense sticks).
    U8Centered,
    /// `(v - 2048) / 2048` (Switch Pro 12-bit sticks).
    U12Centered,
    /// `v / 32767` for v >= 0, `v / 32768` otherwise (XInput sticks).
    I16,
    /// `v / max` (XInput / GIP triggers).
    UnitRange { max: f32 },
    /// `hid_descriptor.rs` logical-range normalization.
    HidLogical { min: i32, max: i32 },
}

impl Normalization {
    pub fn normalize(&self, raw: i64) -> f32 {
        match self {
            Self::U8Centered => normalize_u8(raw.clamp(0, 255) as u8),
            Self::U12Centered => normalize_u12(raw.clamp(0, 0xFFFF) as u16),
            Self::I16 => normalize_i16(raw.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16),
            Self::UnitRange { max } => raw as f32 / *max,
            // `hid_descriptor.rs` converts the raw bits with `as i32`; the
            // truncating cast reproduces that for 32-bit unsigned fields.
            Self::HidLogical { min, max } => normalize_logical(raw as i32, *min, *max),
        }
    }

    /// Raw value whose normalized value is `0.0`.
    pub fn center(&self) -> f64 {
        match self {
            Self::U8Centered => 127.5,
            Self::U12Centered => 2048.0,
            Self::I16 | Self::UnitRange { .. } => 0.0,
            Self::HidLogical { min, max } => {
                if *min < 0 && *max > 0 {
                    0.0
                } else {
                    (f64::from(*min) + f64::from(*max)) / 2.0
                }
            }
        }
    }

    /// Raw units per 1.0 of normalized value, on the positive side
    /// (`direction >= 0`) or the negative side (`direction < 0`).
    pub fn scale(&self, direction: i8) -> f64 {
        match self {
            Self::U8Centered => 127.5,
            Self::U12Centered => 2048.0,
            Self::I16 => {
                if direction < 0 {
                    32768.0
                } else {
                    32767.0
                }
            }
            Self::UnitRange { max } => f64::from(*max),
            Self::HidLogical { min, max } => {
                if max <= min {
                    1.0
                } else if *min < 0 && *max > 0 {
                    if direction < 0 {
                        f64::from(min.unsigned_abs())
                    } else {
                        f64::from(*max)
                    }
                } else {
                    (f64::from(*max) - f64::from(*min)) / 2.0
                }
            }
        }
    }

    /// Inverse of `normalize` (without clamping), piecewise linear around
    /// the center like the normalization itself.
    pub fn denormalize(&self, value: f64) -> f64 {
        let direction = if value < 0.0 { -1 } else { 1 };
        self.center() + value * self.scale(direction)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AxisLocation {
    pub name: String,
    pub field: FieldLocation,
    pub normalization: Normalization,
}

impl AxisLocation {
    pub fn read_normalized(&self, raw: &[u8]) -> Option<f32> {
        self.field
            .extract(raw)
            .map(|value| self.normalization.normalize(value))
    }
}

/// Locations of every logical button and axis of one report format. Index
/// `i` of `buttons` / `axes` corresponds to index `i` of `InputState`.
/// `None` marks a logical button that exists in the state but has no wire
/// location (e.g. the unused XInput bit 11 for GIP).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReportLayout {
    pub protocol: ControllerProtocol,
    pub buttons: Vec<Option<ButtonLocation>>,
    pub axes: Vec<Option<AxisLocation>>,
}

const DS4_REPORT_ID: u8 = 0x01;
const DS4_MIN_LENGTH: u16 = 10;
const DUALSENSE_REPORT_ID: u8 = 0x01;
const DUALSENSE_MIN_LENGTH: u16 = 12;
const SWITCH_REPORT_ID: u8 = 0x30;
const SWITCH_MIN_LENGTH: u16 = 13;
const XUSB_REPORT_TYPE: u8 = 0x00;
const XUSB_MIN_LENGTH: u16 = 14;
pub const GIP_CMD_INPUT: u8 = 0x20;
pub const GIP_CMD_GUIDE: u8 = 0x07;
const GIP_OPT_FRAGMENT: u8 = 0x80;
const GIP_INPUT_MIN_PAYLOAD: usize = 14;
/// Header length of GIP packets whose payload is shorter than 128 bytes.
pub const GIP_SHORT_HEADER_LEN: u32 = 4;

/// Hat values (0 = N, 1 = NE, ... 7 = NW, 8+ = neutral) that count as each
/// D-pad direction, as `protocol.rs::dpad_buttons` decodes them.
pub const HAT_UP: [u32; 3] = [0, 1, 7];
pub const HAT_RIGHT: [u32; 3] = [1, 2, 3];
pub const HAT_DOWN: [u32; 3] = [3, 4, 5];
pub const HAT_LEFT: [u32; 3] = [5, 6, 7];

fn u8_axis(name: &str, report_id: u8, min_length: u16, byte: u32) -> Option<AxisLocation> {
    Some(AxisLocation {
        name: name.to_string(),
        field: FieldLocation::new(Some(report_id), min_length, byte * 8, 8, false, 0, 255),
        normalization: Normalization::U8Centered,
    })
}

fn ps_hat_buttons(report_id: u8, min_length: u16, hat_bit: u32) -> Vec<Option<ButtonLocation>> {
    vec![
        Some(ButtonLocation::hat(
            "dpad_up", report_id, min_length, hat_bit, &HAT_UP,
        )),
        Some(ButtonLocation::hat(
            "dpad_right",
            report_id,
            min_length,
            hat_bit,
            &HAT_RIGHT,
        )),
        Some(ButtonLocation::hat(
            "dpad_down",
            report_id,
            min_length,
            hat_bit,
            &HAT_DOWN,
        )),
        Some(ButtonLocation::hat(
            "dpad_left",
            report_id,
            min_length,
            hat_bit,
            &HAT_LEFT,
        )),
    ]
}

/// DS4 USB report 0x01 (`protocol.rs::parse_ds4`).
pub fn ds4_layout() -> ReportLayout {
    let id = Some(DS4_REPORT_ID);
    let len = DS4_MIN_LENGTH;
    let mut buttons = ps_hat_buttons(DS4_REPORT_ID, len, 5 * 8);
    let names = [
        "square", "cross", "circle", "triangle", "l1", "r1", "l2", "r2", "share", "options", "l3",
        "r3", "ps", "touchpad",
    ];
    // byte 5 bits 4..7, byte 6 bits 0..7, byte 7 bits 0..1
    let bits = (44..48).chain(48..56).chain(56..58);
    for (name, bit) in names.iter().zip(bits) {
        buttons.push(Some(ButtonLocation::bit(name, id, len, bit)));
    }
    ReportLayout {
        protocol: ControllerProtocol::Ds4,
        buttons,
        axes: vec![
            u8_axis("left_x", DS4_REPORT_ID, len, 1),
            u8_axis("left_y", DS4_REPORT_ID, len, 2),
            u8_axis("right_x", DS4_REPORT_ID, len, 3),
            u8_axis("right_y", DS4_REPORT_ID, len, 4),
        ],
    }
}

/// DualSense USB report 0x01 (`protocol.rs::parse_dualsense`).
pub fn dualsense_layout() -> ReportLayout {
    let id = Some(DUALSENSE_REPORT_ID);
    let len = DUALSENSE_MIN_LENGTH;
    let mut buttons = ps_hat_buttons(DUALSENSE_REPORT_ID, len, 8 * 8);
    let names = [
        "square",
        "cross",
        "circle",
        "triangle",
        "l1",
        "r1",
        "l2",
        "r2",
        "create",
        "options",
        "l3",
        "r3",
        "ps",
        "touchpad",
        "mute",
        "byte10_bit3",
        "byte10_bit4",
        "byte10_bit5",
        "byte10_bit6",
        "byte10_bit7",
    ];
    // byte 8 bits 4..7, bytes 9..10
    let bits = (68..72).chain(72..88);
    for (name, bit) in names.iter().zip(bits) {
        buttons.push(Some(ButtonLocation::bit(name, id, len, bit)));
    }
    ReportLayout {
        protocol: ControllerProtocol::DualSense,
        buttons,
        axes: vec![
            u8_axis("left_x", DUALSENSE_REPORT_ID, len, 1),
            u8_axis("left_y", DUALSENSE_REPORT_ID, len, 2),
            u8_axis("right_x", DUALSENSE_REPORT_ID, len, 3),
            u8_axis("right_y", DUALSENSE_REPORT_ID, len, 4),
        ],
    }
}

/// Switch Pro standard full report 0x30 (`protocol.rs::parse_switch`).
pub fn switch_layout() -> ReportLayout {
    let id = Some(SWITCH_REPORT_ID);
    let len = SWITCH_MIN_LENGTH;
    let names = [
        "y",
        "x",
        "b",
        "a",
        "right_sr",
        "right_sl",
        "r",
        "zr",
        "minus",
        "plus",
        "right_stick",
        "left_stick",
        "home",
        "capture",
        "byte4_bit6",
        "charging_grip",
        "down",
        "up",
        "right",
        "left",
        "left_sr",
        "left_sl",
        "l",
        "zl",
    ];
    let buttons = names
        .iter()
        .zip(24..48)
        .map(|(name, bit)| Some(ButtonLocation::bit(name, id, len, bit)))
        .collect();
    let stick = |name: &str, bit: u32| {
        Some(AxisLocation {
            name: name.to_string(),
            field: FieldLocation::new(id, len, bit, 12, false, 0, 4095),
            normalization: Normalization::U12Centered,
        })
    };
    ReportLayout {
        protocol: ControllerProtocol::Switch,
        buttons,
        axes: vec![
            stick("left_x", 48),
            stick("left_y", 60),
            stick("right_x", 72),
            stick("right_y", 84),
        ],
    }
}

const XINPUT_BUTTON_NAMES: [&str; 16] = [
    "dpad_up",
    "dpad_down",
    "dpad_left",
    "dpad_right",
    "start",
    "back",
    "left_thumb",
    "right_thumb",
    "left_shoulder",
    "right_shoulder",
    "guide",
    "reserved_bit11",
    "a",
    "b",
    "x",
    "y",
];

/// XUSB (Xbox 360 wired) 20-byte input report `00 14 ...`: buttons u16 LE at
/// bytes 2-3, LT byte 4, RT byte 5, LX/LY/RX/RY i16 LE at bytes 6..13.
pub fn xusb_layout() -> ReportLayout {
    let id = Some(XUSB_REPORT_TYPE);
    let len = XUSB_MIN_LENGTH;
    let buttons = XINPUT_BUTTON_NAMES
        .iter()
        .enumerate()
        .map(|(index, name)| Some(ButtonLocation::bit(name, id, len, 16 + index as u32)))
        .collect();
    let stick = |name: &str, byte: u32| {
        Some(AxisLocation {
            name: name.to_string(),
            field: FieldLocation::new(id, len, byte * 8, 16, true, -32768, 32767),
            normalization: Normalization::I16,
        })
    };
    let trigger = |name: &str, byte: u32| {
        Some(AxisLocation {
            name: name.to_string(),
            field: FieldLocation::new(id, len, byte * 8, 8, false, 0, 255),
            normalization: Normalization::UnitRange { max: 255.0 },
        })
    };
    ReportLayout {
        protocol: ControllerProtocol::Xusb,
        buttons,
        axes: vec![
            stick("left_x", 6),
            stick("left_y", 8),
            stick("right_x", 10),
            stick("right_y", 12),
            trigger("left_trigger", 4),
            trigger("right_trigger", 5),
        ],
    }
}

/// Parses an XUSB 20-byte input report into the XInput state representation
/// used by `protocol.rs::parse_xinput_raw` (buttons in wButtons bit order,
/// axes `[lx, ly, rx, ry, lt, rt]`). Like xpad and SDL, only byte 0 == 0x00
/// is required; byte 1 (0x14) is not checked.
pub fn parse_xusb_report(raw: &[u8]) -> Option<InputState> {
    if raw.len() < XUSB_MIN_LENGTH as usize || raw[0] != XUSB_REPORT_TYPE {
        return None;
    }
    let mut state = [0_u8; 14];
    state[..12].copy_from_slice(&raw[2..14]);
    parse_xinput_raw(&state)
}

/// Header of a GIP packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GipHeader {
    pub command: u8,
    pub options: u8,
    pub sequence: u8,
    pub payload_length: usize,
    pub header_length: usize,
}

pub fn parse_gip_header(raw: &[u8]) -> Option<GipHeader> {
    if raw.len() < 4 {
        return None;
    }
    let mut length = 0_usize;
    let mut index = 3;
    let mut shift = 0;
    loop {
        let byte = *raw.get(index)?;
        length |= usize::from(byte & 0x7F) << shift;
        index += 1;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 21 {
            return None;
        }
    }
    Some(GipHeader {
        command: raw[0],
        options: raw[1],
        sequence: raw[2],
        payload_length: length,
        header_length: index,
    })
}

fn gip_payload<'a>(raw: &'a [u8], header: &GipHeader) -> &'a [u8] {
    let end = raw.len().min(header.header_length + header.payload_length);
    &raw[header.header_length.min(end)..end]
}

/// Builds the XInput-style state from a GIP 0x20 input payload.
/// The guide button (index 10) is not part of 0x20 and is set by the caller.
fn gip_input_state(payload: &[u8], guide: bool) -> Option<InputState> {
    if payload.len() < GIP_INPUT_MIN_PAYLOAD {
        return None;
    }
    let p0 = payload[0];
    let p1 = payload[1];
    let bit = |byte: u8, bit: u8| byte & (1 << bit) != 0;
    let buttons = vec![
        bit(p1, 0), // dpad up
        bit(p1, 1), // dpad down
        bit(p1, 2), // dpad left
        bit(p1, 3), // dpad right
        bit(p0, 2), // menu (start)
        bit(p0, 3), // view (back)
        bit(p1, 6), // left stick
        bit(p1, 7), // right stick
        bit(p1, 4), // LB
        bit(p1, 5), // RB
        guide,
        false,      // unused XInput bit 11
        bit(p0, 4), // A
        bit(p0, 5), // B
        bit(p0, 6), // X
        bit(p0, 7), // Y
    ];
    let u16_at = |offset: usize| u16::from_le_bytes([payload[offset], payload[offset + 1]]);
    let i16_at = |offset: usize| i16::from_le_bytes([payload[offset], payload[offset + 1]]);
    let trigger = Normalization::UnitRange { max: 1023.0 };
    let axes = vec![
        normalize_i16(i16_at(6)),
        normalize_i16(i16_at(8)),
        normalize_i16(i16_at(10)),
        normalize_i16(i16_at(12)),
        trigger.normalize(i64::from(u16_at(2))),
        trigger.normalize(i64::from(u16_at(4))),
    ];
    Some(InputState { buttons, axes })
}

/// Parses a GIP 0x07 guide-button packet. Returns the pressed state
/// (payload[0] bits 0-1, the union of the xpad and SDL rules).
pub fn parse_gip_guide(raw: &[u8]) -> Option<bool> {
    let header = parse_gip_header(raw)?;
    if header.command != GIP_CMD_GUIDE || header.options & GIP_OPT_FRAGMENT != 0 {
        return None;
    }
    let payload = gip_payload(raw, &header);
    payload.first().map(|value| value & 0x03 != 0)
}

/// GIP input layout (XInput button order, axes `[lx, ly, rx, ry, lt, rt]`)
/// for packets whose header is `header_len` bytes long (4 for payloads
/// shorter than 128 bytes).
pub fn gip_layout(header_len: u32) -> ReportLayout {
    let id = Some(GIP_CMD_INPUT);
    let len = (header_len as usize + GIP_INPUT_MIN_PAYLOAD) as u16;
    let p = |byte: u32, bit: u32| (header_len + byte) * 8 + bit;
    let input_bit =
        |name: &str, byte: u32, bit: u32| Some(ButtonLocation::bit(name, id, len, p(byte, bit)));
    let guide = ButtonLocation {
        name: "guide".to_string(),
        field: FieldLocation::new(
            Some(GIP_CMD_GUIDE),
            (GIP_SHORT_HEADER_LEN + 1) as u16,
            GIP_SHORT_HEADER_LEN * 8,
            2,
            false,
            0,
            3,
        ),
        pressed: PressedRule::Values {
            values: vec![1, 2, 3],
        },
    };
    let buttons = vec![
        input_bit("dpad_up", 1, 0),
        input_bit("dpad_down", 1, 1),
        input_bit("dpad_left", 1, 2),
        input_bit("dpad_right", 1, 3),
        input_bit("start", 0, 2),
        input_bit("back", 0, 3),
        input_bit("left_thumb", 1, 6),
        input_bit("right_thumb", 1, 7),
        input_bit("left_shoulder", 1, 4),
        input_bit("right_shoulder", 1, 5),
        Some(guide),
        None,
        input_bit("a", 0, 4),
        input_bit("b", 0, 5),
        input_bit("x", 0, 6),
        input_bit("y", 0, 7),
    ];
    let stick = |name: &str, byte: u32| {
        Some(AxisLocation {
            name: name.to_string(),
            field: FieldLocation::new(id, len, p(byte, 0), 16, true, -32768, 32767),
            normalization: Normalization::I16,
        })
    };
    let trigger = |name: &str, byte: u32| {
        Some(AxisLocation {
            name: name.to_string(),
            field: FieldLocation::new(id, len, p(byte, 0), 16, false, 0, 1023),
            normalization: Normalization::UnitRange { max: 1023.0 },
        })
    };
    ReportLayout {
        protocol: ControllerProtocol::Gip,
        buttons,
        axes: vec![
            stick("left_x", 6),
            stick("left_y", 8),
            stick("right_x", 10),
            stick("right_y", 12),
            trigger("left_trigger", 2),
            trigger("right_trigger", 4),
        ],
    }
}

/// Layout of a generic HID report, derived from the report descriptor.
/// `report_id` is `None` when the descriptor does not use report IDs.
pub fn generic_hid_layout(parser: &HidReportParser, report_id: Option<u8>) -> ReportLayout {
    let report_id = if parser.has_report_ids() {
        report_id
    } else {
        None
    };
    let min_length = parser
        .input_report_bytes_for(report_id)
        .unwrap_or(0)
        .min(u16::MAX as usize) as u16;
    let location = |field: &HidInputField| FieldLocation {
        report_id: field.report_id,
        min_length,
        bit_offset: field.bit_offset,
        bit_size: field.bit_size.min(32) as u8,
        is_signed: field.is_signed,
        logical_min: field.logical_min,
        logical_max: field.logical_max,
        usage_page: Some(field.usage_page),
        usage: Some(field.usage),
    };
    let buttons = parser
        .button_fields(report_id)
        .iter()
        .enumerate()
        .map(|(index, field)| {
            Some(ButtonLocation {
                name: format!("button_{}", index + 1),
                field: location(field),
                pressed: if field.bit_size == 1 {
                    PressedRule::Values { values: vec![1] }
                } else {
                    PressedRule::NonZero
                },
            })
        })
        .collect();
    let axes = parser
        .axis_fields(report_id)
        .iter()
        .map(|field| {
            Some(AxisLocation {
                name: format!("usage_{:02x}_{:02x}", field.usage_page, field.usage),
                field: location(field),
                normalization: Normalization::HidLogical {
                    min: field.logical_min,
                    max: field.logical_max,
                },
            })
        })
        .collect();
    ReportLayout {
        protocol: ControllerProtocol::GenericHid,
        buttons,
        axes,
    }
}

/// Copy of `hid_descriptor.rs::normalize_logical` (kept private there).
fn normalize_logical(value: i32, logical_min: i32, logical_max: i32) -> f32 {
    if logical_max <= logical_min {
        return 0.0;
    }

    if logical_min < 0 && logical_max > 0 {
        if value >= 0 {
            (value as f32 / logical_max as f32).clamp(-1.0, 1.0)
        } else {
            (value as f32 / logical_min.unsigned_abs() as f32).clamp(-1.0, 1.0)
        }
    } else {
        (((value - logical_min) as f32 / (logical_max - logical_min) as f32) * 2.0 - 1.0)
            .clamp(-1.0, 1.0)
    }
}

/// Stateful decoder for the reports of one controller interface.
#[derive(Clone, Debug)]
pub enum ReportDecoder {
    Ds4,
    DualSense,
    Switch,
    Xusb,
    Gip(GipDecoderState),
    GenericHid(HidReportParser),
}

#[derive(Clone, Debug, Default)]
pub struct GipDecoderState {
    last_input: Option<InputState>,
    guide: bool,
    header_len: Option<u32>,
}

impl ReportDecoder {
    pub fn new(protocol: ControllerProtocol, parser: Option<HidReportParser>) -> Option<Self> {
        Some(match protocol {
            ControllerProtocol::Ds4 => Self::Ds4,
            ControllerProtocol::DualSense => Self::DualSense,
            ControllerProtocol::Switch => Self::Switch,
            ControllerProtocol::Xusb => Self::Xusb,
            ControllerProtocol::Gip => Self::Gip(GipDecoderState::default()),
            ControllerProtocol::GenericHid => Self::GenericHid(parser?),
            ControllerProtocol::Unknown => return None,
        })
    }

    pub fn protocol(&self) -> ControllerProtocol {
        match self {
            Self::Ds4 => ControllerProtocol::Ds4,
            Self::DualSense => ControllerProtocol::DualSense,
            Self::Switch => ControllerProtocol::Switch,
            Self::Xusb => ControllerProtocol::Xusb,
            Self::Gip(_) => ControllerProtocol::Gip,
            Self::GenericHid(_) => ControllerProtocol::GenericHid,
        }
    }

    /// Decodes one report. Reports that do not belong to the input format
    /// of the protocol return `None` (they are ignored instead of falling
    /// back to the generic byte-wise parse of `protocol.rs`, so that the
    /// logical indices stay stable).
    pub fn decode(&mut self, raw: &[u8]) -> Option<InputState> {
        match self {
            Self::Ds4 => parse_ds4(raw),
            Self::DualSense => parse_dualsense(raw),
            Self::Switch => parse_switch(raw),
            Self::Xusb => parse_xusb_report(raw),
            Self::Gip(state) => {
                let header = parse_gip_header(raw)?;
                if header.options & GIP_OPT_FRAGMENT != 0 {
                    return None;
                }
                match header.command {
                    GIP_CMD_INPUT => {
                        let input = gip_input_state(gip_payload(raw, &header), state.guide)?;
                        state.header_len = Some(header.header_length as u32);
                        state.last_input = Some(input.clone());
                        Some(input)
                    }
                    GIP_CMD_GUIDE => {
                        state.guide = parse_gip_guide(raw)?;
                        let mut input = state.last_input.clone()?;
                        input.buttons[10] = state.guide;
                        state.last_input = Some(input.clone());
                        Some(input)
                    }
                    _ => None,
                }
            }
            Self::GenericHid(parser) => parser.parse_input_report(raw),
        }
    }

    /// Layout that applies to reports like `raw` (for generic HID the
    /// layout depends on the report ID in byte 0).
    pub fn layout_for_report(&self, raw: &[u8]) -> ReportLayout {
        match self {
            Self::Gip(state) => {
                // Header length of this very packet when it is an input
                // packet, otherwise the last one seen.
                let header_len = parse_gip_header(raw)
                    .filter(|header| header.command == GIP_CMD_INPUT)
                    .map(|header| header.header_length as u32)
                    .or(state.header_len)
                    .unwrap_or(GIP_SHORT_HEADER_LEN);
                gip_layout(header_len)
            }
            _ => self.layout_for_key(self.report_key(raw)),
        }
    }

    /// Report group a raw report belongs to. Logical indices of
    /// `InputState` are only comparable within one group: for generic HID
    /// with report IDs every report ID numbers its buttons and axes from 0
    /// (`HidReportParser::parse_input_report`), so the group is the report
    /// ID. All other decoders produce one layout (`None`); GIP 0x20 and 0x07
    /// packets share it.
    pub fn report_key(&self, raw: &[u8]) -> Option<u8> {
        match self {
            Self::GenericHid(parser) if parser.has_report_ids() => raw.first().copied(),
            _ => None,
        }
    }

    /// Report groups that carry buttons or axes, in descriptor order.
    pub fn input_report_keys(&self) -> Vec<Option<u8>> {
        match self {
            Self::GenericHid(parser) if parser.has_report_ids() => {
                let mut keys = Vec::new();
                for field in parser.input_fields() {
                    if field.kind != crate::hid_descriptor::HidFieldKind::Other
                        && !keys.contains(&field.report_id)
                    {
                        keys.push(field.report_id);
                    }
                }
                keys
            }
            _ => vec![None],
        }
    }

    /// True when the decoder distinguishes report IDs (`report_key` can be
    /// `Some`).
    pub fn uses_report_ids(&self) -> bool {
        matches!(self, Self::GenericHid(parser) if parser.has_report_ids())
    }

    /// Layout of one report group.
    pub fn layout_for_key(&self, key: Option<u8>) -> ReportLayout {
        match self {
            Self::Ds4 => ds4_layout(),
            Self::DualSense => dualsense_layout(),
            Self::Switch => switch_layout(),
            Self::Xusb => xusb_layout(),
            Self::Gip(state) => gip_layout(state.header_len.unwrap_or(GIP_SHORT_HEADER_LEN)),
            Self::GenericHid(parser) => generic_hid_layout(parser, key),
        }
    }

    /// Layout of the default input report (used when no sample is at hand).
    pub fn default_layout(&self) -> ReportLayout {
        let key = self.input_report_keys().first().copied().flatten();
        self.layout_for_key(key)
    }
}
