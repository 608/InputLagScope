//! Typed bodies of the CH32H417 tester PC protocol. Every structure is encoded and decoded one byte
//! at a time in little-endian order; no structure is cast from memory.
//! Reserved fields are written as zero and ignored when read.
//!
//! Both directions are implemented for every structure; the tester-side
//! direction (decoding requests, encoding responses and events) is used by

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Type codes

pub const RESPONSE_FLAG: u8 = 0x80;

pub const CMD_HELLO: u8 = 0x01;
pub const CMD_GET_STATUS: u8 = 0x02;
pub const CMD_GET_DEVICE_INFO: u8 = 0x03;
pub const CMD_GET_OBJECT: u8 = 0x04;
pub const CMD_SET_HOST_PROFILE: u8 = 0x05;
pub const CMD_GET_HOST_PROFILE: u8 = 0x06;
pub const CMD_SET_VENDOR_TABLE: u8 = 0x07;
pub const CMD_GET_VENDOR_TABLE: u8 = 0x08;
pub const CMD_REENUMERATE: u8 = 0x09;
pub const CMD_TRIGGER_MANUAL: u8 = 0x0A;
pub const CMD_MONITOR_START: u8 = 0x0B;
pub const CMD_MONITOR_STOP: u8 = 0x0C;
pub const CMD_SET_WATCH: u8 = 0x0D;
pub const CMD_TRIAL_START: u8 = 0x0E;
pub const CMD_TRIAL_STOP: u8 = 0x0F;
pub const CMD_POLL_TEST_START: u8 = 0x10;
pub const CMD_POLL_TEST_STOP: u8 = 0x11;
pub const CMD_SET_CALIBRATION: u8 = 0x12;
pub const CMD_GET_CALIBRATION: u8 = 0x13;
pub const CMD_CALIBRATION_MODE: u8 = 0x14;
pub const CMD_ENTER_BOOTLOADER: u8 = 0x15;
pub const CMD_READ_OBJECT: u8 = 0x16;

pub const EVT_LOG: u8 = 0x40;
pub const EVT_DEVICE_ATTACHED: u8 = 0x41;
pub const EVT_DEVICE_DETACHED: u8 = 0x42;
pub const EVT_ENUM_PROGRESS: u8 = 0x43;
pub const EVT_DEVICE_READY: u8 = 0x44;
pub const EVT_MONITOR_REPORT: u8 = 0x45;
pub const EVT_TRIAL_RESULT: u8 = 0x46;
pub const EVT_RUN_DONE: u8 = 0x47;
pub const EVT_POLL_RESULT: u8 = 0x49;
pub const EVT_DROP_COUNTERS: u8 = 0x4A;
pub const EVT_RECOVERED: u8 = 0x4B;
pub const EVT_ERROR: u8 = 0x4C;

// Response status
pub const STATUS_OK: u8 = 0x00;
pub const STATUS_UNKNOWN_COMMAND: u8 = 0x01;
pub const STATUS_BAD_LENGTH: u8 = 0x02;
pub const STATUS_BAD_PARAM: u8 = 0x03;
pub const STATUS_BUSY: u8 = 0x04;
pub const STATUS_NO_DEVICE: u8 = 0x05;
pub const STATUS_NOT_READY: u8 = 0x06;
pub const STATUS_UNSUPPORTED: u8 = 0x07;
pub const STATUS_INTERNAL: u8 = 0x08;
pub const STATUS_SEQ_CONFLICT: u8 = 0x09;
pub const STATUS_STALE_OBJECT: u8 = 0x0A;

// Object kinds
pub const OBJ_DEVICE_DESCRIPTOR: u8 = 1;
pub const OBJ_CONFIG_DESCRIPTOR: u8 = 2;
pub const OBJ_REPORT_DESCRIPTOR: u8 = 3;
pub const OBJ_STRING_DESCRIPTOR: u8 = 4;
pub const OBJ_ENUM_TRACE: u8 = 6;
pub const OBJ_POLL_RAW: u8 = 7;
pub const OBJ_POLL_HISTOGRAM: u8 = 8;
pub const OBJ_POLL_LONG: u8 = 9;

pub const HELLO_MAGIC: u32 = 0x5454_414C; // 'LATT'
pub const HOST_PROTOCOL_VERSION: u8 = 1;
pub const MAX_VENDOR_ENTRIES: usize = 64;
pub const MAX_INTERFACES: usize = 8;
pub const MAX_REPORT_LENGTH: usize = 1024;
pub const MAX_READ_LENGTH: u16 = 1024;
/// Largest object the tester can hold (OBJECT_BUF).
pub const MAX_OBJECT_LENGTH: u32 = 64 * 1024;

// Request/response body lengths
pub const HELLO_REQUEST_LEN: usize = 12;
pub const HELLO_RESPONSE_LEN: usize = 62;
pub const STATUS_RESPONSE_LEN: usize = 52;
pub const DEVICE_INFO_HEADER_LEN: usize = 24;
pub const DEVICE_INFO_IFACE_LEN: usize = 12;
pub const GET_OBJECT_REQUEST_LEN: usize = 8;
pub const OBJECT_INFO_LEN: usize = 16;
pub const HOST_PROFILE_LEN: usize = 8;
pub const VENDOR_TABLE_HEADER_LEN: usize = 2;
pub const VENDOR_ENTRY_LEN: usize = 8;
pub const MONITOR_START_LEN: usize = 8;
pub const SET_WATCH_LEN: usize = 32;
pub const TRIAL_START_LEN: usize = 24;
pub const RUN_ID_REQUEST_LEN: usize = 4;
pub const POLL_TEST_START_LEN: usize = 16;
pub const CALIBRATION_LEN: usize = 28;
pub const READ_OBJECT_REQUEST_LEN: usize = 8;
pub const READ_OBJECT_HEADER_LEN: usize = 16;

// Event body lengths
pub const DEVICE_EVENT_LEN: usize = 8;
pub const ENUM_PROGRESS_LEN: usize = 8;
pub const DEVICE_READY_LEN: usize = 8;
pub const MONITOR_REPORT_HEADER_LEN: usize = 16;
pub const TRIAL_RESULT_HEADER_LEN: usize = 94;
pub const RUN_DONE_LEN: usize = 28;
pub const POLL_RESULT_LEN: usize = 88;
pub const DROP_COUNTERS_LEN: usize = 8;
pub const RECOVERED_LEN: usize = 4;
pub const ERROR_EVENT_LEN: usize = 8;

pub const ENUM_TRACE_V1_HEADER_LEN: usize = 8;
pub const ENUM_TRACE_V2_HEADER_LEN: usize = 16;
pub const ENUM_RECORD_V1_FIXED_LEN: usize = 45;
pub const ENUM_RECORD_V2_FIXED_LEN: usize = 53;

/// Number of fail kinds in TRIAL_RESULT / RUN_DONE (0..=6).
pub const FAIL_KIND_COUNT: usize = 7;

pub fn command_name(command: u8) -> &'static str {
    match command & !RESPONSE_FLAG {
        CMD_HELLO => "HELLO",
        CMD_GET_STATUS => "GET_STATUS",
        CMD_GET_DEVICE_INFO => "GET_DEVICE_INFO",
        CMD_GET_OBJECT => "GET_OBJECT",
        CMD_SET_HOST_PROFILE => "SET_HOST_PROFILE",
        CMD_GET_HOST_PROFILE => "GET_HOST_PROFILE",
        CMD_SET_VENDOR_TABLE => "SET_VENDOR_TABLE",
        CMD_GET_VENDOR_TABLE => "GET_VENDOR_TABLE",
        CMD_REENUMERATE => "REENUMERATE",
        CMD_TRIGGER_MANUAL => "TRIGGER_MANUAL",
        CMD_MONITOR_START => "MONITOR_START",
        CMD_MONITOR_STOP => "MONITOR_STOP",
        CMD_SET_WATCH => "SET_WATCH",
        CMD_TRIAL_START => "TRIAL_START",
        CMD_TRIAL_STOP => "TRIAL_STOP",
        CMD_POLL_TEST_START => "POLL_TEST_START",
        CMD_POLL_TEST_STOP => "POLL_TEST_STOP",
        CMD_SET_CALIBRATION => "SET_CALIBRATION",
        CMD_GET_CALIBRATION => "GET_CALIBRATION",
        CMD_CALIBRATION_MODE => "CALIBRATION_MODE",
        CMD_ENTER_BOOTLOADER => "ENTER_BOOTLOADER",
        CMD_READ_OBJECT => "READ_OBJECT",
        _ => "UNKNOWN",
    }
}

pub fn status_name(status: u8) -> &'static str {
    match status {
        STATUS_OK => "OK",
        STATUS_UNKNOWN_COMMAND => "UNKNOWN_COMMAND",
        STATUS_BAD_LENGTH => "BAD_LENGTH",
        STATUS_BAD_PARAM => "BAD_PARAM",
        STATUS_BUSY => "BUSY",
        STATUS_NO_DEVICE => "NO_DEVICE",
        STATUS_NOT_READY => "NOT_READY",
        STATUS_UNSUPPORTED => "UNSUPPORTED",
        STATUS_INTERNAL => "INTERNAL",
        STATUS_SEQ_CONFLICT => "SEQ_CONFLICT",
        STATUS_STALE_OBJECT => "STALE_OBJECT",
        _ => "UNKNOWN_STATUS",
    }
}

pub fn speed_name(speed: u8) -> &'static str {
    match speed {
        0 => "none",
        1 => "low",
        2 => "full",
        3 => "high",
        _ => "unknown",
    }
}

pub fn host_state_name(state: u8) -> &'static str {
    match state {
        0 => "starting",
        1 => "disconnected",
        2 => "debounce",
        3 => "reset",
        4 => "enumerating",
        5 => "configured",
        6 => "recovering",
        7 => "error_stopped",
        _ => "unknown",
    }
}

pub fn classification_name(classification: u8) -> &'static str {
    match classification {
        0 => "other",
        1 => "composite",
        2 => "single_hid",
        3 => "single_xusb",
        4 => "gip",
        _ => "unknown",
    }
}

pub fn fail_kind_name(kind: u8) -> &'static str {
    match kind {
        0 => "success",
        1 => "timeout",
        2 => "not_released",
        3 => "confirm_reverted",
        4 => "event_queue_full",
        5 => "disconnected",
        6 => "aborted",
        _ => "unknown",
    }
}

pub fn c1_status_name(status: u8) -> &'static str {
    match status {
        0 => "measured_fs",
        1 => "estimated_from_fs",
        2 => "unknown",
        _ => "invalid",
    }
}

pub fn measurement_kind_name(kind: u8) -> &'static str {
    match kind {
        0 => "t1_wire_minus_t0_wire_estimate",
        _ => "unknown",
    }
}

pub fn completion_reason_name(reason: u8) -> &'static str {
    match reason {
        0 => "normal",
        1 => "pc_abort",
        2 => "disconnected",
        3 => "internal_error",
        4 => "event_overflow",
        _ => "unknown",
    }
}

pub fn run_kind_name(kind: u8) -> &'static str {
    match kind {
        0 => "none",
        1 => "trial",
        2 => "poll_test",
        3 => "calibration",
        _ => "unknown",
    }
}

pub fn error_code_name(code: u16) -> &'static str {
    match code {
        1 => "EVENT_OVERFLOW",
        2 => "IPC_ABI_MISMATCH",
        3 => "IRQ_ROUTE_FAILED",
        4 => "TIMER_CLOCK_WRONG",
        5 => "HUB_UNSUPPORTED",
        6 => "ENUM_FAILED",
        0x100..=u16::MAX => "INTERNAL",
        _ => "UNKNOWN",
    }
}

// ---------------------------------------------------------------------------
// Byte-wise helpers

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DecodeError {
    #[error("{name}: expected {expected} bytes, got {actual}")]
    Length {
        name: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("{name}: {message}")]
    Invalid { name: &'static str, message: String },
}

fn expect_len(name: &'static str, data: &[u8], expected: usize) -> Result<(), DecodeError> {
    if data.len() != expected {
        return Err(DecodeError::Length {
            name,
            expected,
            actual: data.len(),
        });
    }
    Ok(())
}

fn expect_min_len(name: &'static str, data: &[u8], minimum: usize) -> Result<(), DecodeError> {
    if data.len() < minimum {
        return Err(DecodeError::Length {
            name,
            expected: minimum,
            actual: data.len(),
        });
    }
    Ok(())
}

fn invalid(name: &'static str, message: impl Into<String>) -> DecodeError {
    DecodeError::Invalid {
        name,
        message: message.into(),
    }
}

/// Little-endian reader over a slice whose length was already validated.
pub(crate) struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    pub(crate) fn bytes(&mut self, len: usize) -> &'a [u8] {
        let out = &self.data[self.pos..self.pos + len];
        self.pos += len;
        out
    }


    pub(crate) fn u8(&mut self) -> u8 {
        let value = self.data[self.pos];
        self.pos += 1;
        value
    }


    pub(crate) fn u16(&mut self) -> u16 {
        let value = u16::from(self.data[self.pos]) | (u16::from(self.data[self.pos + 1]) << 8);
        self.pos += 2;
        value
    }

    pub(crate) fn u32(&mut self) -> u32 {
        let mut value = 0_u32;
        for index in 0..4 {
            value |= u32::from(self.data[self.pos + index]) << (8 * index);
        }
        self.pos += 4;
        value
    }

    pub(crate) fn i32(&mut self) -> i32 {
        self.u32() as i32
    }

    pub(crate) fn u64(&mut self) -> u64 {
        let mut value = 0_u64;
        for index in 0..8 {
            value |= u64::from(self.data[self.pos + index]) << (8 * index);
        }
        self.pos += 8;
        value
    }


    pub(crate) fn array<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0_u8; N];
        out.copy_from_slice(self.bytes(N));
        out
    }

    pub(crate) fn skip(&mut self, len: usize) {
        self.pos += len;
    }

    pub(crate) fn position(&self) -> usize {
        self.pos
    }
}

/// Little-endian writer.
#[derive(Default)]
pub(crate) struct ByteWriter {
    buf: Vec<u8>,
}

impl ByteWriter {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
        }
    }

    pub(crate) fn u8(&mut self, value: u8) -> &mut Self {
        self.buf.push(value);
        self
    }

    pub(crate) fn i8(&mut self, value: i8) -> &mut Self {
        self.buf.push(value as u8);
        self
    }



    pub(crate) fn u16(&mut self, value: u16) -> &mut Self {
        self.buf.push(value as u8);
        self.buf.push((value >> 8) as u8);
        self
    }

    pub(crate) fn u32(&mut self, value: u32) -> &mut Self {
        for index in 0..4 {
            self.buf.push((value >> (8 * index)) as u8);
        }
        self
    }

    pub(crate) fn i32(&mut self, value: i32) -> &mut Self {
        self.u32(value as u32)
    }

    pub(crate) fn u64(&mut self, value: u64) -> &mut Self {
        for index in 0..8 {
            self.buf.push((value >> (8 * index)) as u8);
        }
        self
    }


    pub(crate) fn zeros(&mut self, len: usize) -> &mut Self {
        self.buf.resize(self.buf.len() + len, 0);
        self
    }

    pub(crate) fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

// ---------------------------------------------------------------------------
// HELLO

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloRequest {
    pub host_protocol_version: u8,
    pub host_nonce: u64,
}

impl HelloRequest {
    pub fn encode(&self) -> Vec<u8> {
        ByteWriter::with_capacity(HELLO_REQUEST_LEN)
            .u8(self.host_protocol_version)
            .zeros(3)
            .u64(self.host_nonce)
            .finish()
    }

}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloResponse {
    pub magic: u32,
    pub protocol_version: u8,
    pub fw_major: u16,
    pub fw_minor: u16,
    pub build: [u8; 12],
    pub uuid: [u8; 16],
    pub capabilities: u32,
    pub max_payload: u16,
    pub max_poll_raw_count: u32,
    pub host_nonce_echo: u64,
    pub session_id: u32,
}

impl HelloResponse {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("HELLO response", data, HELLO_RESPONSE_LEN)?;
        let mut r = ByteReader::new(data);
        let magic = r.u32();
        let protocol_version = r.u8();
        r.skip(1);
        let fw_major = r.u16();
        let fw_minor = r.u16();
        let build = r.array::<12>();
        let uuid = r.array::<16>();
        let capabilities = r.u32();
        let max_payload = r.u16();
        r.skip(2);
        Ok(Self {
            magic,
            protocol_version,
            fw_major,
            fw_minor,
            build,
            uuid,
            capabilities,
            max_payload,
            max_poll_raw_count: r.u32(),
            host_nonce_echo: r.u64(),
            session_id: r.u32(),
        })
    }

    pub fn uuid_hex(&self) -> String {
        hex_string(&self.uuid)
    }

    pub fn build_string(&self) -> String {
        self.build
            .iter()
            .take_while(|byte| **byte != 0)
            .map(|byte| {
                if byte.is_ascii_graphic() || *byte == b' ' {
                    *byte as char
                } else {
                    '?'
                }
            })
            .collect()
    }
}

pub const CAP_MONITOR: u32 = 1 << 0;
pub const CAP_TRIAL: u32 = 1 << 1;
pub const CAP_POLL_TEST: u32 = 1 << 2;
pub const CAP_ENUM_TRACE: u32 = 1 << 3;
pub const CAP_CALIBRATION_MODE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// GET_STATUS

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StatusResponse {
    pub host_state: u8,
    pub speed: u8,
    pub device_generation: u32,
    pub enum_error: u32,
    pub sched_late_count: u32,
    pub sched_late_max: u32,
    pub txn_max_ticks: u32,
    pub recover_count: u32,
    pub drop_monitor: u32,
    pub drop_log: u32,
    pub now: u64,
    pub active_run_id: u32,
    pub run_kind: u8,
}

impl StatusResponse {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("GET_STATUS response", data, STATUS_RESPONSE_LEN)?;
        let mut r = ByteReader::new(data);
        let host_state = r.u8();
        let speed = r.u8();
        r.skip(2);
        Ok(Self {
            host_state,
            speed,
            device_generation: r.u32(),
            enum_error: r.u32(),
            sched_late_count: r.u32(),
            sched_late_max: r.u32(),
            txn_max_ticks: r.u32(),
            recover_count: r.u32(),
            drop_monitor: r.u32(),
            drop_log: r.u32(),
            now: r.u64(),
            active_run_id: r.u32(),
            run_kind: r.u8(),
        })
    }
}

// ---------------------------------------------------------------------------
// GET_DEVICE_INFO

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub iface_number: u8,
    pub iface_class: u8,
    pub iface_subclass: u8,
    pub iface_protocol: u8,
    pub ep_in: u8,
    pub ep_out: u8,
    pub in_max_packet: u16,
    pub in_interval: u8,
    pub alt_setting: u8,
    pub report_desc_len: u16,
}

impl InterfaceInfo {

    fn read(r: &mut ByteReader<'_>) -> Self {
        Self {
            iface_number: r.u8(),
            iface_class: r.u8(),
            iface_subclass: r.u8(),
            iface_protocol: r.u8(),
            ep_in: r.u8(),
            ep_out: r.u8(),
            in_max_packet: r.u16(),
            in_interval: r.u8(),
            alt_setting: r.u8(),
            report_desc_len: r.u16(),
        }
    }

    pub fn is_hid(&self) -> bool {
        self.iface_class == 0x03
    }

    /// XUSB gamepad interface (class 0xFF, subclass 0x5D, protocol 0x01).
    pub fn is_xusb_gamepad(&self) -> bool {
        self.iface_class == 0xFF && self.iface_subclass == 0x5D && self.iface_protocol == 0x01
    }

    /// GIP interface (class 0xFF, subclass 0x47, protocol 0xD0).
    pub fn is_gip(&self) -> bool {
        self.iface_class == 0xFF && self.iface_subclass == 0x47 && self.iface_protocol == 0xD0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfoResponse {
    pub device_generation: u32,
    pub speed: u8,
    pub address: u8,
    pub vid: u16,
    pub pid: u16,
    pub bcd_usb: u16,
    pub bcd_device: u16,
    pub device_class: u8,
    pub classification: u8,
    pub num_interfaces: u8,
    pub vendor_code_state: u8,
    pub vendor_code: u8,
    pub interfaces: Vec<InterfaceInfo>,
}

impl DeviceInfoResponse {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        const NAME: &str = "GET_DEVICE_INFO response";
        expect_min_len(NAME, data, DEVICE_INFO_HEADER_LEN)?;
        let count = usize::from(data[19]);
        if count > MAX_INTERFACES {
            return Err(invalid(NAME, format!("iface_count {count} exceeds 8")));
        }
        expect_len(
            NAME,
            data,
            DEVICE_INFO_HEADER_LEN + DEVICE_INFO_IFACE_LEN * count,
        )?;
        let mut r = ByteReader::new(data);
        let device_generation = r.u32();
        let speed = r.u8();
        let address = r.u8();
        let vid = r.u16();
        let pid = r.u16();
        let bcd_usb = r.u16();
        let bcd_device = r.u16();
        let device_class = r.u8();
        let classification = r.u8();
        let num_interfaces = r.u8();
        let vendor_code_state = r.u8();
        let vendor_code = r.u8();
        r.skip(1 + 4);
        let interfaces = (0..count).map(|_| InterfaceInfo::read(&mut r)).collect();
        Ok(Self {
            device_generation,
            speed,
            address,
            vid,
            pid,
            bcd_usb,
            bcd_device,
            device_class,
            classification,
            num_interfaces,
            vendor_code_state,
            vendor_code,
            interfaces,
        })
    }
}

// ---------------------------------------------------------------------------
// GET_OBJECT / READ_OBJECT

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetObjectRequest {
    pub kind: u8,
    pub index: u8,
    pub source_id: u32,
}

impl GetObjectRequest {
    pub fn encode(&self) -> Vec<u8> {
        ByteWriter::with_capacity(GET_OBJECT_REQUEST_LEN)
            .u8(self.kind)
            .u8(self.index)
            .zeros(2)
            .u32(self.source_id)
            .finish()
    }

}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectInfo {
    pub object_id: u16,
    pub kind: u8,
    pub index: u8,
    pub source_id: u32,
    pub total_length: u32,
    pub crc32: u32,
}

impl ObjectInfo {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("GET_OBJECT response", data, OBJECT_INFO_LEN)?;
        let mut r = ByteReader::new(data);
        Ok(Self {
            object_id: r.u16(),
            kind: r.u8(),
            index: r.u8(),
            source_id: r.u32(),
            total_length: r.u32(),
            crc32: r.u32(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadObjectRequest {
    pub object_id: u16,
    pub max_length: u16,
    pub offset: u32,
}

impl ReadObjectRequest {
    pub fn encode(&self) -> Vec<u8> {
        ByteWriter::with_capacity(READ_OBJECT_REQUEST_LEN)
            .u16(self.object_id)
            .u16(self.max_length)
            .u32(self.offset)
            .finish()
    }

}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadObjectResponse {
    pub object_id: u16,
    pub offset: u32,
    pub total_length: u32,
    pub crc32: u32,
    pub data: Vec<u8>,
}

impl ReadObjectResponse {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        const NAME: &str = "READ_OBJECT response";
        expect_min_len(NAME, data, READ_OBJECT_HEADER_LEN)?;
        let mut r = ByteReader::new(data);
        let object_id = r.u16();
        let data_length = usize::from(r.u16());
        expect_len(NAME, data, READ_OBJECT_HEADER_LEN + data_length)?;
        Ok(Self {
            object_id,
            offset: r.u32(),
            total_length: r.u32(),
            crc32: r.u32(),
            data: r.bytes(data_length).to_vec(),
        })
    }
}

// ---------------------------------------------------------------------------
// SET_HOST_PROFILE / GET_HOST_PROFILE

/// Host profile. Values are kept as the raw protocol codes:
/// byte 0 reserved (always 0); post_requests 0/1; interval_mode
/// 0 = round down to a power of two, 1 = descriptor value; reset2_policy 0;
/// app_init_switch 0 none / 1 input_minimal / 2 sdl_open;
/// app_init_sony 0 none / 2 sdl_open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProfile {
    pub post_requests: u8,
    pub interval_mode: u8,
    pub reset2_policy: u8,
    pub app_init_switch: u8,
    pub app_init_sony: u8,
}

impl Default for HostProfile {
    fn default() -> Self {
        Self {
            post_requests: 1,
            interval_mode: 0,
            reset2_policy: 0,
            app_init_switch: 2,
            app_init_sony: 0,
        }
    }
}

impl HostProfile {
    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("host profile", data, HOST_PROFILE_LEN)?;
        let mut r = ByteReader::new(data);
        let _reserved = r.u8();
        Ok(Self {
            post_requests: r.u8(),
            interval_mode: r.u8(),
            reset2_policy: r.u8(),
            app_init_switch: r.u8(),
            app_init_sony: r.u8(),
        })
    }
}

// ---------------------------------------------------------------------------
// SET_VENDOR_TABLE / GET_VENDOR_TABLE

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendorEntry {
    pub vid: u16,
    pub pid: u16,
    pub bcd_device: u16,
    /// 1 = vendor code present, 2 = no MS OS descriptor.
    pub state: u8,
    pub vendor_code: u8,
}

impl VendorEntry {
    pub fn key(&self) -> (u16, u16, u16) {
        (self.vid, self.pid, self.bcd_device)
    }
}

pub fn encode_vendor_table(entries: &[VendorEntry]) -> Vec<u8> {
    let mut w =
        ByteWriter::with_capacity(VENDOR_TABLE_HEADER_LEN + VENDOR_ENTRY_LEN * entries.len());
    w.u8(entries.len() as u8).zeros(1);
    for entry in entries {
        w.u16(entry.vid)
            .u16(entry.pid)
            .u16(entry.bcd_device)
            .u8(entry.state)
            .u8(entry.vendor_code);
    }
    w.finish()
}

pub fn decode_vendor_table(data: &[u8]) -> Result<Vec<VendorEntry>, DecodeError> {
    const NAME: &str = "vendor table";
    expect_min_len(NAME, data, VENDOR_TABLE_HEADER_LEN)?;
    let count = usize::from(data[0]);
    if count > MAX_VENDOR_ENTRIES {
        return Err(invalid(NAME, format!("count {count} exceeds 64")));
    }
    expect_len(
        NAME,
        data,
        VENDOR_TABLE_HEADER_LEN + VENDOR_ENTRY_LEN * count,
    )?;
    let mut r = ByteReader::at(data, VENDOR_TABLE_HEADER_LEN);
    Ok((0..count)
        .map(|_| VendorEntry {
            vid: r.u16(),
            pid: r.u16(),
            bcd_device: r.u16(),
            state: r.u8(),
            vendor_code: r.u8(),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// TRIGGER_MANUAL

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerManual {
    pub assert_mask: u8,
    pub release_mask: u8,
}

impl TriggerManual {
    pub fn encode(&self) -> Vec<u8> {
        vec![self.assert_mask, self.release_mask]
    }

}

// ---------------------------------------------------------------------------
// MONITOR_START

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorStart {
    pub iface: u8,
    pub ep_in: u8,
    pub change_only: u8,
    pub min_interval_us: u32,
}

impl MonitorStart {
    pub fn encode(&self) -> Vec<u8> {
        ByteWriter::with_capacity(MONITOR_START_LEN)
            .u8(self.iface)
            .u8(self.ep_in)
            .u8(self.change_only)
            .zeros(1)
            .u32(self.min_interval_us)
            .finish()
    }

}

// ---------------------------------------------------------------------------
// SET_WATCH

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WatchRule {
    /// kind 0: pressed when the value equals one of 1..=4 values.
    Values { values: Vec<u32> },
    /// kind 1: axis rule.
    Axis {
        neutral: i32,
        threshold: u32,
        direction: i8,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchSpec {
    pub iface: u8,
    pub ep_in: u8,
    /// 0 = no condition, 1 = byte 0 must equal `report_id`.
    pub report_id_mode: u8,
    pub report_id: u8,
    pub min_length: u16,
    pub bit_offset: u16,
    pub bit_size: u8,
    pub is_signed: bool,
    pub rule: WatchRule,
}

impl WatchSpec {
    /// Range checks (the tester answers BAD_PARAM otherwise).
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=32).contains(&self.bit_size) {
            return Err(format!("bit_size {} is outside 1..=32", self.bit_size));
        }
        if usize::from(self.min_length) > MAX_REPORT_LENGTH {
            return Err(format!("min_length {} exceeds 1024", self.min_length));
        }
        if u32::from(self.bit_offset) + u32::from(self.bit_size) > u32::from(self.min_length) * 8 {
            return Err(format!(
                "bit_offset {} + bit_size {} exceeds min_length {} bytes",
                self.bit_offset, self.bit_size, self.min_length
            ));
        }
        if self.report_id_mode > 1 {
            return Err(format!(
                "report_id_mode {} is not 0 or 1",
                self.report_id_mode
            ));
        }
        if let WatchRule::Values { values } = &self.rule {
            if values.is_empty() || values.len() > 4 {
                return Err(format!("value_count {} is outside 1..=4", values.len()));
            }
        }
        Ok(())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = ByteWriter::with_capacity(SET_WATCH_LEN);
        w.u8(self.iface)
            .u8(self.ep_in)
            .u8(self.report_id_mode)
            .u8(self.report_id)
            .u16(self.min_length)
            .u8(match self.rule {
                WatchRule::Values { .. } => 0,
                WatchRule::Axis { .. } => 1,
            })
            .zeros(1)
            .u16(self.bit_offset)
            .u8(self.bit_size)
            .u8(u8::from(self.is_signed));
        match &self.rule {
            WatchRule::Values { values } => {
                w.u8(values.len().min(4) as u8).zeros(3);
                for index in 0..4 {
                    w.u32(values.get(index).copied().unwrap_or(0));
                }
            }
            WatchRule::Axis {
                neutral,
                threshold,
                direction,
            } => {
                w.i32(*neutral).u32(*threshold).i8(*direction).zeros(11);
            }
        }
        w.finish()
    }


}

// ---------------------------------------------------------------------------
// TRIAL_START / TRIAL_STOP, 4.12 POLL_TEST_START / POLL_TEST_STOP

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrialStart {
    pub run_id: u32,
    pub trigger_mask: u8,
    pub measure_release: u8,
    pub settle_reports: u8,
    pub count: u16,
    pub timeout_ms: u16,
    pub release_timeout_ms: u16,
    pub inter_trial_ms: u16,
    pub random_max_us: u32,
    pub seed: u32,
}

impl TrialStart {
    pub fn encode(&self) -> Vec<u8> {
        ByteWriter::with_capacity(TRIAL_START_LEN)
            .u32(self.run_id)
            .u8(self.trigger_mask)
            .u8(self.measure_release)
            .u8(self.settle_reports)
            .zeros(1)
            .u16(self.count)
            .u16(self.timeout_ms)
            .u16(self.release_timeout_ms)
            .u16(self.inter_trial_ms)
            .u32(self.random_max_us)
            .u32(self.seed)
            .finish()
    }

}

/// Body of TRIAL_STOP and POLL_TEST_STOP.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunIdRequest {
    pub run_id: u32,
}

impl RunIdRequest {
    pub fn encode(&self) -> Vec<u8> {
        ByteWriter::with_capacity(RUN_ID_REQUEST_LEN)
            .u32(self.run_id)
            .finish()
    }

}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollTestStart {
    pub run_id: u32,
    pub iface: u8,
    pub ep_in: u8,
    pub keep_raw: u8,
    pub duration_ms: u32,
    pub raw_max: u32,
}

impl PollTestStart {
    pub fn encode(&self) -> Vec<u8> {
        ByteWriter::with_capacity(POLL_TEST_START_LEN)
            .u32(self.run_id)
            .u8(self.iface)
            .u8(self.ep_in)
            .u8(self.keep_raw)
            .zeros(1)
            .u32(self.duration_ms)
            .u32(self.raw_max)
            .finish()
    }

}

// ---------------------------------------------------------------------------
// SET_CALIBRATION / GET_CALIBRATION, 4.14 CALIBRATION_MODE, 4.15

pub const CAL_VALID_C0: u8 = 1 << 0;
pub const C1_UNCERTAINTY_UNKNOWN: u32 = 0xFFFF_FFFF;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calibration {
    pub c0: i32,
    pub c_m: i32,
    pub c1_fs: i32,
    pub c0_uncertainty: u32,
    pub c_m_uncertainty: u32,
    /// 0xFFFFFFFF = unknown.
    pub c1_uncertainty: u32,
    /// bit0 c0, bit1 c_m, bit2 c1_fs.
    pub valid: u8,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            c0: 0,
            c_m: 0,
            c1_fs: 0,
            c0_uncertainty: 0,
            c_m_uncertainty: 0,
            c1_uncertainty: C1_UNCERTAINTY_UNKNOWN,
            valid: 0,
        }
    }
}

impl Calibration {
    pub fn encode(&self) -> Vec<u8> {
        ByteWriter::with_capacity(CALIBRATION_LEN)
            .i32(self.c0)
            .i32(self.c_m)
            .i32(self.c1_fs)
            .u32(self.c0_uncertainty)
            .u32(self.c_m_uncertainty)
            .u32(self.c1_uncertainty)
            .u8(self.valid)
            .zeros(3)
            .finish()
    }

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("calibration", data, CALIBRATION_LEN)?;
        let mut r = ByteReader::new(data);
        Ok(Self {
            c0: r.i32(),
            c_m: r.i32(),
            c1_fs: r.i32(),
            c0_uncertainty: r.u32(),
            c_m_uncertainty: r.u32(),
            c1_uncertainty: r.u32(),
            valid: r.u8(),
        })
    }

    pub fn c0_valid(&self) -> bool {
        self.valid & CAL_VALID_C0 != 0
    }
}

// ---------------------------------------------------------------------------
// 6. Events

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEvent {
    pub level: u8,
    pub text: String,
}

impl LogEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_min_len("LOG event", data, 1)?;
        Ok(Self {
            level: data[0],
            text: data[1..]
                .iter()
                .map(|byte| if byte.is_ascii() { *byte as char } else { '?' })
                .collect(),
        })
    }
}

/// Body of DEVICE_ATTACHED (0x41) and DEVICE_DETACHED (0x42).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEvent {
    pub device_generation: u32,
    pub speed: u8,
}

impl DeviceEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("DEVICE_ATTACHED/DETACHED event", data, DEVICE_EVENT_LEN)?;
        let mut r = ByteReader::new(data);
        Ok(Self {
            device_generation: r.u32(),
            speed: r.u8(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumProgressEvent {
    pub device_generation: u32,
    pub step: u16,
    /// 0 = success, otherwise a failure code.
    pub result: u8,
}

impl EnumProgressEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("ENUM_PROGRESS event", data, ENUM_PROGRESS_LEN)?;
        let mut r = ByteReader::new(data);
        Ok(Self {
            device_generation: r.u32(),
            step: r.u16(),
            result: r.u8(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceReadyEvent {
    pub device_generation: u32,
    pub classification: u8,
}

impl DeviceReadyEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("DEVICE_READY event", data, DEVICE_READY_LEN)?;
        let mut r = ByteReader::new(data);
        Ok(Self {
            device_generation: r.u32(),
            classification: r.u8(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorReportEvent {
    pub device_generation: u32,
    pub iface: u8,
    pub ep_in: u8,
    pub t1_irq: u64,
    pub data: Vec<u8>,
}

impl MonitorReportEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        const NAME: &str = "MONITOR_REPORT event";
        expect_min_len(NAME, data, MONITOR_REPORT_HEADER_LEN)?;
        let mut r = ByteReader::new(data);
        let device_generation = r.u32();
        let iface = r.u8();
        let ep_in = r.u8();
        let length = usize::from(r.u16());
        if length > MAX_REPORT_LENGTH {
            return Err(invalid(NAME, format!("length {length} exceeds 1024")));
        }
        expect_len(NAME, data, MONITOR_REPORT_HEADER_LEN + length)?;
        Ok(Self {
            device_generation,
            iface,
            ep_in,
            t1_irq: r.u64(),
            data: r.bytes(length).to_vec(),
        })
    }
}

pub const VALID_LATENCY: u8 = 1 << 0;
pub const VALID_RELEASE_LATENCY: u8 = 1 << 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrialResultEvent {
    pub run_id: u32,
    pub trial_index: u16,
    pub fail_kind: u8,
    pub release_fail_kind: u8,
    pub speed: u8,
    pub c1_status: u8,
    pub measurement_kind: u8,
    pub valid_flags: u8,
    pub random_delay_ticks: u32,
    pub t0_sched: u64,
    pub t0_isr: u64,
    pub t0_tick: u64,
    pub t1_irq: u64,
    pub confirm_t1_irq: u64,
    pub c0: i32,
    pub c1: i32,
    pub c1_uncertainty: u32,
    pub t0_frame: u16,
    pub t0_uframe: u8,
    pub t1_frame: u16,
    pub t1_uframe: u8,
    pub release_t0_tick: u64,
    pub release_t1_irq: u64,
    pub report: Vec<u8>,
}

impl TrialResultEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        const NAME: &str = "TRIAL_RESULT event";
        expect_min_len(NAME, data, TRIAL_RESULT_HEADER_LEN)?;
        let report_length = usize::from(ByteReader::at(data, 92).u16());
        if report_length > MAX_REPORT_LENGTH {
            return Err(invalid(
                NAME,
                format!("report_length {report_length} exceeds 1024"),
            ));
        }
        expect_len(NAME, data, TRIAL_RESULT_HEADER_LEN + report_length)?;
        let mut r = ByteReader::new(data);
        let run_id = r.u32();
        let trial_index = r.u16();
        let fail_kind = r.u8();
        let release_fail_kind = r.u8();
        let speed = r.u8();
        let c1_status = r.u8();
        let measurement_kind = r.u8();
        let valid_flags = r.u8();
        let random_delay_ticks = r.u32();
        let t0_sched = r.u64();
        let t0_isr = r.u64();
        let t0_tick = r.u64();
        let t1_irq = r.u64();
        let confirm_t1_irq = r.u64();
        let c0 = r.i32();
        let c1 = r.i32();
        let c1_uncertainty = r.u32();
        let t0_frame = r.u16();
        let t0_uframe = r.u8();
        r.skip(1);
        let t1_frame = r.u16();
        let t1_uframe = r.u8();
        r.skip(1);
        let release_t0_tick = r.u64();
        let release_t1_irq = r.u64();
        r.skip(2);
        debug_assert_eq!(r.position(), TRIAL_RESULT_HEADER_LEN);
        Ok(Self {
            run_id,
            trial_index,
            fail_kind,
            release_fail_kind,
            speed,
            c1_status,
            measurement_kind,
            valid_flags,
            random_delay_ticks,
            t0_sched,
            t0_isr,
            t0_tick,
            t1_irq,
            confirm_t1_irq,
            c0,
            c1,
            c1_uncertainty,
            t0_frame,
            t0_uframe,
            t1_frame,
            t1_uframe,
            release_t0_tick,
            release_t1_irq,
            report: r.bytes(report_length).to_vec(),
        })
    }

    pub fn latency_valid(&self) -> bool {
        self.valid_flags & VALID_LATENCY != 0
    }

    pub fn release_latency_valid(&self) -> bool {
        self.valid_flags & VALID_RELEASE_LATENCY != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunDoneEvent {
    pub run_id: u32,
    pub requested: u16,
    pub completed: u16,
    pub succeeded: u16,
    pub valid_latency_count: u16,
    pub fail_counts: [u16; FAIL_KIND_COUNT],
    pub completion_reason: u8,
}

impl RunDoneEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("RUN_DONE event", data, RUN_DONE_LEN)?;
        let mut r = ByteReader::new(data);
        let run_id = r.u32();
        let requested = r.u16();
        let completed = r.u16();
        let succeeded = r.u16();
        let valid_latency_count = r.u16();
        let mut fail_counts = [0_u16; FAIL_KIND_COUNT];
        for count in &mut fail_counts {
            *count = r.u16();
        }
        Ok(Self {
            run_id,
            requested,
            completed,
            succeeded,
            valid_latency_count,
            fail_counts,
            completion_reason: r.u8(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PollResultEvent {
    pub run_id: u32,
    pub completion_reason: u8,
    pub raw_truncated: u8,
    pub long_truncated: u8,
    pub duration_ms: u32,
    pub data_count: u32,
    pub nak_count: u32,
    pub error_count: u32,
    pub interval_count: u32,
    pub overflow_count: u32,
    pub bin_width: u32,
    pub bin_count: u32,
    pub min_interval: u64,
    pub max_interval: u64,
    pub sum_below_50ms: u64,
    pub count_below_50ms: u32,
    pub long_threshold: u32,
    pub long_count: u32,
    pub raw_count: u32,
    pub first_data_time: u64,
}

impl PollResultEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("POLL_RESULT event", data, POLL_RESULT_LEN)?;
        let mut r = ByteReader::new(data);
        let run_id = r.u32();
        let completion_reason = r.u8();
        let raw_truncated = r.u8();
        let long_truncated = r.u8();
        r.skip(1);
        Ok(Self {
            run_id,
            completion_reason,
            raw_truncated,
            long_truncated,
            duration_ms: r.u32(),
            data_count: r.u32(),
            nak_count: r.u32(),
            error_count: r.u32(),
            interval_count: r.u32(),
            overflow_count: r.u32(),
            bin_width: r.u32(),
            bin_count: r.u32(),
            min_interval: r.u64(),
            max_interval: r.u64(),
            sum_below_50ms: r.u64(),
            count_below_50ms: r.u32(),
            long_threshold: r.u32(),
            long_count: r.u32(),
            raw_count: r.u32(),
            first_data_time: r.u64(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropCountersEvent {
    pub drop_monitor: u32,
    pub drop_log: u32,
}

impl DropCountersEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("DROP_COUNTERS event", data, DROP_COUNTERS_LEN)?;
        let mut r = ByteReader::new(data);
        Ok(Self {
            drop_monitor: r.u32(),
            drop_log: r.u32(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveredEvent {
    pub cause: u8,
    pub result: u8,
}

impl RecoveredEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("RECOVERED event", data, RECOVERED_LEN)?;
        Ok(Self {
            cause: data[0],
            result: data[1],
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub code: u16,
    pub detail: u32,
}

impl ErrorEvent {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        expect_len("ERROR event", data, ERROR_EVENT_LEN)?;
        let mut r = ByteReader::new(data);
        let code = r.u16();
        r.skip(2);
        Ok(Self {
            code,
            detail: r.u32(),
        })
    }
}

/// A decoded event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TesterEvent {
    Log(LogEvent),
    DeviceAttached(DeviceEvent),
    DeviceDetached(DeviceEvent),
    EnumProgress(EnumProgressEvent),
    DeviceReady(DeviceReadyEvent),
    MonitorReport(MonitorReportEvent),
    TrialResult(TrialResultEvent),
    RunDone(RunDoneEvent),
    PollResult(PollResultEvent),
    DropCounters(DropCountersEvent),
    Recovered(RecoveredEvent),
    Error(ErrorEvent),
    Unknown { event_type: u8, payload: Vec<u8> },
}

impl TesterEvent {
    pub fn decode(event_type: u8, payload: &[u8]) -> Result<Self, DecodeError> {
        Ok(match event_type {
            EVT_LOG => Self::Log(LogEvent::decode(payload)?),
            EVT_DEVICE_ATTACHED => Self::DeviceAttached(DeviceEvent::decode(payload)?),
            EVT_DEVICE_DETACHED => Self::DeviceDetached(DeviceEvent::decode(payload)?),
            EVT_ENUM_PROGRESS => Self::EnumProgress(EnumProgressEvent::decode(payload)?),
            EVT_DEVICE_READY => Self::DeviceReady(DeviceReadyEvent::decode(payload)?),
            EVT_MONITOR_REPORT => Self::MonitorReport(MonitorReportEvent::decode(payload)?),
            EVT_TRIAL_RESULT => Self::TrialResult(TrialResultEvent::decode(payload)?),
            EVT_RUN_DONE => Self::RunDone(RunDoneEvent::decode(payload)?),
            EVT_POLL_RESULT => Self::PollResult(PollResultEvent::decode(payload)?),
            EVT_DROP_COUNTERS => Self::DropCounters(DropCountersEvent::decode(payload)?),
            EVT_RECOVERED => Self::Recovered(RecoveredEvent::decode(payload)?),
            EVT_ERROR => Self::Error(ErrorEvent::decode(payload)?),
            other => Self::Unknown {
                event_type: other,
                payload: payload.to_vec(),
            },
        })
    }


}

// ---------------------------------------------------------------------------
// 5. Enumeration trace (GET_OBJECT kind 6)

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumPause {
    pub endpoint: u8,
    pub cause_result: u8,
    pub consecutive_errors: u32,
    pub resume_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumResume {
    pub endpoint: u8,
    pub consecutive_errors: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumReenumerate {
    pub address: u8,
    pub time: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumRecord {
    pub record_length: u16,
    /// 0 transaction, 1 reset start, 2 reset end, 3 attach, 4 detach,
    /// 5 speed decided, 6 NAK run (summary).
    pub kind: u8,
    pub address: u8,
    pub time: u64,
    /// bit7 = IN.
    pub endpoint: u8,
    /// 0 SETUP, 1 IN, 2 OUT.
    pub token: u8,
    /// 0 ACK, 1 NAK, 2 STALL, 3 DATA0, 4 DATA1, 5 timeout, 6 other.
    pub result: u8,
    pub toggle: u8,
    pub step: u16,
    pub data_length: u16,
    pub late_ticks: i32,
    pub nak_count: u32,
    pub nak_last_time: u64,
    pub setup: [u8; 8],
    /// Transaction completion time. Version 1 records decode this as 0.
    pub done_time: u64,
    pub data: Vec<u8>,
    pub open_nak_run: bool,
    pub pause: Option<EnumPause>,
    pub resume: Option<EnumResume>,
    pub reenumerate: Option<EnumReenumerate>,
    pub host_profile: Option<HostProfile>,
    pub vendor_state: Option<u8>,
    pub vendor_code: Option<u8>,
}

impl EnumRecord {
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            0 => "transaction",
            1 => "reset_start",
            2 => "reset_end",
            3 => "attach",
            4 => "detach",
            5 => "speed",
            6 => "nak_run",
            7 => "poll_pause",
            8 => "poll_resume",
            9 => "reenumerate",
            10 => "host_profile",
            _ => "unknown",
        }
    }

    pub fn token_name(&self) -> &'static str {
        match self.token {
            0 => "SETUP",
            1 => "IN",
            2 => "OUT",
            _ => "unknown",
        }
    }

    pub fn result_name(&self) -> &'static str {
        match self.result {
            0 => "ACK",
            1 => "NAK",
            2 => "STALL",
            3 => "DATA0",
            4 => "DATA1",
            5 => "timeout",
            6 => "other",
            _ => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumTrace {
    pub record_version: u16,
    pub record_count: u16,
    pub dropped_records: u32,
    pub device_generation: Option<u32>,
    pub flags: Option<u8>,
    pub open_nak_count: Option<u8>,
    pub records: Vec<EnumRecord>,
}

impl EnumTrace {

    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        const NAME: &str = "enumeration trace";
        expect_min_len(NAME, data, 2)?;
        let mut r = ByteReader::new(data);
        let record_version = r.u16();
        let (header_len, fixed_len) = match record_version {
            1 => (ENUM_TRACE_V1_HEADER_LEN, ENUM_RECORD_V1_FIXED_LEN),
            2 => (ENUM_TRACE_V2_HEADER_LEN, ENUM_RECORD_V2_FIXED_LEN),
            other => return Err(invalid(NAME, format!("unsupported record_version {other}"))),
        };
        expect_min_len(NAME, data, header_len)?;
        let record_count = r.u16();
        let dropped_records = r.u32();
        let (device_generation, flags, open_nak_count) = if record_version == 2 {
            let values = (Some(r.u32()), Some(r.u8()), Some(r.u8()));
            r.skip(2);
            values
        } else {
            (None, None, None)
        };
        let mut records = Vec::with_capacity(usize::from(record_count));
        let mut pos = header_len;
        for index in 0..record_count {
            if data.len() < pos + fixed_len {
                return Err(invalid(NAME, format!("record {index} is truncated")));
            }
            let mut r = ByteReader::at(data, pos);
            let record_length = r.u16();
            let length = usize::from(record_length);
            if length < fixed_len || pos + length > data.len() {
                return Err(invalid(
                    NAME,
                    format!("record {index} has invalid record_length {record_length}"),
                ));
            }
            let kind = r.u8();
            let address = r.u8();
            let time = r.u64();
            let endpoint = r.u8();
            let token = r.u8();
            let result = r.u8();
            let toggle = r.u8();
            let step = r.u16();
            let data_length = r.u16();
            let late_ticks = r.i32();
            let nak_count = r.u32();
            let nak_last_time = r.u64();
            let setup = r.array::<8>();
            let done_time = if record_version == 2 { r.u64() } else { 0 };
            let data_saved = usize::from(r.u8());
            if data_saved > 64 {
                return Err(invalid(
                    NAME,
                    format!("record {index} data_saved {data_saved} exceeds 64"),
                ));
            }
            if fixed_len + data_saved > length {
                return Err(invalid(
                    NAME,
                    format!("record {index} data_saved {data_saved} exceeds its length"),
                ));
            }
            if fixed_len + data_saved != length {
                return Err(invalid(
                    NAME,
                    format!(
                        "record {index} record_length {record_length} does not match data_saved {data_saved}"
                    ),
                ));
            }
            let saved = r.bytes(data_saved).to_vec();
            if kind == 9
                && (record_length != ENUM_RECORD_V2_FIXED_LEN as u16
                    || endpoint != 0
                    || token != 0
                    || result != 0
                    || toggle != 0
                    || step != 0
                    || data_length != 0
                    || late_ticks != 0
                    || nak_count != 0
                    || nak_last_time != 0
                    || setup != [0; 8]
                    || done_time != 0)
            {
                return Err(invalid(
                    NAME,
                    format!("record {index} has invalid kind 9 fields"),
                ));
            }
            if kind == 10
                && (record_length != (ENUM_RECORD_V2_FIXED_LEN + 10) as u16
                    || address != 0
                    || endpoint != 0
                    || token != 0
                    || result != 0
                    || toggle != 0
                    || step != 0
                    || data_length != 0
                    || late_ticks != 0
                    || nak_count != 0
                    || nak_last_time != 0
                    || setup != [0; 8]
                    || done_time != 0)
            {
                return Err(invalid(
                    NAME,
                    format!("record {index} has invalid kind 10 fields"),
                ));
            }
            let (pause, resume, reenumerate) = match kind {
                7 => (
                    Some(EnumPause {
                        endpoint,
                        cause_result: result,
                        consecutive_errors: nak_count,
                        resume_at: nak_last_time,
                    }),
                    None,
                    None,
                ),
                8 => (
                    None,
                    Some(EnumResume {
                        endpoint,
                        consecutive_errors: nak_count,
                    }),
                    None,
                ),
                9 => (None, None, Some(EnumReenumerate { address, time })),
                _ => (None, None, None),
            };
            let (host_profile, vendor_state, vendor_code) = if kind == 10 {
                if data_saved != 10 {
                    return Err(invalid(
                        NAME,
                        format!("record {index} kind 10 has data_saved {data_saved}, expected 10"),
                    ));
                }
                (
                    Some(HostProfile::decode(&saved[..8])?),
                    Some(saved[8]),
                    Some(saved[9]),
                )
            } else {
                (None, None, None)
            };
            records.push(EnumRecord {
                record_length,
                kind,
                address,
                time,
                endpoint,
                token,
                result,
                toggle,
                step,
                data_length,
                late_ticks,
                nak_count,
                nak_last_time,
                setup,
                done_time,
                data: saved,
                open_nak_run: false,
                pause,
                resume,
                reenumerate,
                host_profile,
                vendor_state,
                vendor_code,
            });
            pos += length;
        }
        if pos != data.len() {
            return Err(invalid(
                NAME,
                format!("{} trailing bytes after the records", data.len() - pos),
            ));
        }
        let open_count = usize::from(open_nak_count.unwrap_or_default());
        if flags.is_some_and(|flags| flags & 1 != u8::from(open_count != 0)) {
            return Err(invalid(NAME, "flags bit 0 does not match open_nak_count"));
        }
        if open_count > records.len()
            || records[records.len().saturating_sub(open_count)..]
                .iter()
                .any(|record| record.kind != 6)
        {
            return Err(invalid(
                NAME,
                format!("open_nak_count {open_count} does not name trailing kind 6 records"),
            ));
        }
        for record in records.iter_mut().rev().take(open_count) {
            record.open_nak_run = true;
        }
        Ok(Self {
            record_version,
            record_count,
            dropped_records,
            device_generation,
            flags,
            open_nak_count,
            records,
        })
    }
}
