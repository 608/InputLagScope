//! Framing and session layer of the CH32H417 tester PC protocol.
//!
//! * COBS encoding with a single 0x00 delimiter
//! * 12-byte header + payload (<= 1152) + CRC-16/CCITT-FALSE
//! * HELLO with a random 64-bit host nonce, session_id filtering
//! * one outstanding command, resend with the same seq after 1 s
//!
//! A reader thread decodes incoming frames. Responses are handed to the
//! thread waiting in `TesterClient::command`; events of the current session
//! are decoded and broadcast to every subscriber.
//!
//! Some client methods (CALIBRATION_MODE, ENTER_BOOTLOADER, link
//! statistics) are part of the protocol API without a GUI command yet.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{bounded, unbounded, Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;
use serde::Serialize;
use thiserror::Error;

use super::protocol::{
    self as proto, command_name, status_name, Calibration, DeviceInfoResponse, EnumProgressEvent,
    ErrorEvent, GetObjectRequest, HelloRequest, HelloResponse, HostProfile, MonitorStart,
    ObjectInfo, PollTestStart, ReadObjectRequest, ReadObjectResponse, RecoveredEvent, RunIdRequest,
    StatusResponse, TesterEvent, TrialStart, TriggerManual, VendorEntry, WatchSpec,
};

pub const PROTOCOL_VERSION: u8 = 1;
pub const HEADER_LEN: usize = 12;
pub const CRC_LEN: usize = 2;
pub const MAX_PAYLOAD: usize = 1152;
/// COBS-encoded maximum (1166 + ceil(1166 / 254)).
pub const MAX_ENCODED_FRAME: usize = 1171;
/// Receive buffer including the delimiter.
pub const RX_BUFFER_LEN: usize = MAX_ENCODED_FRAME + 1;

pub const DEFAULT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(1);
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
pub const TESTER_BAUD_RATE: u32 = 115_200;

// ---------------------------------------------------------------------------
// CRC

/// CRC-16/CCITT-FALSE: poly 0x1021, init 0xFFFF, no reflection, no final XOR.
pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc = 0xFFFF_u16;
    for byte in data {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

const fn make_crc32_table() -> [u32; 256] {
    let mut table = [0_u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

static CRC32_TABLE: [u32; 256] = make_crc32_table();

/// CRC-32/ISO-HDLC (same as zlib.crc32).
pub fn crc32_iso_hdlc(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in data {
        crc = CRC32_TABLE[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------------------
// COBS

/// Canonical COBS (Cheshire & Baker): when the data ends exactly with a
/// full block of 254 non-zero bytes, no extra code byte is appended.
pub fn cobs_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 254 + 2);
    let mut code_index = Some(0);
    out.push(0);
    let mut code = 1_u8;
    for (index, byte) in data.iter().enumerate() {
        if *byte != 0 {
            out.push(*byte);
            code += 1;
        }
        if *byte == 0 || code == 0xFF {
            if let Some(position) = code_index {
                out[position] = code;
            }
            code = 1;
            if *byte == 0 || index + 1 < data.len() {
                code_index = Some(out.len());
                out.push(0);
            } else {
                code_index = None;
            }
        }
    }
    if let Some(position) = code_index {
        out[position] = code;
    }
    out
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CobsError {
    #[error("COBS code byte is zero")]
    ZeroCode,
    #[error("COBS block runs past the end of the frame")]
    Truncated,
    #[error("COBS block contains a zero byte")]
    ZeroInBlock,
}

pub fn cobs_decode(data: &[u8]) -> Result<Vec<u8>, CobsError> {
    let mut out = Vec::with_capacity(data.len());
    let mut index = 0;
    while index < data.len() {
        let code = data[index];
        if code == 0 {
            return Err(CobsError::ZeroCode);
        }
        index += 1;
        let block = usize::from(code) - 1;
        if index + block > data.len() {
            return Err(CobsError::Truncated);
        }
        let bytes = &data[index..index + block];
        if bytes.contains(&0) {
            return Err(CobsError::ZeroInBlock);
        }
        out.extend_from_slice(bytes);
        index += block;
        if code != 0xFF && index < data.len() {
            out.push(0);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Frame

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub version: u8,
    pub frame_type: u8,
    pub flags: u8,
    pub status: u8,
    pub seq: u16,
    pub session_id: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FrameError {
    #[error("COBS decode failed: {0}")]
    Cobs(#[from] CobsError),
    #[error("frame is shorter than header and CRC ({0} bytes)")]
    TooShort(usize),
    #[error("payload length {0} exceeds 1152")]
    PayloadTooLong(usize),
    #[error("decoded length {actual} does not match 12 + length + 2 = {expected}")]
    LengthMismatch { expected: usize, actual: usize },
    #[error("CRC mismatch: frame 0x{received:04X}, computed 0x{computed:04X}")]
    Crc { received: u16, computed: u16 },
}

impl Frame {
    pub fn new(frame_type: u8, seq: u16, session_id: u32, payload: Vec<u8>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            frame_type,
            flags: 0,
            status: 0,
            seq,
            session_id,
            payload,
        }
    }

    /// Decoded form: header, payload and CRC-16.
    pub fn to_decoded_bytes(&self) -> Result<Vec<u8>, FrameError> {
        if self.payload.len() > MAX_PAYLOAD {
            return Err(FrameError::PayloadTooLong(self.payload.len()));
        }
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len() + CRC_LEN);
        out.push(self.version);
        out.push(self.frame_type);
        out.push(self.flags);
        out.push(self.status);
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&(self.payload.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.session_id.to_le_bytes());
        out.extend_from_slice(&self.payload);
        let crc = crc16_ccitt_false(&out);
        out.extend_from_slice(&crc.to_le_bytes());
        Ok(out)
    }

    pub fn from_decoded_bytes(bytes: &[u8]) -> Result<Self, FrameError> {
        if bytes.len() < HEADER_LEN + CRC_LEN {
            return Err(FrameError::TooShort(bytes.len()));
        }
        let length = usize::from(u16::from_le_bytes([bytes[6], bytes[7]]));
        if length > MAX_PAYLOAD {
            return Err(FrameError::PayloadTooLong(length));
        }
        let expected = HEADER_LEN + length + CRC_LEN;
        if bytes.len() != expected {
            return Err(FrameError::LengthMismatch {
                expected,
                actual: bytes.len(),
            });
        }
        let body = &bytes[..HEADER_LEN + length];
        let received = u16::from_le_bytes([bytes[expected - 2], bytes[expected - 1]]);
        let computed = crc16_ccitt_false(body);
        if received != computed {
            return Err(FrameError::Crc { received, computed });
        }
        Ok(Self {
            version: bytes[0],
            frame_type: bytes[1],
            flags: bytes[2],
            status: bytes[3],
            seq: u16::from_le_bytes([bytes[4], bytes[5]]),
            session_id: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            payload: bytes[HEADER_LEN..HEADER_LEN + length].to_vec(),
        })
    }

    /// Wire form: COBS-encoded frame followed by the 0x00 delimiter.
    pub fn to_wire(&self) -> Result<Vec<u8>, FrameError> {
        let mut out = cobs_encode(&self.to_decoded_bytes()?);
        out.push(0);
        Ok(out)
    }

    pub fn is_response(&self) -> bool {
        (0x81..=0xBF).contains(&self.frame_type)
    }

    pub fn is_event(&self) -> bool {
        (0x40..=0x7F).contains(&self.frame_type)
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct DecoderStats {
    pub frames: u64,
    pub cobs_errors: u64,
    pub length_errors: u64,
    pub crc_errors: u64,
    pub overflows: u64,
}

/// Splits a byte stream at 0x00 delimiters and decodes frames. USB packet
/// boundaries are not frame boundaries. Accumulated data longer than
/// the receive buffer is dropped up to the next delimiter.
#[derive(Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
    discarding: bool,
    pub stats: DecoderStats,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(RX_BUFFER_LEN),
            ..Self::default()
        }
    }

    pub fn push(&mut self, data: &[u8]) -> Vec<Frame> {
        let mut frames = Vec::new();
        for byte in data {
            if *byte == 0 {
                if self.discarding {
                    self.discarding = false;
                } else if !self.buf.is_empty() {
                    if let Some(frame) = self.decode_buffer() {
                        frames.push(frame);
                    }
                }
                self.buf.clear();
                continue;
            }
            if self.discarding {
                continue;
            }
            if self.buf.len() >= MAX_ENCODED_FRAME {
                self.stats.overflows += 1;
                self.discarding = true;
                self.buf.clear();
                continue;
            }
            self.buf.push(*byte);
        }
        frames
    }

    fn decode_buffer(&mut self) -> Option<Frame> {
        let decoded = match cobs_decode(&self.buf) {
            Ok(decoded) => decoded,
            Err(_) => {
                self.stats.cobs_errors += 1;
                return None;
            }
        };
        match Frame::from_decoded_bytes(&decoded) {
            Ok(frame) => {
                self.stats.frames += 1;
                Some(frame)
            }
            Err(FrameError::Crc { .. }) => {
                self.stats.crc_errors += 1;
                None
            }
            Err(_) => {
                self.stats.length_errors += 1;
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Errors

#[derive(Debug, Error)]
pub enum TesterError {
    #[error("tester returned {} (0x{status:02X}) for {}", status_name(*.status), command_name(*.command))]
    Status { command: u8, status: u8 },
    #[error("no response from tester for {} after {attempts} attempt(s)", command_name(*.command))]
    Timeout { command: u8, attempts: u32 },
    #[error("tester link closed: {0}")]
    Disconnected(String),
    #[error("tester protocol error: {0}")]
    Protocol(String),
    #[error("tester I/O error: {0}")]
    Io(#[from] io::Error),
}

impl TesterError {
    pub fn status(&self) -> Option<u8> {
        match self {
            Self::Status { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn is_status(&self, status: u8) -> bool {
        self.status() == Some(status)
    }

    /// True when it is unknown whether the tester executed the command: no
    /// response (lost or link down) or a response that could not be used.
    /// A status response means the tester answered, so the outcome is known.
    pub fn outcome_unknown(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. } | Self::Disconnected(_) | Self::Io(_) | Self::Protocol(_)
        )
    }
}

impl From<proto::DecodeError> for TesterError {
    fn from(error: proto::DecodeError) -> Self {
        Self::Protocol(error.to_string())
    }
}

impl From<FrameError> for TesterError {
    fn from(error: FrameError) -> Self {
        Self::Protocol(error.to_string())
    }
}

// ---------------------------------------------------------------------------
// Client

pub type LinkReader = Box<dyn Read + Send>;
pub type LinkWriter = Box<dyn Write + Send>;

#[derive(Clone, Debug)]
pub struct ClientOptions {
    /// Time to wait for a response before resending (1.4: 1 s).
    pub response_timeout: Duration,
    /// Total number of transmissions of one command (first send included).
    pub max_attempts: u32,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            response_timeout: DEFAULT_RESPONSE_TIMEOUT,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LinkStats {
    pub decoder: DecoderStats,
    pub responses: u64,
    pub events: u64,
    pub dropped_other_session: u64,
    pub dropped_bad_version: u64,
    pub dropped_unexpected_type: u64,
    pub event_decode_errors: u64,
    pub stale_responses: u64,
    pub resends: u64,
}

/// State tracked from events of the current session.
#[derive(Clone, Debug, Default, Serialize)]
pub struct LiveState {
    pub device_generation: Option<u32>,
    pub attached: Option<bool>,
    pub speed: Option<u8>,
    pub ready_generation: Option<u32>,
    pub classification: Option<u8>,
    pub last_enum_progress: Option<EnumProgressEvent>,
    pub drop_monitor: u32,
    pub drop_log: u32,
    pub last_error: Option<ErrorEvent>,
    pub recovered_count: u32,
    pub last_recovered: Option<RecoveredEvent>,
    pub logs: VecDeque<String>,
}

const LIVE_LOG_LIMIT: usize = 100;

struct PendingHello {
    seq: u16,
    nonce: u64,
}

struct Shared {
    session_id: AtomicU32,
    response_tx: Sender<Frame>,
    response_rx: Receiver<Frame>,
    subscribers: Mutex<Vec<Sender<TesterEvent>>>,
    alive: AtomicBool,
    stop: AtomicBool,
    close_reason: Mutex<Option<String>>,
    stats: Mutex<LinkStats>,
    live: Mutex<LiveState>,
    pending_hello: Mutex<Option<PendingHello>>,
}

impl Shared {
    fn dispatch(&self, frame: Frame) {
        if frame.version != PROTOCOL_VERSION {
            self.stats.lock().dropped_bad_version += 1;
            return;
        }
        let current = self.session_id.load(Ordering::Acquire);
        if frame.is_response() {
            if frame.frame_type == (proto::CMD_HELLO | proto::RESPONSE_FLAG) {
                self.accept_hello_response(&frame);
            } else if current == 0 || frame.session_id != current {
                self.stats.lock().dropped_other_session += 1;
                return;
            }
            self.stats.lock().responses += 1;
            let _ = self.response_tx.try_send(frame);
        } else if frame.is_event() {
            if current == 0 || frame.session_id != current {
                self.stats.lock().dropped_other_session += 1;
                return;
            }
            match TesterEvent::decode(frame.frame_type, &frame.payload) {
                Ok(event) => {
                    self.stats.lock().events += 1;
                    self.update_live(&event);
                    let mut subscribers = self.subscribers.lock();
                    subscribers.retain(|subscriber| subscriber.send(event.clone()).is_ok());
                }
                Err(_) => self.stats.lock().event_decode_errors += 1,
            }
        } else {
            self.stats.lock().dropped_unexpected_type += 1;
        }
    }

    /// Switches to the new session as soon as the matching HELLO response is
    /// seen, so that events sent right after it are not dropped.
    fn accept_hello_response(&self, frame: &Frame) {
        if frame.status != proto::STATUS_OK {
            return;
        }
        let pending = self.pending_hello.lock();
        let Some(pending) = pending.as_ref() else {
            return;
        };
        if frame.seq != pending.seq {
            return;
        }
        let Ok(hello) = HelloResponse::decode(&frame.payload) else {
            return;
        };
        if hello.magic == proto::HELLO_MAGIC
            && hello.protocol_version == PROTOCOL_VERSION
            && hello.host_nonce_echo == pending.nonce
            && hello.session_id != 0
            && hello.session_id == frame.session_id
        {
            self.session_id.store(hello.session_id, Ordering::Release);
            *self.live.lock() = LiveState::default();
        }
    }

    fn update_live(&self, event: &TesterEvent) {
        let mut live = self.live.lock();
        match event {
            TesterEvent::DeviceAttached(event) => {
                live.device_generation = Some(event.device_generation);
                live.attached = Some(true);
                live.speed = Some(event.speed);
            }
            TesterEvent::DeviceDetached(event) => {
                live.device_generation = Some(event.device_generation);
                live.attached = Some(false);
                live.speed = Some(event.speed);
                live.ready_generation = None;
            }
            TesterEvent::EnumProgress(event) => {
                live.device_generation = Some(event.device_generation);
                live.last_enum_progress = Some(event.clone());
            }
            TesterEvent::DeviceReady(event) => {
                live.device_generation = Some(event.device_generation);
                live.ready_generation = Some(event.device_generation);
                live.classification = Some(event.classification);
                live.attached = Some(true);
            }
            TesterEvent::DropCounters(event) => {
                live.drop_monitor = event.drop_monitor;
                live.drop_log = event.drop_log;
            }
            TesterEvent::Recovered(event) => {
                live.recovered_count = live.recovered_count.saturating_add(1);
                live.last_recovered = Some(event.clone());
            }
            TesterEvent::Error(event) => live.last_error = Some(event.clone()),
            TesterEvent::Log(event) => {
                live.logs
                    .push_back(format!("[{}] {}", event.level, event.text));
                while live.logs.len() > LIVE_LOG_LIMIT {
                    live.logs.pop_front();
                }
            }
            _ => {}
        }
    }

    fn close(&self, reason: String) {
        {
            let mut close_reason = self.close_reason.lock();
            if close_reason.is_none() {
                *close_reason = Some(reason);
            }
        }
        self.alive.store(false, Ordering::Release);
        // Dropping the senders wakes every subscriber with `Disconnected`.
        self.subscribers.lock().clear();
    }
}

struct CommandState {
    next_seq: u16,
}

/// A connection to one tester (one COM port open = one session).
pub struct TesterClient {
    port_name: String,
    shared: Arc<Shared>,
    writer: Mutex<LinkWriter>,
    command_lock: Mutex<CommandState>,
    object_lock: Mutex<()>,
    run_id: Mutex<u32>,
    hello: Mutex<Option<HelloResponse>>,
    c1_uncertainties: Mutex<[u32; 2]>,
    options: ClientOptions,
    reader: Mutex<Option<JoinHandle<()>>>,
}

static NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Random 64-bit host nonce. `RandomState` is seeded from the operating
/// system's random source; time, process id and a counter are mixed in.
pub fn random_nonce() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    hasher.write_u32(std::process::id());
    hasher.write_u64(NONCE_COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.finish()
}

impl TesterClient {
    pub fn new(
        port_name: impl Into<String>,
        reader: LinkReader,
        writer: LinkWriter,
        options: ClientOptions,
    ) -> Self {
        let (response_tx, response_rx) = bounded(64);
        let shared = Arc::new(Shared {
            session_id: AtomicU32::new(0),
            response_tx,
            response_rx,
            subscribers: Mutex::new(Vec::new()),
            alive: AtomicBool::new(true),
            stop: AtomicBool::new(false),
            close_reason: Mutex::new(None),
            stats: Mutex::new(LinkStats::default()),
            live: Mutex::new(LiveState::default()),
            pending_hello: Mutex::new(None),
        });
        let thread_shared = shared.clone();
        let join = thread::Builder::new()
            .name("tester-reader".to_string())
            .spawn(move || reader_loop(thread_shared, reader))
            .expect("failed to spawn tester reader thread");
        Self {
            port_name: port_name.into(),
            shared,
            writer: Mutex::new(writer),
            command_lock: Mutex::new(CommandState { next_seq: 1 }),
            object_lock: Mutex::new(()),
            run_id: Mutex::new(0),
            hello: Mutex::new(None),
            c1_uncertainties: Mutex::new([proto::C1_UNCERTAINTY_UNKNOWN; 2]),
            options,
            reader: Mutex::new(Some(join)),
        }
    }

    /// Opens a COM port. The caller then starts a session with `hello`.
    pub fn open_serial(port_name: &str, options: ClientOptions) -> Result<Self, TesterError> {
        let mut port = serialport::new(port_name, TESTER_BAUD_RATE)
            .timeout(Duration::from_millis(20))
            .open()
            .map_err(|error| TesterError::Io(io::Error::other(error.to_string())))?;
        // CDC-ACM devices commonly gate transmission on DTR.
        let _ = port.write_data_terminal_ready(true);
        let _ = port.clear(serialport::ClearBuffer::All);
        let reader = port
            .try_clone()
            .map_err(|error| TesterError::Io(io::Error::other(error.to_string())))?;
        Ok(Self::new(
            port_name,
            Box::new(reader),
            Box::new(port),
            options,
        ))
    }

    pub fn port_name(&self) -> &str {
        &self.port_name
    }

    pub fn session_id(&self) -> u32 {
        self.shared.session_id.load(Ordering::Acquire)
    }

    pub fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::Acquire)
    }

    pub fn close_reason(&self) -> Option<String> {
        self.shared.close_reason.lock().clone()
    }

    /// Receiver of every decoded event of the current session.
    pub fn subscribe(&self) -> Receiver<TesterEvent> {
        let (tx, rx) = unbounded();
        if self.is_alive() {
            self.shared.subscribers.lock().push(tx);
        }
        rx
    }

    /// Next run_id of this session (shared by trials and polling tests,
    /// strictly increasing, 1.5). The counter never wraps: when it is
    /// exhausted the connection is closed, so that the next operation opens
    /// a new session (HELLO), which starts again from run_id 1.
    pub fn next_run_id(&self) -> Result<u32, TesterError> {
        let next = {
            let mut run_id = self.run_id.lock();
            let next = run_id.checked_add(1);
            if let Some(next) = next {
                *run_id = next;
            }
            next
        };
        match next {
            Some(next) => Ok(next),
            None => {
                self.close();
                Err(TesterError::Disconnected(
                    "run_id is exhausted in this session; the connection was closed and the \
                     next operation starts a new session (HELLO)"
                        .to_string(),
                ))
            }
        }
    }

    /// Serializes GET_OBJECT/READ_OBJECT sequences (the tester keeps one
    /// object at a time).
    pub fn lock_objects(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.object_lock.lock()
    }

    /// Stops the reader thread and releases the port (both handles), even
    /// while other threads still hold this client.
    pub fn close(&self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(join) = self.reader.lock().take() {
            let _ = join.join();
        }
        self.shared.close("closed by host".to_string());
        *self.writer.lock() = Box::new(io::sink());
    }

    fn ensure_alive(&self) -> Result<(), TesterError> {
        if self.is_alive() {
            Ok(())
        } else {
            Err(TesterError::Disconnected(
                self.close_reason()
                    .unwrap_or_else(|| "link closed".to_string()),
            ))
        }
    }

    fn write_wire(&self, wire: &[u8]) -> Result<(), TesterError> {
        let mut writer = self.writer.lock();
        writer
            .write_all(wire)
            .and_then(|_| writer.flush())
            .map_err(|error| TesterError::Disconnected(format!("write failed: {error}")))
    }

    /// Sends a command and waits for its response (one outstanding command;
    /// resend with the same seq after `response_timeout`). Returns the whole
    /// response frame, whatever its status.
    fn transact(&self, command: u8, payload: &[u8]) -> Result<Frame, TesterError> {
        if payload.len() > MAX_PAYLOAD {
            return Err(TesterError::Protocol(format!(
                "payload of {} is {} bytes (max 1152)",
                command_name(command),
                payload.len()
            )));
        }
        let mut state = self.command_lock.lock();
        self.ensure_alive()?;
        let session_id = if command == proto::CMD_HELLO {
            0
        } else {
            let session_id = self.session_id();
            if session_id == 0 {
                return Err(TesterError::Protocol(
                    "no session; HELLO has not completed".to_string(),
                ));
            }
            session_id
        };
        let seq = state.next_seq;
        state.next_seq = state.next_seq.wrapping_add(1);
        let frame = Frame::new(command, seq, session_id, payload.to_vec());
        self.send_and_wait(&frame)
    }

    fn send_and_wait(&self, frame: &Frame) -> Result<Frame, TesterError> {
        let wire = frame.to_wire()?;
        while self.shared.response_rx.try_recv().is_ok() {
            self.shared.stats.lock().stale_responses += 1;
        }
        let expected_type = frame.frame_type | proto::RESPONSE_FLAG;
        let attempts = self.options.max_attempts.max(1);
        for attempt in 0..attempts {
            if attempt > 0 {
                self.shared.stats.lock().resends += 1;
            }
            self.write_wire(&wire)?;
            let deadline = Instant::now() + self.options.response_timeout;
            loop {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let wait = (deadline - now).min(Duration::from_millis(50));
                match self.shared.response_rx.recv_timeout(wait) {
                    Ok(response) => {
                        if response.seq == frame.seq && response.frame_type == expected_type {
                            return Ok(response);
                        }
                        self.shared.stats.lock().stale_responses += 1;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        self.ensure_alive()?;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err(TesterError::Disconnected("response channel closed".into()))
                    }
                }
            }
            self.ensure_alive()?;
        }
        Err(TesterError::Timeout {
            command: frame.frame_type,
            attempts,
        })
    }

    /// Sends a command and returns the payload of an OK response. Non-OK
    /// responses become `TesterError::Status`.
    pub fn command(&self, command: u8, payload: &[u8]) -> Result<Vec<u8>, TesterError> {
        let response = self.transact(command, payload)?;
        if response.status != proto::STATUS_OK {
            return Err(TesterError::Status {
                command,
                status: response.status,
            });
        }
        Ok(response.payload)
    }

    fn command_empty(&self, command: u8, payload: &[u8]) -> Result<(), TesterError> {
        let response = self.command(command, payload)?;
        if !response.is_empty() {
            return Err(TesterError::Protocol(format!(
                "{} response has {} bytes, expected 0",
                command_name(command),
                response.len()
            )));
        }
        Ok(())
    }

    // -- HELLO -------------------------------------------------------------

    /// Starts a new session with a fresh random nonce.
    pub fn hello(&self) -> Result<HelloResponse, TesterError> {
        self.hello_with_nonce(random_nonce())
    }

    pub fn hello_with_nonce(&self, nonce: u64) -> Result<HelloResponse, TesterError> {
        let request = HelloRequest {
            host_protocol_version: proto::HOST_PROTOCOL_VERSION,
            host_nonce: nonce,
        };
        let payload = request.encode();
        let mut state = self.command_lock.lock();
        self.ensure_alive()?;
        let seq = state.next_seq;
        state.next_seq = state.next_seq.wrapping_add(1);
        *self.shared.pending_hello.lock() = Some(PendingHello { seq, nonce });
        let frame = Frame::new(proto::CMD_HELLO, seq, 0, payload);
        let result = self.send_and_wait(&frame);
        *self.shared.pending_hello.lock() = None;
        drop(state);
        let response = result?;
        if response.status != proto::STATUS_OK {
            return Err(TesterError::Status {
                command: proto::CMD_HELLO,
                status: response.status,
            });
        }
        let hello = HelloResponse::decode(&response.payload)?;
        if hello.magic != proto::HELLO_MAGIC {
            return Err(TesterError::Protocol(format!(
                "HELLO magic 0x{:08X} is not 'LATT'",
                hello.magic
            )));
        }
        if hello.protocol_version != PROTOCOL_VERSION {
            return Err(TesterError::Protocol(format!(
                "protocol_version {} is not supported",
                hello.protocol_version
            )));
        }
        if hello.host_nonce_echo != nonce {
            return Err(TesterError::Protocol(
                "HELLO host_nonce_echo does not match".to_string(),
            ));
        }
        if hello.session_id == 0 || hello.session_id != response.session_id {
            return Err(TesterError::Protocol(format!(
                "HELLO session_id {} does not match header {}",
                hello.session_id, response.session_id
            )));
        }
        // The raw intervals of a polling test must fit in one object.
        if u64::from(hello.max_poll_raw_count) * 4 > proto::MAX_OBJECT_LENGTH as u64 {
            // The reader already switched to this session; make it unusable.
            self.shared.session_id.store(0, Ordering::Release);
            return Err(TesterError::Protocol(format!(
                "max_poll_raw_count {} x 4 bytes exceeds the 64 KiB object limit",
                hello.max_poll_raw_count
            )));
        }
        self.shared
            .session_id
            .store(hello.session_id, Ordering::Release);
        *self.run_id.lock() = 0;
        *self.hello.lock() = Some(hello.clone());
        Ok(hello)
    }

    // -- typed commands ----------------------------------------------------

    pub fn get_status(&self) -> Result<StatusResponse, TesterError> {
        Ok(StatusResponse::decode(
            &self.command(proto::CMD_GET_STATUS, &[])?,
        )?)
    }

    pub fn get_device_info(&self) -> Result<DeviceInfoResponse, TesterError> {
        Ok(DeviceInfoResponse::decode(
            &self.command(proto::CMD_GET_DEVICE_INFO, &[])?,
        )?)
    }

    pub fn get_object(&self, request: &GetObjectRequest) -> Result<ObjectInfo, TesterError> {
        Ok(ObjectInfo::decode(
            &self.command(proto::CMD_GET_OBJECT, &request.encode())?,
        )?)
    }

    pub fn read_object(
        &self,
        request: &ReadObjectRequest,
    ) -> Result<ReadObjectResponse, TesterError> {
        Ok(ReadObjectResponse::decode(
            &self.command(proto::CMD_READ_OBJECT, &request.encode())?,
        )?)
    }

    pub fn get_host_profile(&self) -> Result<HostProfile, TesterError> {
        Ok(HostProfile::decode(
            &self.command(proto::CMD_GET_HOST_PROFILE, &[])?,
        )?)
    }

    pub fn set_vendor_table(&self, entries: &[VendorEntry]) -> Result<(), TesterError> {
        if entries.len() > proto::MAX_VENDOR_ENTRIES {
            return Err(TesterError::Protocol(format!(
                "vendor table has {} entries (max 64)",
                entries.len()
            )));
        }
        self.command_empty(
            proto::CMD_SET_VENDOR_TABLE,
            &proto::encode_vendor_table(entries),
        )
    }

    pub fn get_vendor_table(&self) -> Result<Vec<VendorEntry>, TesterError> {
        Ok(proto::decode_vendor_table(
            &self.command(proto::CMD_GET_VENDOR_TABLE, &[])?,
        )?)
    }

    pub fn reenumerate(&self) -> Result<(), TesterError> {
        self.command_empty(proto::CMD_REENUMERATE, &[])
    }

    pub fn trigger_manual(&self, assert_mask: u8, release_mask: u8) -> Result<(), TesterError> {
        self.command_empty(
            proto::CMD_TRIGGER_MANUAL,
            &TriggerManual {
                assert_mask,
                release_mask,
            }
            .encode(),
        )
    }

    pub fn monitor_start(&self, request: &MonitorStart) -> Result<(), TesterError> {
        self.command_empty(proto::CMD_MONITOR_START, &request.encode())
    }

    pub fn monitor_stop(&self) -> Result<(), TesterError> {
        self.command_empty(proto::CMD_MONITOR_STOP, &[])
    }

    pub fn set_watch(&self, watch: &WatchSpec) -> Result<(), TesterError> {
        watch
            .validate()
            .map_err(|message| TesterError::Protocol(format!("invalid watch spec: {message}")))?;
        self.command_empty(proto::CMD_SET_WATCH, &watch.encode())
    }

    pub fn trial_start(&self, request: &TrialStart) -> Result<(), TesterError> {
        if request.measure_release > 1 {
            return Err(TesterError::Protocol(
                "invalid TRIAL_START: measure_release must be 0 or 1".to_string(),
            ));
        }
        if request.random_max_us > 10_000_000 {
            return Err(TesterError::Protocol(
                "invalid TRIAL_START: random_max_us must be at most 10000000".to_string(),
            ));
        }
        self.sync_calibration_for_attached_speed()?;
        self.command_empty(proto::CMD_TRIAL_START, &request.encode())
    }

    pub fn trial_stop(&self, run_id: u32) -> Result<(), TesterError> {
        self.command_empty(proto::CMD_TRIAL_STOP, &RunIdRequest { run_id }.encode())
    }

    pub fn poll_test_start(&self, request: &PollTestStart) -> Result<(), TesterError> {
        self.command_empty(proto::CMD_POLL_TEST_START, &request.encode())
    }

    pub fn poll_test_stop(&self, run_id: u32) -> Result<(), TesterError> {
        self.command_empty(proto::CMD_POLL_TEST_STOP, &RunIdRequest { run_id }.encode())
    }

    pub fn c1_uncertainty_for_speed(&self, speed: u8) -> u32 {
        match speed {
            2 => self.c1_uncertainties.lock()[0],
            3 => self.c1_uncertainties.lock()[1],
            _ => proto::C1_UNCERTAINTY_UNKNOWN,
        }
    }

    /// Read the live speed before every run, including after a reconnect.
    pub fn sync_calibration_for_attached_speed(&self) -> Result<(), TesterError> {
        let uncertainty = self.c1_uncertainty_for_speed(self.get_status()?.speed);
        let mut calibration = self.get_calibration()?;
        if calibration.c1_uncertainty != uncertainty {
            calibration.c1_uncertainty = uncertainty;
            self.set_calibration(&calibration)?;
        }
        Ok(())
    }

    pub fn set_calibration(&self, calibration: &Calibration) -> Result<(), TesterError> {
        self.command_empty(proto::CMD_SET_CALIBRATION, &calibration.encode())
    }

    pub fn get_calibration(&self) -> Result<Calibration, TesterError> {
        Ok(Calibration::decode(
            &self.command(proto::CMD_GET_CALIBRATION, &[])?,
        )?)
    }
}

impl Drop for TesterClient {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(join) = self.reader.lock().take() {
            let _ = join.join();
        }
    }
}

fn reader_loop(shared: Arc<Shared>, mut reader: LinkReader) {
    let mut decoder = FrameDecoder::new();
    let mut buf = [0_u8; 512];
    while !shared.stop.load(Ordering::Acquire) {
        match reader.read(&mut buf) {
            Ok(0) => {
                shared.close("end of stream".to_string());
                break;
            }
            Ok(count) => {
                let frames = decoder.push(&buf[..count]);
                shared.stats.lock().decoder = decoder.stats.clone();
                for frame in frames {
                    shared.dispatch(frame);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => {
                shared.close(format!("read failed: {error}"));
                break;
            }
        }
    }
    shared.alive.store(false, Ordering::Release);
    shared.subscribers.lock().clear();
    let mut reason = shared.close_reason.lock();
    if reason.is_none() {
        *reason = Some("reader stopped".to_string());
    }
}
