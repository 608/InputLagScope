//! Pulling objects from the tester: GET_OBJECT, then
//! READ_OBJECT in chunks, then a CRC-32/ISO-HDLC check of the whole object.
//! An object invalidated in the middle of a read (STALE_OBJECT from
//! READ_OBJECT) or a CRC mismatch restarts the sequence from GET_OBJECT.
//! STALE_OBJECT from GET_OBJECT means the source (device generation or
//! run_id) is no longer current and is returned to the caller.

use super::protocol::{
    self as proto, GetObjectRequest, ObjectInfo, ReadObjectRequest, MAX_OBJECT_LENGTH,
    STATUS_STALE_OBJECT,
};
use super::transport::{crc32_iso_hdlc, TesterClient, TesterError};

/// Largest chunk requested with READ_OBJECT (1..=1024).
pub const READ_CHUNK: u16 = proto::MAX_READ_LENGTH;
/// Attempts of the whole GET_OBJECT + READ_OBJECT sequence.
pub const MAX_OBJECT_ATTEMPTS: usize = 3;

enum ReadFailure {
    Retry(TesterError),
    Fatal(TesterError),
}

/// Reads one object completely and verifies its CRC-32.
pub fn read_object(
    client: &TesterClient,
    kind: u8,
    index: u8,
    source_id: u32,
) -> Result<Vec<u8>, TesterError> {
    read_object_with_info(client, kind, index, source_id).map(|(_, data)| data)
}

pub fn read_object_with_info(
    client: &TesterClient,
    kind: u8,
    index: u8,
    source_id: u32,
) -> Result<(ObjectInfo, Vec<u8>), TesterError> {
    let _guard = client.lock_objects();
    let mut last_error = None;
    for _ in 0..MAX_OBJECT_ATTEMPTS {
        let info = client.get_object(&GetObjectRequest {
            kind,
            index,
            source_id,
        })?;
        if info.kind != kind || info.index != index || info.source_id != source_id {
            return Err(TesterError::Protocol(format!(
                "GET_OBJECT answered kind {} index {} source {} for kind {kind} index {index} source {source_id}",
                info.kind, info.index, info.source_id
            )));
        }
        if info.total_length > MAX_OBJECT_LENGTH {
            return Err(TesterError::Protocol(format!(
                "object total_length {} exceeds the 64 KiB object limit",
                info.total_length
            )));
        }
        match read_chunks(client, &info) {
            Ok(data) => return Ok((info, data)),
            Err(ReadFailure::Retry(error)) => last_error = Some(error),
            Err(ReadFailure::Fatal(error)) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| TesterError::Protocol("object read failed".to_string())))
}

fn read_chunks(client: &TesterClient, info: &ObjectInfo) -> Result<Vec<u8>, ReadFailure> {
    let total = info.total_length as usize;
    let mut data = Vec::with_capacity(total);
    while data.len() < total {
        let offset = data.len() as u32;
        let response = client
            .read_object(&ReadObjectRequest {
                object_id: info.object_id,
                max_length: READ_CHUNK,
                offset,
            })
            .map_err(|error| {
                if error.is_status(STATUS_STALE_OBJECT) {
                    ReadFailure::Retry(error)
                } else {
                    ReadFailure::Fatal(error)
                }
            })?;
        let expected_len = usize::from(READ_CHUNK).min(total - data.len());
        if response.object_id != info.object_id
            || response.offset != offset
            || response.total_length != info.total_length
            || response.crc32 != info.crc32
            || response.data.len() != expected_len
        {
            return Err(ReadFailure::Retry(TesterError::Protocol(format!(
                "READ_OBJECT answer does not match the object (id {} offset {} total {} crc 0x{:08X} len {}; expected id {} offset {offset} total {} crc 0x{:08X} len {expected_len})",
                response.object_id,
                response.offset,
                response.total_length,
                response.crc32,
                response.data.len(),
                info.object_id,
                info.total_length,
                info.crc32
            ))));
        }
        data.extend_from_slice(&response.data);
    }
    let crc = crc32_iso_hdlc(&data);
    if crc != info.crc32 {
        return Err(ReadFailure::Retry(TesterError::Protocol(format!(
            "object CRC-32 mismatch: announced 0x{:08X}, computed 0x{crc:08X}",
            info.crc32
        ))));
    }
    Ok(data)
}

/// Decodes a string descriptor as received (bLength, bDescriptorType = 3,
/// UTF-16LE text). `None` when bLength is below 2 or beyond the received
/// bytes; a trailing odd byte is ignored.
pub fn decode_string_descriptor(raw: &[u8]) -> Option<String> {
    if raw.len() < 2 || raw[1] != 0x03 {
        return None;
    }
    let length = usize::from(raw[0]);
    if length < 2 || length > raw.len() {
        return None;
    }
    let units = raw[2..length]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let text = String::from_utf16_lossy(&units);
    let text = text.trim_end_matches('\0').trim().to_string();
    (!text.is_empty()).then_some(text)
}
