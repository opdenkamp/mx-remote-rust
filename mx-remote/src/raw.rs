// Author: Lars Op den Kamp (lars@opdenkamp-it.nl)
// Copyright (c) 2026 Op den Kamp IT Solutions

//! Whole frames as bytes, for a caller relaying them between the mesh and
//! somewhere else: [`EventHandler::on_mesh_frame`](crate::EventHandler::on_mesh_frame)
//! hands over each frame on the mesh, and
//! [`Remote::send_raw`](crate::Remote::send_raw) puts one on it.
//!
//! A frame is a 24-byte header followed by its payload:
//!
//! ```text
//! [0]      0x50 'P'
//! [1]      0x38 '8'
//! [2:4]    protocol version (u16 LE)
//! [4:20]   sender device uid
//! [20:22]  opcode (u16 LE)
//! [22:24]  payload length (u16 LE)
//! [24:]    payload
//! ```

use crate::rx::{accepted, HELLO_LEN};
use crate::wire::{DeviceFeature, DeviceUid, Frame, HEADER_LEN, MAGIC};

/// Whether a frame came in from the mesh or went out to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Received from another device.
    Received,
    /// Sent by this client.
    Sent,
}

/// A mesh endpoint: the socket a frame came in on or went out by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Endpoint {
    /// The discovery group, or a client's one endpoint on a group of its own
    /// ([`Config::target_ip`](crate::Config::target_ip)).
    Group,
    /// The subnet broadcast endpoint, or a client's one endpoint on a
    /// broadcast or unicast address.
    Broadcast,
}

/// Where [`Remote::send_raw`](crate::Remote::send_raw) puts a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RawEndpoints {
    /// Both endpoints, as a unit sends a frame for every unit.
    Both,
    /// The discovery group alone.
    Group,
}

/// The opcode of a hello (`SYS_HELLO`).
pub const OP_HELLO: u16 = 0x00;

/// Where a hello's features sit in its payload.
const HELLO_FEATURES_AT: usize = 50;

/// A frame's header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    /// The protocol version the frame is stamped with: the version its
    /// opcode's layout needs, not the sender's own.
    pub protocol: u16,
    /// The device that sent it.
    pub sender: DeviceUid,
    /// Its opcode.
    pub opcode: u16,
    /// The payload length it declares.
    pub payload_len: u16,
}

impl FrameHeader {
    /// How long the header is.
    pub const LEN: usize = HEADER_LEN;

    /// Reads the header at the start of `frame`: `None` when it is shorter
    /// than the header or does not start `P8`. Nothing past the header is
    /// looked at.
    pub fn parse(frame: &[u8]) -> Option<Self> {
        Frame::parse(frame).ok().map(Self::of)
    }

    /// Reads the header of a frame a unit takes, as this client takes one:
    /// one that [`parse`](Self::parse)s, is exactly as long as its header
    /// declares, and is stamped with a protocol version a unit reads.
    pub fn parse_accepted(frame: &[u8]) -> Option<Self> {
        accepted(frame).map(Self::of)
    }

    /// Writes this header over the first [`LEN`](Self::LEN) bytes of
    /// `frame`, magic included. `false`, and nothing written, when `frame`
    /// is shorter.
    pub fn write_into(&self, frame: &mut [u8]) -> bool {
        let Some(header) = frame.get_mut(..HEADER_LEN) else {
            return false;
        };
        header[..2].copy_from_slice(&MAGIC);
        header[2..4].copy_from_slice(&self.protocol.to_le_bytes());
        header[4..20].copy_from_slice(self.sender.as_bytes());
        header[20..22].copy_from_slice(&self.opcode.to_le_bytes());
        header[22..24].copy_from_slice(&self.payload_len.to_le_bytes());
        true
    }

    /// How long the whole frame is by its header.
    pub fn frame_len(&self) -> usize {
        HEADER_LEN + usize::from(self.payload_len)
    }

    fn of(frame: Frame<'_>) -> Self {
        Self {
            protocol: frame.protocol(),
            sender: frame.remote_id(),
            opcode: frame.opcode().0,
            payload_len: frame.payload_len(),
        }
    }
}

/// Where the features of the hello `frame` carries are, when it carries one
/// a unit takes: a frame with the hello opcode whose payload arrived whole
/// and is at least the 54 bytes a unit reads.
fn hello_features_at(frame: &[u8]) -> Option<usize> {
    let header = FrameHeader::parse(frame)?;
    let whole = frame.len() >= header.frame_len();
    let hello = header.opcode == OP_HELLO && usize::from(header.payload_len) >= HELLO_LEN;
    (whole && hello).then_some(HEADER_LEN + HELLO_FEATURES_AT)
}

/// The features a hello announces, `None` for a frame that is not a hello a
/// unit takes.
pub fn hello_features(frame: &[u8]) -> Option<DeviceFeature> {
    let at = hello_features_at(frame)?;
    let bytes = frame.get(at..at + 4)?;
    let bits = u32::from_le_bytes(<[u8; 4]>::try_from(bytes).ok()?);
    Some(DeviceFeature::from_bits(bits))
}

/// Replaces the features a hello announces. `false`, and the frame left as
/// it was, for a frame that is not a hello a unit takes.
pub fn set_hello_features(frame: &mut [u8], features: DeviceFeature) -> bool {
    let Some(at) = hello_features_at(frame) else {
        return false;
    };
    match frame.get_mut(at..at + 4) {
        Some(field) => {
            field.copy_from_slice(&features.bits().to_le_bytes());
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{build_hello, PROTOCOL_VERSION};

    const SENDER: DeviceUid = DeviceUid::from_array([0x5A; 16]);

    fn hello(features: DeviceFeature) -> Vec<u8> {
        let payload = build_hello(
            PROTOCOL_VERSION,
            "app",
            "P9SN00000001",
            "1.0",
            features.bits(),
        );
        let mut frame = vec![0; HEADER_LEN];
        let header = FrameHeader {
            protocol: 0x01,
            sender: SENDER,
            opcode: OP_HELLO,
            payload_len: u16::try_from(payload.len()).unwrap(),
        };
        assert!(header.write_into(&mut frame));
        frame.extend_from_slice(&payload);
        frame
    }

    /// The header is written where a unit reads it, and reads back the same.
    #[test]
    fn a_header_reads_back_as_written() {
        let header = FrameHeader {
            protocol: 0x1D,
            sender: SENDER,
            opcode: 0x0158,
            payload_len: 0x0203,
        };
        let mut frame = [0xEE; 26];
        assert!(header.write_into(&mut frame));
        let mut expected = vec![0x50, 0x38, 0x1D, 0x00];
        expected.extend_from_slice(&[0x5A; 16]);
        expected.extend_from_slice(&[0x58, 0x01, 0x03, 0x02, 0xEE, 0xEE]);
        assert_eq!(frame.to_vec(), expected);
        assert_eq!(FrameHeader::parse(&frame), Some(header));
        assert_eq!(header.frame_len(), 24 + 0x0203);
    }

    /// A buffer shorter than the header takes nothing.
    #[test]
    fn a_short_buffer_takes_no_header() {
        let mut frame = [0xEE; 23];
        assert!(!FrameHeader::parse(&hello(DeviceFeature::MANAGER))
            .unwrap()
            .write_into(&mut frame));
        assert_eq!(frame, [0xEE; 23]);
    }

    /// Only a frame starting `P8` and holding a whole header parses.
    #[test]
    fn only_a_header_parses() {
        let frame = hello(DeviceFeature::MANAGER);
        assert!(FrameHeader::parse(&frame[..24]).is_some());
        assert_eq!(FrameHeader::parse(&frame[..23]), None);
        let mut bad = frame.clone();
        bad[1] = b'9';
        assert_eq!(FrameHeader::parse(&bad), None);
    }

    /// A frame a unit takes is exactly as long as it declares and stamped
    /// with a version a unit reads.
    #[test]
    fn an_accepted_frame_is_whole_and_readable() {
        let frame = hello(DeviceFeature::MANAGER);
        assert!(FrameHeader::parse_accepted(&frame).is_some());

        let mut padded = frame.clone();
        padded.push(0);
        assert_eq!(FrameHeader::parse_accepted(&padded), None, "padded");
        assert_eq!(
            FrameHeader::parse_accepted(&frame[..frame.len() - 1]),
            None,
            "truncated"
        );
        for protocol in [PROTOCOL_VERSION + 1, 2] {
            let mut stamped = frame.clone();
            stamped[2..4].copy_from_slice(&protocol.to_le_bytes());
            assert_eq!(
                FrameHeader::parse_accepted(&stamped),
                None,
                "stamped {protocol:#x}"
            );
        }
    }

    /// A hello's features are read and replaced in place, and nothing else
    /// in it moves.
    #[test]
    fn a_hellos_features_are_read_and_replaced() {
        let mut frame = hello(DeviceFeature::MANAGER | DeviceFeature::OSD_SYNC);
        assert_eq!(
            hello_features(&frame),
            Some(DeviceFeature::MANAGER | DeviceFeature::OSD_SYNC)
        );
        assert!(set_hello_features(&mut frame, DeviceFeature::MESH_MEMBER));
        assert_eq!(hello_features(&frame), Some(DeviceFeature::MESH_MEMBER));
        assert_eq!(frame, hello(DeviceFeature::MESH_MEMBER));
    }

    /// A frame that is not a hello a unit takes has no features to read or
    /// replace: another opcode, a hello declaring under 54 bytes, or one cut
    /// short. The last two still carry bytes where the features would be.
    #[test]
    fn only_a_hello_has_features() {
        let frame = hello(DeviceFeature::MANAGER);
        let mut other = frame.clone();
        other[20] = 0x01;
        let mut short = frame.clone();
        short[22..24].copy_from_slice(&53u16.to_le_bytes());
        let mut cut = frame.clone();
        cut[22..24].copy_from_slice(&60u16.to_le_bytes());
        cut.extend_from_slice(&[0; 4]);
        for (what, mut frame) in [("other", other), ("short", short), ("cut", cut)] {
            let before = frame.clone();
            assert_eq!(hello_features(&frame), None, "{what}");
            assert!(
                !set_hello_features(&mut frame, DeviceFeature::MESH),
                "{what}"
            );
            assert_eq!(frame, before, "{what}");
        }
    }
}
