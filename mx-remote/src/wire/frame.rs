// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The 24-byte frame header, and bounds-checked access to the payload behind it.

use core::fmt;

use super::opcode::Opcode;
use super::uid::{DeviceUid, UID_LEN};

/// Width of the frame header.
pub(crate) const HEADER_LEN: usize = 24;

const MAGIC: [u8; 2] = [0x50, 0x38]; // 'P', '8'

/// Width of the fixed-size device name field on the wire (`MXR_DEVICE_NAME_LEN`).
///
/// A value that fills the field leaves no room for a terminator, so read
/// exactly the field width and only then cut at a NUL - scanning on runs into
/// the neighbouring struct member.
pub(crate) const DEVICE_NAME_LEN: usize = 16;

/// Width of the fixed-size firmware version field on the wire (`MXR_FW_VERSION_LEN`).
pub(crate) const FW_VERSION_LEN: usize = 128;

/// A datagram that does not carry a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameError {
    /// Shorter than the header.
    TooShort(usize),
    /// The first two bytes are not `P8`.
    BadMagic(u8, u8),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort(n) => write!(f, "invalid mx_remote frame (length = {n})"),
            Self::BadMagic(a, b) => write!(f, "invalid mx_remote frame (header = {a}:{b})"),
        }
    }
}

/// A decoded MX Remote wire frame: a 24-byte header followed by an
/// opcode-specific payload.
///
/// Wire layout:
///
/// ```text
/// [0]      0x50 'P'
/// [1]      0x38 '8'
/// [2:4]    protocol version (u16 LE)
/// [4:20]   sender device UID (16 bytes)
/// [20:22]  opcode (u16 LE)
/// [22:24]  payload length (u16 LE)
/// [24:]    payload
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frame<'a> {
    data: &'a [u8],
}

impl<'a> Frame<'a> {
    /// Reads the header of a received datagram. The payload is not inspected.
    pub(crate) fn parse(data: &'a [u8]) -> Result<Self, FrameError> {
        match (data.first(), data.get(1)) {
            _ if data.len() < HEADER_LEN => Err(FrameError::TooShort(data.len())),
            (Some(&a), Some(&b)) if [a, b] != MAGIC => Err(FrameError::BadMagic(a, b)),
            _ => Ok(Self { data }),
        }
    }

    /// The protocol version the sender stamped on this frame.
    pub(crate) fn protocol(&self) -> u16 {
        self.header_u16(2)
    }

    /// The UID of the device that sent this frame.
    pub(crate) fn remote_id(&self) -> DeviceUid {
        self.data
            .get(4..4 + UID_LEN)
            .and_then(|b| <[u8; UID_LEN]>::try_from(b).ok())
            .map(DeviceUid::from_array)
            .unwrap_or_default()
    }

    /// The opcode this frame carries.
    pub(crate) fn opcode(&self) -> Opcode {
        Opcode(self.header_u16(20))
    }

    /// The payload length the header declares, which is not necessarily the
    /// number of payload bytes that arrived.
    pub(crate) fn payload_len(&self) -> u16 {
        self.header_u16(22)
    }

    /// The frame payload, bounded by both the length the header declares and
    /// the bytes that actually arrived: a truncated datagram can claim more
    /// than it carries, and a padded one can carry more than it claims.
    pub(crate) fn payload(&self) -> &'a [u8] {
        let declared = HEADER_LEN.saturating_add(self.payload_len() as usize);
        let end = declared.min(self.data.len());
        self.data.get(HEADER_LEN..end).unwrap_or_default()
    }

    /// Reads a header field. The header is present for every `Frame`, so the
    /// fallback is unreachable rather than a decoding decision.
    fn header_u16(&self, idx: usize) -> u16 {
        self.data
            .get(idx..idx + 2)
            .and_then(|b| <[u8; 2]>::try_from(b).ok())
            .map(u16::from_le_bytes)
            .unwrap_or(0)
    }

    /// Borrows `len` payload bytes at `idx`, or `None` when fewer arrived.
    ///
    /// `idx` is relative to the start of the payload, and the bound is the
    /// bytes that arrived rather than the length the header declares: a
    /// truncated datagram must not be read past its end even when its header
    /// promises more.
    fn slice(&self, idx: usize, len: usize) -> Option<&'a [u8]> {
        let start = HEADER_LEN.checked_add(idx)?;
        let end = start.checked_add(len)?;
        self.data.get(start..end)
    }

    /// Reads a `u8` at `idx`.
    pub(crate) fn u8(&self, idx: usize) -> Option<u8> {
        self.slice(idx, 1).and_then(|b| b.first().copied())
    }

    /// Reads a byte at `idx` as a boolean. A byte other than 1, and a byte that
    /// did not arrive, are both false - matching a firmware sender that writes
    /// exactly 0 or 1.
    pub(crate) fn boolean(&self, idx: usize) -> bool {
        self.u8(idx) == Some(1)
    }

    /// Reads a little-endian `u16` at `idx`.
    pub(crate) fn u16(&self, idx: usize) -> Option<u16> {
        self.slice(idx, 2)
            .and_then(|b| <[u8; 2]>::try_from(b).ok())
            .map(u16::from_le_bytes)
    }

    /// Reads a little-endian `u32` at `idx`.
    pub(crate) fn u32(&self, idx: usize) -> Option<u32> {
        self.slice(idx, 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_le_bytes)
    }

    /// Reads an ASCII string from a fixed-width field, cutting at the first NUL.
    ///
    /// Exactly `len` bytes are taken, so a value filling its field never runs
    /// on into the next struct member.
    pub(crate) fn str(&self, idx: usize, len: usize) -> Option<String> {
        self.slice(idx, len).map(cstr)
    }

    /// Reads an ASCII string running from `idx` to the end of the datagram.
    pub(crate) fn str_to_end(&self, idx: usize) -> Option<String> {
        self.bytes_from(idx).map(cstr)
    }

    /// Reads a device UID at `idx`.
    pub(crate) fn uid(&self, idx: usize) -> Option<DeviceUid> {
        self.slice(idx, UID_LEN)
            .and_then(|b| <[u8; UID_LEN]>::try_from(b).ok())
            .map(DeviceUid::from_array)
    }

    /// Borrows everything from `idx` to the end of the datagram.
    pub(crate) fn bytes_from(&self, idx: usize) -> Option<&'a [u8]> {
        let start = HEADER_LEN.checked_add(idx)?;
        self.data.get(start..)
    }
}

/// Decodes a fixed-width ASCII wire field: the bytes up to the first NUL, or
/// all of `b` when the value fills the field and leaves no room for one.
///
/// A peer on older firmware can still put junk in such a field, so a non-ASCII
/// byte costs one character rather than the whole name.
pub(crate) fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b.get(..end)
        .unwrap_or_default()
        .iter()
        .map(|&c| {
            if c > 0x7F {
                char::REPLACEMENT_CHARACTER
            } else {
                c as char
            }
        })
        .collect()
}

/// Assembles a frame for transmission.
///
/// `protocol` is the version to stamp on the header, which is the opcode's own
/// minimum rather than the version this library speaks; see
/// [`protocol_for`](super::opcode::protocol_for).
pub(in crate::wire) fn build_frame(
    uid: DeviceUid,
    opcode: Opcode,
    protocol: u16,
    payload: &[u8],
) -> Vec<u8> {
    // A UDP datagram cannot carry more payload than the length field can
    // describe, so no caller reaches the saturating case.
    let declared = u16::try_from(payload.len()).unwrap_or(u16::MAX);

    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&protocol.to_le_bytes());
    out.extend_from_slice(uid.as_bytes());
    out.extend_from_slice(&opcode.0.to_le_bytes());
    out.extend_from_slice(&declared.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::opcode::op;

    const UID: DeviceUid =
        DeviceUid::from_array([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);

    /// Every payload byte differs from its neighbours, so a read at the right
    /// offset but the wrong width returns a wrong value rather than the same one.
    const PAYLOAD: [u8; 8] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];

    fn built() -> Vec<u8> {
        build_frame(UID, op::BAY_STATUS, 0x0F, &PAYLOAD)
    }

    #[test]
    fn a_built_frame_parses_back_to_its_header_fields() {
        let raw = built();
        let f = Frame::parse(&raw).expect("a frame this library built must parse");
        assert_eq!(f.protocol(), 0x0F);
        assert_eq!(f.remote_id(), UID);
        assert_eq!(f.opcode(), op::BAY_STATUS);
        assert_eq!(f.payload_len(), PAYLOAD.len() as u16);
        assert_eq!(f.payload(), PAYLOAD);
    }

    #[test]
    fn a_datagram_shorter_than_the_header_is_dropped() {
        let raw = built();
        for len in 0..HEADER_LEN {
            assert_eq!(
                Frame::parse(&raw[..len]),
                Err(FrameError::TooShort(len)),
                "a {len}-byte datagram was accepted"
            );
        }
    }

    #[test]
    fn a_datagram_that_is_not_p8_is_dropped() {
        let mut raw = built();
        raw[1] = b'9';
        assert_eq!(Frame::parse(&raw), Err(FrameError::BadMagic(0x50, b'9')));
    }

    #[test]
    fn a_truncated_payload_is_bounded_by_what_arrived() {
        let raw = built();
        let cut = &raw[..raw.len() - 3];
        let f = Frame::parse(cut).expect("the header still arrived in full");
        assert_eq!(f.payload_len(), 8, "the header still claims eight bytes");
        assert_eq!(f.payload(), &PAYLOAD[..5]);
        assert_eq!(f.u32(1), Some(0x55443322));
        assert_eq!(f.u32(2), None, "a read running past the datagram must fail");
    }

    #[test]
    fn a_padded_payload_is_bounded_by_the_declared_length() {
        let mut raw = built();
        raw.extend_from_slice(&[0xFF; 4]);
        let f = Frame::parse(&raw).expect("padding does not stop a frame parsing");
        assert_eq!(f.payload(), PAYLOAD);
    }

    #[test]
    fn accessors_refuse_to_read_past_the_datagram() {
        let raw = built();
        let f = Frame::parse(&raw).expect("a frame this library built must parse");
        assert_eq!(f.u8(7), Some(0x88));
        assert_eq!(f.u8(8), None);
        assert_eq!(f.u16(6), Some(0x8877));
        assert_eq!(f.u16(7), None);
        assert_eq!(f.u32(4), Some(0x88776655));
        assert_eq!(f.u32(5), None);
        assert_eq!(f.uid(0), None, "eight payload bytes cannot hold a uid");
        assert!(!f.boolean(8), "a byte that did not arrive is not true");
    }

    #[test]
    fn a_string_stops_at_its_field_width() {
        let raw = build_frame(UID, op::CHANGE_BAY_NAME, 0x06, b"ABCDEFGH");
        let f = Frame::parse(&raw).expect("a frame this library built must parse");
        assert_eq!(
            f.str(0, 4).as_deref(),
            Some("ABCD"),
            "a value filling its field ran on into the next one"
        );
        assert_eq!(
            f.str(0, 9),
            None,
            "a field wider than the payload must fail"
        );
        assert_eq!(f.str_to_end(4).as_deref(), Some("EFGH"));
    }

    #[test]
    fn a_string_stops_at_a_nul_inside_its_field() {
        let raw = build_frame(UID, op::CHANGE_BAY_NAME, 0x06, b"AB\0DEFGH");
        let f = Frame::parse(&raw).expect("a frame this library built must parse");
        assert_eq!(f.str(0, 8).as_deref(), Some("AB"));
    }

    #[test]
    fn a_non_ascii_byte_costs_one_character_not_the_whole_field() {
        assert_eq!(cstr(b"Ki\xFFchen"), "Ki\u{FFFD}chen");
    }
}
