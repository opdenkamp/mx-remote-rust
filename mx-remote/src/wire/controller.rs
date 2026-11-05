// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! The layout of `CONTROLLER_STATE` (0x58), which dedicated controllers use
//! between themselves and with which one marks itself as dedicated.
//!
//! Every frame opens with a 20-byte header: the payload version, the kind,
//! the part, a reserved byte, then the generation, offset, length and crc32,
//! each a u32. A hello follows it with the sender's role, the number of parts,
//! two reserved bytes, the primary's uid and a 12-byte entry per part.

use crate::types::{ControllerHello, ControllerPart, CONTROLLER_PARTS_MAX};

use super::enums::ControllerRole;
use super::uid::DeviceUid;

/// The payload version every `CONTROLLER_STATE` frame opens with; any other
/// is dropped.
pub(crate) const CONTROLLER_STATE_VERSION: u8 = 1;

/// Width of the `CONTROLLER_STATE` header.
pub(crate) const CONTROLLER_STATE_HEADER_SIZE: usize = 20;

/// The shortest hello a unit takes: the header, role, part count, reserved
/// bytes and primary, without any part.
pub(crate) const CONTROLLER_HELLO_MIN: usize = 40;

/// Width of one part entry of a hello.
pub(crate) const CONTROLLER_PART_SIZE: usize = 12;

/// The `CONTROLLER_STATE` kind at byte 1.
pub(crate) mod state_kind {
    pub(crate) const HELLO: u8 = 0;
    // Pulling another's state, which the failsafe work adds.
    #[allow(dead_code)]
    pub(crate) const REQUEST: u8 = 1;
    #[allow(dead_code)]
    pub(crate) const DATA: u8 = 2;
}

/// The 20-byte header every `CONTROLLER_STATE` frame opens with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StateHeader {
    pub(crate) kind: u8,
    /// REQUEST, DATA: the part; 0 in a hello.
    pub(crate) part: u8,
    pub(crate) generation: u32,
    /// REQUEST: the first byte asked for. DATA: this frame's offset.
    pub(crate) offset: u32,
    /// REQUEST: the bytes asked for. DATA: the whole part's length.
    pub(crate) length: u32,
    /// DATA: the crc32 of the whole part.
    pub(crate) crc: u32,
}

impl StateHeader {
    /// Lays the header out, with this library's payload version.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut p = Vec::with_capacity(CONTROLLER_STATE_HEADER_SIZE);
        p.extend_from_slice(&[CONTROLLER_STATE_VERSION, self.kind, self.part, 0]);
        for word in [self.generation, self.offset, self.length, self.crc] {
            p.extend_from_slice(&word.to_le_bytes());
        }
        p
    }

    /// Reads a header of payload version 1; `None` for a shorter payload or
    /// another version.
    pub(crate) fn decode(p: &[u8]) -> Option<Self> {
        if p.len() < CONTROLLER_STATE_HEADER_SIZE || p[0] != CONTROLLER_STATE_VERSION {
            return None;
        }
        let word = |at: usize| u32::from_le_bytes([p[at], p[at + 1], p[at + 2], p[at + 3]]);
        Some(Self {
            kind: p[1],
            part: p[2],
            generation: word(4),
            offset: word(8),
            length: word(12),
            crc: word(16),
        })
    }
}

/// Builds a `CONTROLLER_STATE` hello; only the first
/// [`CONTROLLER_PARTS_MAX`] parts are listed.
pub(crate) fn build_controller_hello(hello: &ControllerHello) -> Vec<u8> {
    let parts = &hello.parts[..hello.parts.len().min(CONTROLLER_PARTS_MAX)];
    let mut p = StateHeader {
        kind: state_kind::HELLO,
        generation: hello.generation,
        ..StateHeader::default()
    }
    .encode();
    p.extend_from_slice(&[hello.role.to_wire(), parts.len() as u8, 0, 0]);
    p.extend_from_slice(hello.primary.as_bytes());
    for part in parts {
        p.extend_from_slice(&[part.part, 0, 0, 0]);
        p.extend_from_slice(&part.length.to_le_bytes());
        p.extend_from_slice(&part.crc.to_le_bytes());
    }
    p
}

/// Reads a `CONTROLLER_STATE` hello: version 1, kind 0, at least
/// [`CONTROLLER_HELLO_MIN`] bytes. Only the part entries the payload holds
/// whole are read, up to the count it gives.
pub(crate) fn decode_controller_hello(p: &[u8]) -> Option<ControllerHello> {
    let header = StateHeader::decode(p)?;
    if header.kind != state_kind::HELLO || p.len() < CONTROLLER_HELLO_MIN {
        return None;
    }
    let mut primary = [0; 16];
    primary.copy_from_slice(&p[24..40]);
    let parts = p[CONTROLLER_HELLO_MIN..]
        .chunks_exact(CONTROLLER_PART_SIZE)
        .take(usize::from(p[21]))
        .map(|e| ControllerPart {
            part: e[0],
            length: u32::from_le_bytes([e[4], e[5], e[6], e[7]]),
            crc: u32::from_le_bytes([e[8], e[9], e[10], e[11]]),
        })
        .collect();
    Some(ControllerHello {
        role: ControllerRole::from_wire(p[20]),
        generation: header.generation,
        primary: DeviceUid::from_array(primary),
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(n: u8) -> DeviceUid {
        DeviceUid::from_array([n; 16])
    }

    /// A hello as routers.md lays it out: the 20-byte header with kind 0 and
    /// the generation, then role, part count, two reserved bytes, the
    /// primary and 12 bytes per part.
    #[test]
    fn a_controller_hello_is_laid_out_as_routers_md_says() {
        let hello = ControllerHello {
            role: ControllerRole::PRIMARY,
            generation: 0x1122_3344,
            primary: uid(0x5A),
            parts: vec![ControllerPart {
                part: 2,
                length: 0x0000_0A0B,
                crc: 0xDEAD_BEEF,
            }],
        };
        let p = build_controller_hello(&hello);
        let mut expected = vec![1, 0, 0, 0, 0x44, 0x33, 0x22, 0x11];
        expected.extend_from_slice(&[0; 12]);
        expected.extend_from_slice(&[1, 1, 0, 0]);
        expected.extend_from_slice(&[0x5A; 16]);
        expected.extend_from_slice(&[2, 0, 0, 0, 0x0B, 0x0A, 0, 0, 0xEF, 0xBE, 0xAD, 0xDE]);
        assert_eq!(p, expected);
        assert_eq!(decode_controller_hello(&p), Some(hello));
    }

    /// The default, sent before the application sets one: role 0, no
    /// primary, no parts, 40 bytes.
    #[test]
    fn the_default_hello_is_40_bytes() {
        let p = build_controller_hello(&ControllerHello::default());
        assert_eq!(p.len(), CONTROLLER_HELLO_MIN);
        assert!(p.iter().enumerate().all(|(i, b)| *b == u8::from(i == 0)));
    }

    /// Only the first five parts are sent.
    #[test]
    fn at_most_five_parts_are_sent() {
        let hello = ControllerHello {
            parts: vec![ControllerPart::default(); 7],
            ..ControllerHello::default()
        };
        let p = build_controller_hello(&hello);
        assert_eq!(p[21], 5);
        assert_eq!(p.len(), CONTROLLER_HELLO_MIN + 5 * CONTROLLER_PART_SIZE);
    }

    #[test]
    fn the_header_round_trips() {
        let header = StateHeader {
            kind: state_kind::DATA,
            part: 1,
            generation: 9,
            offset: 1024,
            length: 4096,
            crc: 0x0102_0304,
        };
        let p = header.encode();
        assert_eq!(p.len(), CONTROLLER_STATE_HEADER_SIZE);
        assert_eq!(StateHeader::decode(&p), Some(header));
        assert_eq!(StateHeader::decode(&p[..19]), None);
        let mut other = p.clone();
        other[0] = 2;
        assert_eq!(StateHeader::decode(&other), None);
    }
}
