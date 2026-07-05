use anyhow::{anyhow, Context, Result};
use libloading::Library;

use crate::clock::timestamp_ns;
use crate::models::{InputState, ProtocolKind, RawSample};
use crate::protocol::parse_xinput_raw;

const ERROR_SUCCESS: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct XInputGamepad {
    w_buttons: u16,
    b_left_trigger: u8,
    b_right_trigger: u8,
    s_thumb_lx: i16,
    s_thumb_ly: i16,
    s_thumb_rx: i16,
    s_thumb_ry: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct XInputState {
    dw_packet_number: u32,
    gamepad: XInputGamepad,
}

type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XInputState) -> u32;

pub struct XInputApi {
    _library: Library,
    get_state: XInputGetStateFn,
}

impl XInputApi {
    pub fn load() -> Result<Self> {
        let mut last_error = None;
        for name in ["xinput1_4.dll", "xinput1_3.dll", "xinput9_1_0.dll"] {
            match unsafe { Library::new(name) } {
                Ok(library) => {
                    let get_state = unsafe {
                        *library
                            .get::<XInputGetStateFn>(b"XInputGetState\0")
                            .context("XInputGetState not found")?
                    };
                    return Ok(Self {
                        _library: library,
                        get_state,
                    });
                }
                Err(error) => last_error = Some(error),
            }
        }

        Err(anyhow!("XInput DLL load failed: {last_error:?}"))
    }

    pub fn connected_slots(&self) -> Vec<u32> {
        (0..4)
            .filter(|slot| self.get_state(*slot).is_ok())
            .collect()
    }

    pub fn get_state(&self, user_index: u32) -> Result<XInputSnapshot> {
        let mut state = XInputState::default();
        let code = unsafe { (self.get_state)(user_index, &mut state) };
        let timestamp_ns = timestamp_ns();
        if code != ERROR_SUCCESS {
            return Err(anyhow!("XInputGetState failed: {code}"));
        }

        let raw = state_to_raw(&state);
        Ok(XInputSnapshot {
            timestamp_ns,
            packet_number: state.dw_packet_number,
            state: parse_xinput_raw(&raw).unwrap_or_default(),
            raw,
        })
    }
}

pub struct XInputSnapshot {
    pub timestamp_ns: u64,
    pub packet_number: u32,
    pub raw: Vec<u8>,
    pub state: InputState,
}

pub fn snapshot_to_sample(snapshot: XInputSnapshot, sequence: u64) -> RawSample {
    RawSample {
        timestamp_ns: snapshot.timestamp_ns,
        sequence,
        protocol: ProtocolKind::XInput,
        raw_report: snapshot.raw,
        state: snapshot.state,
    }
}

fn state_to_raw(state: &XInputState) -> Vec<u8> {
    let gamepad = state.gamepad;
    let mut raw = Vec::with_capacity(14);
    raw.extend_from_slice(&gamepad.w_buttons.to_le_bytes());
    raw.push(gamepad.b_left_trigger);
    raw.push(gamepad.b_right_trigger);
    raw.extend_from_slice(&gamepad.s_thumb_lx.to_le_bytes());
    raw.extend_from_slice(&gamepad.s_thumb_ly.to_le_bytes());
    raw.extend_from_slice(&gamepad.s_thumb_rx.to_le_bytes());
    raw.extend_from_slice(&gamepad.s_thumb_ry.to_le_bytes());
    raw.extend_from_slice(&state.dw_packet_number.to_le_bytes()[..2]);
    raw
}
