use crate::hid_descriptor::HidReportParser;
use crate::models::{InputState, ProtocolKind};

pub fn classify_vid_pid(vid: u16, pid: u16) -> ProtocolKind {
    match (vid, pid) {
        (0x054C, 0x05C4) | (0x054C, 0x09CC) => ProtocolKind::Ds4,
        (0x054C, 0x0CE6) => ProtocolKind::DualSense,
        (0x057E, 0x2009) => ProtocolKind::Switch,
        _ => ProtocolKind::GenericHid,
    }
}

pub fn parse_report_with_descriptor(
    protocol: &ProtocolKind,
    raw: &[u8],
    descriptor: Option<&HidReportParser>,
) -> InputState {
    match protocol {
        ProtocolKind::Ds4 => parse_ds4(raw).unwrap_or_else(|| parse_generic(raw)),
        ProtocolKind::DualSense => parse_dualsense(raw).unwrap_or_else(|| parse_generic(raw)),
        ProtocolKind::Switch => parse_switch(raw).unwrap_or_else(|| parse_generic(raw)),
        ProtocolKind::XInput => parse_xinput_raw(raw).unwrap_or_default(),
        ProtocolKind::GenericHid | ProtocolKind::Unknown => descriptor
            .and_then(|parser| parser.parse_input_report(raw))
            .unwrap_or_else(|| parse_generic(raw)),
    }
}

fn parse_ds4(raw: &[u8]) -> Option<InputState> {
    if raw.len() < 10 || raw[0] != 0x01 {
        return None;
    }

    let axes = raw[1..5].iter().map(|v| normalize_u8(*v)).collect();
    let mut buttons = Vec::with_capacity(18);
    buttons.extend(dpad_buttons(raw[5] & 0x0F));
    for bit in 4..8 {
        buttons.push((raw[5] & (1 << bit)) != 0);
    }
    for bit in 0..8 {
        buttons.push((raw[6] & (1 << bit)) != 0);
    }
    for bit in 0..2 {
        buttons.push((raw[7] & (1 << bit)) != 0);
    }

    Some(InputState { buttons, axes })
}

fn parse_dualsense(raw: &[u8]) -> Option<InputState> {
    if raw.len() < 12 || raw[0] != 0x01 {
        return None;
    }

    let axes = raw[1..5].iter().map(|v| normalize_u8(*v)).collect();
    let mut buttons = Vec::with_capacity(24);
    buttons.extend(dpad_buttons(raw[8] & 0x0F));
    for bit in 4..8 {
        buttons.push((raw[8] & (1 << bit)) != 0);
    }
    for byte in &raw[9..11] {
        for bit in 0..8 {
            buttons.push((byte & (1 << bit)) != 0);
        }
    }

    Some(InputState { buttons, axes })
}

fn parse_switch(raw: &[u8]) -> Option<InputState> {
    if raw.len() < 13 || raw[0] != 0x30 {
        return None;
    }

    let mut buttons = Vec::with_capacity(24);
    for byte in &raw[3..6] {
        for bit in 0..8 {
            buttons.push((byte & (1 << bit)) != 0);
        }
    }

    let lx = u16::from(raw[6]) | (u16::from(raw[7] & 0x0F) << 8);
    let ly = (u16::from(raw[7]) >> 4) | (u16::from(raw[8]) << 4);
    let rx = u16::from(raw[9]) | (u16::from(raw[10] & 0x0F) << 8);
    let ry = (u16::from(raw[10]) >> 4) | (u16::from(raw[11]) << 4);
    let axes = vec![
        normalize_u12(lx),
        normalize_u12(ly),
        normalize_u12(rx),
        normalize_u12(ry),
    ];

    Some(InputState { buttons, axes })
}

pub fn parse_xinput_raw(raw: &[u8]) -> Option<InputState> {
    if raw.len() < 14 {
        return None;
    }

    let buttons_bits = u16::from_le_bytes([raw[0], raw[1]]);
    let mut buttons = Vec::with_capacity(16);
    for bit in 0..16 {
        buttons.push((buttons_bits & (1 << bit)) != 0);
    }

    let lt = raw[2] as f32 / 255.0;
    let rt = raw[3] as f32 / 255.0;
    let lx = normalize_i16(i16::from_le_bytes([raw[4], raw[5]]));
    let ly = normalize_i16(i16::from_le_bytes([raw[6], raw[7]]));
    let rx = normalize_i16(i16::from_le_bytes([raw[8], raw[9]]));
    let ry = normalize_i16(i16::from_le_bytes([raw[10], raw[11]]));

    Some(InputState {
        buttons,
        axes: vec![lx, ly, rx, ry, lt, rt],
    })
}

fn parse_generic(raw: &[u8]) -> InputState {
    let start = usize::from(raw.first().copied().unwrap_or(0) != 0);
    let mut buttons = Vec::with_capacity(raw.len() * 8);
    for byte in raw.iter().skip(start) {
        for bit in 0..8 {
            buttons.push((byte & (1 << bit)) != 0);
        }
    }

    let axes = raw.iter().skip(start).map(|v| normalize_u8(*v)).collect();
    InputState { buttons, axes }
}

fn dpad_buttons(hat: u8) -> [bool; 4] {
    let up = matches!(hat, 0 | 1 | 7);
    let right = matches!(hat, 1..=3);
    let down = matches!(hat, 3..=5);
    let left = matches!(hat, 5..=7);
    [up, right, down, left]
}

fn normalize_u8(value: u8) -> f32 {
    ((value as f32 - 127.5) / 127.5).clamp(-1.0, 1.0)
}

fn normalize_u12(value: u16) -> f32 {
    ((value as f32 - 2048.0) / 2048.0).clamp(-1.0, 1.0)
}

fn normalize_i16(value: i16) -> f32 {
    if value >= 0 {
        value as f32 / i16::MAX as f32
    } else {
        value as f32 / 32768.0
    }
    .clamp(-1.0, 1.0)
}
